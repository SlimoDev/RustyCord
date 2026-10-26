using System.Collections.ObjectModel;
using System.Net;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Threading.Channels;

namespace NotDiscord.Terminal;

/// <summary>Own-client functions. No Discord/Vencord runtime, telemetry, or token access.</summary>
public sealed class NativeFeatures : IDisposable, IAsyncDisposable
{
    private const long SegmentLimit = 4 * 1024 * 1024;
    public const long DefaultChannelHistoryLimit = 64 * 1024 * 1024;
    public const long DefaultTotalHistoryLimit = 256 * 1024 * 1024;
    private const int MaxRecordBytes = 1024 * 1024;
    private const int MaxJournalFiles = 4096;
    private readonly string directory;
    private readonly string historyDirectory;
    private readonly object pinLock = new();
    private readonly List<string> pins = new();
    private readonly Channel<JournalWrite> writes = Channel.CreateBounded<JournalWrite>(
        new BoundedChannelOptions(32) { SingleReader = true, FullMode = BoundedChannelFullMode.Wait });
    private readonly Task writer;
    private readonly SemaphoreSlim historyLock = new(1, 1);
    private readonly Dictionary<string, long> validatedJournalLengths = new(StringComparer.Ordinal);
    private int disposed;
    private static readonly Regex Token = new(@"https?://[^\s<>`""\x00-\x1F]+|(?<!\\)<(?<animated>a?):(?<name>[^:\s<>]{1,64}):(?<id>[0-9]{1,20})>", RegexOptions.Compiled | RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(200));
    private static readonly HashSet<string> TrackingKeys = new(StringComparer.OrdinalIgnoreCase)
    { "fbclid", "gclid", "dclid", "msclkid", "yclid", "mc_cid", "mc_eid", "igshid", "_hsenc", "_hsmi", "mkt_tok", "vero_id" };

    public NativeFeatures(string dataDirectory)
    {
        directory = Path.GetFullPath(dataDirectory);
        historyDirectory = Path.Combine(directory, "message-history");
        Directory.CreateDirectory(historyDirectory);
        string pinPath = Path.Combine(directory, "pins.dpapi");
        if (File.Exists(pinPath))
        {
            byte[] bytes = FeatureProtection.Unprotect(File.ReadAllBytes(pinPath));
            try
            {
                var restored = JsonSerializer.Deserialize<string[]>(bytes) ?? [];
                pins.AddRange(restored.Where(x => !string.IsNullOrWhiteSpace(x)).Distinct().Take(1000));
            }
            finally { CryptographicOperations.ZeroMemory(bytes); }
        }
        // Never remove history automatically. A full store produces a surfaced exception.
        string policy = Path.Combine(historyDirectory, "retention-policy.json");
        if (!File.Exists(policy))
            AtomicWrite(policy, JsonSerializer.SerializeToUtf8Bytes(new
            {
                segmentBytes = SegmentLimit, channelLimitBytes = DefaultChannelHistoryLimit,
                totalLimitBytes = DefaultTotalHistoryLimit, maxJournalFiles = MaxJournalFiles, encryption = "Windows DPAPI CurrentUser",
                atCapacity = "Stop recording and report HistoryCapacityException; existing history is retained."
            }));
        writer = Task.Run(WriteLoopAsync);
    }

    public IReadOnlyList<string> PinnedIds { get { lock (pinLock) return Array.AsReadOnly(pins.ToArray()); } }
    public bool IsPinned(string channelId) { lock (pinLock) return pins.Contains(channelId); }

    public void TogglePin(string channelId)
    {
        ObjectDisposedException.ThrowIf(Volatile.Read(ref disposed) != 0, this);
        ArgumentException.ThrowIfNullOrWhiteSpace(channelId);
        lock (pinLock)
        {
            var updated = new List<string>(pins);
            if (!updated.Remove(channelId))
            {
                if (updated.Count >= 1000) throw new InvalidOperationException("Límite de 1000 conversaciones fijadas alcanzado.");
                updated.Add(channelId);
            }
            byte[] plain = JsonSerializer.SerializeToUtf8Bytes(updated);
            try { AtomicWrite(Path.Combine(directory, "pins.dpapi"), FeatureProtection.Protect(plain)); }
            finally { CryptographicOperations.ZeroMemory(plain); }
            pins.Clear(); pins.AddRange(updated);
        }
    }

    /// <summary>External emoji fallback sends an image link, never a paid account entitlement.</summary>
    public string PrepareOutgoing(string text) => PrepareOutgoing(text, null);

    public string PrepareOutgoing(string text, Func<string, bool>? canUseNativeEmoji)
    {
        ArgumentNullException.ThrowIfNull(text);
        if (text.Length > 1_000_000) throw new ArgumentOutOfRangeException(nameof(text));
        var result = new StringBuilder(text.Length);
        int start = 0, cursor = 0;
        while (cursor < text.Length)
        {
            if (text[cursor] != '`' || IsEscaped(text, cursor)) { cursor++; continue; }
            result.Append(TransformPlainText(text[start..cursor], canUseNativeEmoji));
            int ticks = 1;
            while (cursor + ticks < text.Length && text[cursor + ticks] == '`') ticks++;
            int close = cursor + ticks;
            while (close < text.Length)
            {
                if (text[close] != '`' || IsEscaped(text, close)) { close++; continue; }
                int endTicks = 1;
                while (close + endTicks < text.Length && text[close + endTicks] == '`') endTicks++;
                if (endTicks == ticks) { close += ticks; break; }
                close += endTicks;
            }
            // Incomplete code is preserved too; drafts must not unexpectedly change.
            result.Append(text.AsSpan(cursor, close - cursor));
            cursor = close; start = close;
        }
        if (start < text.Length) result.Append(TransformPlainText(text[start..], canUseNativeEmoji));
        return result.ToString();
    }

    private static bool IsEscaped(string value, int index)
    {
        int slashes = 0;
        while (--index >= 0 && value[index] == '\\') slashes++;
        return slashes % 2 != 0;
    }

    private static string TransformPlainText(string text, Func<string, bool>? canUseNativeEmoji) => Token.Replace(text, match =>
    {
        if (match.Value.StartsWith("http", StringComparison.OrdinalIgnoreCase)) return CleanUrlToken(match.Value);
        if (canUseNativeEmoji?.Invoke(match.Groups["id"].Value) == true) return match.Value;
        string extension = match.Groups["animated"].Value.Length == 0 ? "png" : "gif";
        return $"https://cdn.discordapp.com/emojis/{match.Groups["id"].Value}.{extension}?size=48&name={Uri.EscapeDataString(match.Groups["name"].Value)}";
    });

    private static string CleanUrlToken(string value)
    {
        int end = value.Length;
        while (end > 0)
        {
            char last = value[end - 1];
            char opener = last switch { ')' => '(', ']' => '[', '}' => '{', _ => '\0' };
            if (last is '.' or ',' or ';') { end--; continue; }
            if (opener != '\0' && value[..end].Count(x => x == last) > value[..end].Count(x => x == opener)) { end--; continue; }
            break;
        }
        return StripTracking(value[..end]) + value[end..];
    }

    private static string StripTracking(string value)
    {
        int query = value.IndexOf('?');
        if (query < 0) return value;
        int hash = value.IndexOf('#');
        if (hash >= 0 && hash < query) return value;
        int end = hash < 0 ? value.Length : hash;
        string[] parts = value[(query + 1)..end].Split('&');
        // A query signature may cover every parameter, including a tracking-looking name.
        if (parts.Any(part =>
        {
            int eq = part.IndexOf('=');
            string key = WebUtility.UrlDecode(eq < 0 ? part : part[..eq]);
            return key.Equals("signature", StringComparison.OrdinalIgnoreCase) || key.Equals("sig", StringComparison.OrdinalIgnoreCase)
                || key.Equals("x-amz-signature", StringComparison.OrdinalIgnoreCase) || key.Equals("x-goog-signature", StringComparison.OrdinalIgnoreCase);
        })) return value;
        var kept = new List<string>(parts.Length);
        foreach (string part in parts)
        {
            int equals = part.IndexOf('=');
            string key = WebUtility.UrlDecode(equals < 0 ? part : part[..equals]);
            if (!key.StartsWith("utm_", StringComparison.OrdinalIgnoreCase) && !TrackingKeys.Contains(key)) kept.Add(part);
        }
        if (kept.Count == parts.Length) return value;
        return value[..query] + (kept.Count == 0 ? "" : "?" + string.Join('&', kept)) + value[end..];
    }

    public async Task RecordChangeAsync(ChatMessage previous, ChatMessage? updated, CancellationToken ct = default)
    {
        ObjectDisposedException.ThrowIf(Volatile.Read(ref disposed) != 0, this);
        ArgumentNullException.ThrowIfNull(previous);
        if (updated is not null && (updated.Id != previous.Id || updated.ChannelId != previous.ChannelId))
            throw new ArgumentException("La revisión debe pertenecer al mismo mensaje y canal.", nameof(updated));
        if (updated is not null && previous.Content == updated.Content && previous.Attachments.SequenceEqual(updated.Attachments)) return;
        var snapshot = previous with { Edited = updated is not null, Deleted = updated is null,
            Attachments = Array.AsReadOnly(previous.Attachments.ToArray()) };
        var operation = new JournalWrite(new LoggedChange(DateTimeOffset.UtcNow, snapshot), ct);
        await writes.Writer.WriteAsync(operation, ct).ConfigureAwait(false);
        // Acknowledgement means the encrypted record was flushed, or the error is delivered.
        await operation.Done.Task.ConfigureAwait(false);
    }

    private async Task WriteLoopAsync()
    {
        await foreach (var operation in writes.Reader.ReadAllAsync().ConfigureAwait(false))
        {
            try
            {
                operation.Cancellation.ThrowIfCancellationRequested();
                await historyLock.WaitAsync(operation.Cancellation).ConfigureAwait(false);
                try { await AppendAsync(operation.Change, operation.Cancellation).ConfigureAwait(false); }
                finally { historyLock.Release(); }
                operation.Done.TrySetResult();
            }
            catch (OperationCanceledException) { operation.Done.TrySetCanceled(operation.Cancellation); }
            catch (Exception ex) { operation.Done.TrySetException(ex); }
        }
    }

    private async Task AppendAsync(LoggedChange change, CancellationToken ct)
    {
        byte[] plain = JsonSerializer.SerializeToUtf8Bytes(change);
        byte[] cipher;
        try
        {
            if (plain.Length > MaxRecordBytes) throw new HistoryCapacityException("El mensaje supera el límite de 1 MiB del registro; no se descartó historial existente.");
            cipher = FeatureProtection.Protect(plain);
        }
        finally { CryptographicOperations.ZeroMemory(plain); }
        string key = ChannelKey(change.Snapshot.ChannelId);
        var files = new DirectoryInfo(historyDirectory).EnumerateFiles("*.ndj").Take(MaxJournalFiles + 1).ToArray();
        if (files.Length > MaxJournalFiles) throw new HistoryCapacityException("El historial excede el límite de 4096 segmentos; se conserva sin añadir nuevas entradas.");
        var channelFiles = files.Where(x => x.Name.StartsWith(key + "-", StringComparison.Ordinal)).OrderBy(x => x.Name, StringComparer.Ordinal).ToArray();
        long bytes = cipher.Length + sizeof(int);
        if (files.Sum(x => x.Length) + bytes > DefaultTotalHistoryLimit || channelFiles.Sum(x => x.Length) + bytes > DefaultChannelHistoryLimit)
            throw new HistoryCapacityException("El historial cifrado está lleno (64 MiB por canal / 256 MiB total). Se conservó el historial existente; archívalo o amplía su política antes de continuar registrando.");
        string path;
        var last = channelFiles.LastOrDefault();
        if (last is null || last.Length + bytes > SegmentLimit)
        {
            if (files.Length >= MaxJournalFiles) throw new HistoryCapacityException("El historial alcanzó el límite de 4096 segmentos; se conserva sin añadir nuevas entradas.");
            int number = last is null ? 0 : int.Parse(Path.GetFileNameWithoutExtension(last.Name)[(key.Length + 1)..], System.Globalization.CultureInfo.InvariantCulture) + 1;
            path = Path.Combine(historyDirectory, $"{key}-{number:D8}.ndj");
        }
        else path = last.FullName;
        ct.ThrowIfCancellationRequested();
        await using var stream = new FileStream(path, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.Read, 8192, FileOptions.Asynchronous);
        // A killed process can leave only its final frame incomplete. Repair that tail before append.
        if (!validatedJournalLengths.TryGetValue(path, out long validLength) || validLength != stream.Length)
            ValidateJournalTail(stream);
        stream.Seek(0, SeekOrigin.End);
        long oldLength = stream.Position;
        try
        {
            // Once an append starts, finish or roll back the complete frame, even if caller cancels.
            await stream.WriteAsync(BitConverter.GetBytes(cipher.Length), CancellationToken.None).ConfigureAwait(false);
            await stream.WriteAsync(cipher, CancellationToken.None).ConfigureAwait(false);
            await stream.FlushAsync(CancellationToken.None).ConfigureAwait(false);
            stream.Flush(flushToDisk: true);
            if (!validatedJournalLengths.ContainsKey(path) && validatedJournalLengths.Count >= 128)
                validatedJournalLengths.Remove(validatedJournalLengths.Keys.First());
            validatedJournalLengths[path] = stream.Length;
        }
        catch { stream.SetLength(oldLength); throw; }
    }

    private static void ValidateJournalTail(FileStream stream)
    {
        stream.Position = 0;
        Span<byte> lengthBytes = stackalloc byte[sizeof(int)];
        while (stream.Position < stream.Length)
        {
            long start = stream.Position;
            if (stream.Length - start < sizeof(int)) { stream.SetLength(start); return; }
            stream.ReadExactly(lengthBytes);
            int size = BitConverter.ToInt32(lengthBytes);
            if (size <= 0 || size > MaxRecordBytes + 65536) throw new InvalidDataException("Registro de historial dañado; se conserva para recuperación.");
            if (stream.Length - stream.Position < size) { stream.SetLength(start); return; }
            stream.Seek(size, SeekOrigin.Current);
        }
    }

    public async Task<IReadOnlyList<ChatMessage>> ReadHistoryAsync(string channelId, int limit = 100, CancellationToken ct = default)
    {
        ObjectDisposedException.ThrowIf(Volatile.Read(ref disposed) != 0, this);
        if (limit is < 1 or > 1000) throw new ArgumentOutOfRangeException(nameof(limit), "El límite debe estar entre 1 y 1000.");
        string key = ChannelKey(channelId);
        await historyLock.WaitAsync(ct).ConfigureAwait(false);
        try
        {
            // Scan frame offsets, not JSON contents. Decrypt only the requested tail.
            var positions = new Queue<(string Path, long Offset, int Size)>();
            foreach (string path in Directory.EnumerateFiles(historyDirectory, key + "-*.ndj").OrderBy(x => x, StringComparer.Ordinal))
            {
                using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
                byte[] length = new byte[sizeof(int)];
                while (stream.Position < stream.Length)
                {
                    ct.ThrowIfCancellationRequested();
                    if (stream.Length - stream.Position < sizeof(int)) break;
                    stream.ReadExactly(length);
                    int size = BitConverter.ToInt32(length);
                    if (size <= 0 || size > MaxRecordBytes + 65536) throw new InvalidDataException("Registro de historial dañado; se conserva para recuperación.");
                    if (stream.Length - stream.Position < size) break; // Incomplete final frame after interruption.
                    positions.Enqueue((path, stream.Position, size));
                    if (positions.Count > limit) positions.Dequeue();
                    stream.Seek(size, SeekOrigin.Current);
                }
            }
            var result = new List<ChatMessage>(positions.Count);
            foreach (var position in positions)
            {
                ct.ThrowIfCancellationRequested();
                using var stream = new FileStream(position.Path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
                stream.Position = position.Offset;
                byte[] cipher = new byte[position.Size];
                await stream.ReadExactlyAsync(cipher, ct).ConfigureAwait(false);
                byte[] plain = FeatureProtection.Unprotect(cipher);
                try
                {
                    var change = JsonSerializer.Deserialize<LoggedChange>(plain) ?? throw new InvalidDataException("Entrada de historial vacía.");
                    if (change.Snapshot.ChannelId != channelId) throw new InvalidDataException("La entrada no pertenece al canal esperado.");
                    result.Add(change.Snapshot);
                }
                finally { CryptographicOperations.ZeroMemory(plain); }
            }
            return new ReadOnlyCollection<ChatMessage>(result);
        }
        finally { historyLock.Release(); }
    }

    private static string ChannelKey(string id)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(id);
        return Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(id))).ToLowerInvariant();
    }

    internal static void AtomicWrite(string path, byte[] bytes)
    {
        string temp = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            using (var stream = new FileStream(temp, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            { stream.Write(bytes); stream.Flush(flushToDisk: true); }
            File.Move(temp, path, overwrite: true);
        }
        finally { if (File.Exists(temp)) File.Delete(temp); }
    }

    public async ValueTask DisposeAsync()
    {
        if (Interlocked.Exchange(ref disposed, 1) == 0) writes.Writer.TryComplete();
        await writer.ConfigureAwait(false);
    }
    public void Dispose() => DisposeAsync().AsTask().GetAwaiter().GetResult();
    private sealed record LoggedChange(DateTimeOffset ChangedAt, ChatMessage Snapshot);
    private sealed record JournalWrite(LoggedChange Change, CancellationToken Cancellation)
    {
        public TaskCompletionSource Done { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    }
}

public sealed class HistoryCapacityException(string message) : IOException(message);

/// <summary>Encrypts with the current Windows user's DPAPI keys; no credential store is read.</summary>
public static class FeatureProtection
{
    private const uint UiForbidden = 1;
    private static readonly byte[] Entropy = Encoding.UTF8.GetBytes("NotDiscord.Terminal.Features.v1");
    public static byte[] Protect(byte[] plain) => Transform(plain, true);
    public static byte[] Unprotect(byte[] cipher) => Transform(cipher, false);
    private static byte[] Transform(byte[] data, bool protect)
    {
        if (!OperatingSystem.IsWindows()) throw new PlatformNotSupportedException("El almacén de NotDiscord requiere Windows DPAPI.");
        DataBlob input = Allocate(data), entropy = Allocate(Entropy), output = default;
        IntPtr description = IntPtr.Zero;
        try
        {
            bool ok = protect
                ? CryptProtectData(ref input, "NotDiscord feature data", ref entropy, IntPtr.Zero, IntPtr.Zero, UiForbidden, out output)
                : CryptUnprotectData(ref input, out description, ref entropy, IntPtr.Zero, IntPtr.Zero, UiForbidden, out output);
            if (!ok) throw new CryptographicException(Marshal.GetLastWin32Error());
            var result = new byte[output.Length];
            Marshal.Copy(output.Data, result, 0, result.Length);
            return result;
        }
        finally
        {
            Wipe(input); Wipe(entropy);
            if (output.Data != IntPtr.Zero)
            {
                unsafe { CryptographicOperations.ZeroMemory(new Span<byte>(output.Data.ToPointer(), output.Length)); }
                LocalFree(output.Data);
            }
            if (description != IntPtr.Zero) LocalFree(description);
        }
    }
    private static DataBlob Allocate(byte[] bytes)
    {
        IntPtr data = Marshal.AllocHGlobal(Math.Max(1, bytes.Length));
        Marshal.Copy(bytes, 0, data, bytes.Length);
        return new DataBlob { Length = bytes.Length, Data = data };
    }
    private static void Wipe(DataBlob blob)
    {
        if (blob.Data == IntPtr.Zero) return;
        unsafe { CryptographicOperations.ZeroMemory(new Span<byte>(blob.Data.ToPointer(), blob.Length)); }
        Marshal.FreeHGlobal(blob.Data);
    }
    [StructLayout(LayoutKind.Sequential)] private struct DataBlob { public int Length; public IntPtr Data; }
    [DllImport("crypt32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)] private static extern bool CryptProtectData(ref DataBlob input, string description, ref DataBlob entropy, IntPtr reserved, IntPtr prompt, uint flags, out DataBlob output);
    [DllImport("crypt32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)] private static extern bool CryptUnprotectData(ref DataBlob input, out IntPtr description, ref DataBlob entropy, IntPtr reserved, IntPtr prompt, uint flags, out DataBlob output);
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr memory);
}

/// <summary>LRU channel windows; bounds cover all cached channels, not each window independently.</summary>
public sealed class MessageWindowCache
{
    private readonly int maxChannels, maxMessages;
    private readonly long maxTextBytes;
    private readonly object gate = new();
    private readonly Dictionary<string, Window> windows = new(StringComparer.Ordinal);
    private long clock;
    public MessageWindowCache(int maxChannels = 3, int maxMessages = 200, long maxTextBytes = 4 * 1024 * 1024)
    {
        if (maxChannels < 1 || maxMessages < 1 || maxTextBytes < 1) throw new ArgumentOutOfRangeException(nameof(maxMessages));
        this.maxChannels = maxChannels; this.maxMessages = maxMessages; this.maxTextBytes = maxTextBytes;
    }
    public int Count { get { lock (gate) return windows.Values.Sum(x => x.Messages.Count); } }
    public int ChannelCount { get { lock (gate) return windows.Count; } }
    public bool ContainsChannel(string channelId) { lock (gate) return windows.ContainsKey(channelId); }
    public long EstimatedTextBytes { get { lock (gate) return windows.Values.Sum(x => x.Bytes); } }

    public void Replace(string channelId, IEnumerable<ChatMessage> messages)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(channelId); ArgumentNullException.ThrowIfNull(messages);
        lock (gate)
        {
            // Build through the same bounded insertion path, never materialize an unlimited enumerable.
            windows.Remove(channelId);
            foreach (ChatMessage message in messages)
            {
                if (message.ChannelId != channelId) throw new ArgumentException("El mensaje pertenece a otro canal.", nameof(messages));
                UpsertLocked(message);
            }
        }
    }
    public void Upsert(ChatMessage message) { ArgumentNullException.ThrowIfNull(message); lock (gate) UpsertLocked(message); }
    private void UpsertLocked(ChatMessage message)
    {
        long cost = Estimate(message);
        if (cost > maxTextBytes)
        {
            // Oversized entries are fetched on demand; never truncate message content in the cache.
            Remove(message.ChannelId, message.Id); return;
        }
        if (!windows.TryGetValue(message.ChannelId, out Window? window)) windows.Add(message.ChannelId, window = new());
        window.Touch = ++clock;
        int existing = window.Messages.FindIndex(x => x.Id == message.Id);
        if (existing >= 0) { window.Bytes -= Estimate(window.Messages[existing]); window.Messages.RemoveAt(existing); }
        var copy = message with { Attachments = Array.AsReadOnly(message.Attachments.ToArray()),
            Mentions = message.Mentions == null ? null : Array.AsReadOnly(message.Mentions.ToArray()),
            ReplyPreview = message.ReplyPreview == null ? null : message.ReplyPreview with { Mentions = message.ReplyPreview.Mentions == null ? null : Array.AsReadOnly(message.ReplyPreview.Mentions.ToArray()) } };
        int insert = window.Messages.BinarySearch(copy, MessageOrder.Instance);
        window.Messages.Insert(insert < 0 ? ~insert : insert, copy);
        window.Bytes += cost;
        while (windows.Count > maxChannels) windows.Remove(windows.MinBy(x => x.Value.Touch).Key);
        while (windows.Values.Sum(x => x.Messages.Count) > maxMessages || windows.Values.Sum(x => x.Bytes) > maxTextBytes)
        {
            // Evict oldest inactive channel first; retain the newest messages in the active window.
            var oldest = windows.MinBy(x => x.Value.Touch);
            if (windows.Count > 1 && oldest.Key != message.ChannelId) { windows.Remove(oldest.Key); continue; }
            var entry = oldest.Value;
            if (entry.Messages.Count == 0) { windows.Remove(oldest.Key); continue; }
            entry.Bytes -= Estimate(entry.Messages[0]); entry.Messages.RemoveAt(0);
            if (entry.Messages.Count == 0) windows.Remove(oldest.Key);
        }
    }
    public bool Remove(string channelId, string messageId)
    {
        lock (gate)
        {
            if (!windows.TryGetValue(channelId, out Window? window)) return false;
            int index = window.Messages.FindIndex(x => x.Id == messageId);
            if (index < 0) return false;
            window.Bytes -= Estimate(window.Messages[index]); window.Messages.RemoveAt(index);
            if (window.Messages.Count == 0) windows.Remove(channelId);
            return true;
        }
    }
    public IReadOnlyList<ChatMessage> Read(string channelId)
    {
        lock (gate)
        {
            if (!windows.TryGetValue(channelId, out Window? window)) return Array.Empty<ChatMessage>();
            window.Touch = ++clock; return Array.AsReadOnly(window.Messages.ToArray());
        }
    }
    public ChatMessage? Get(string channelId, string messageId)
    {
        lock (gate)
        {
            if (!windows.TryGetValue(channelId, out Window? window)) return null;
            window.Touch = ++clock; return window.Messages.FirstOrDefault(x => x.Id == messageId);
        }
    }
    public void Clear() { lock (gate) windows.Clear(); }
    private static long Estimate(ChatMessage m)
    {
        long chars = (long)m.Id.Length + m.ChannelId.Length + m.Author.Id.Length + m.Author.Name.Length + (m.Author.AvatarUrl?.Length ?? 0) + m.Content.Length + (m.ReplyTo?.Length ?? 0);
        foreach (var a in m.Attachments) chars += (long)a.Id.Length + a.Name.Length + a.Url.Length + (a.ContentType?.Length ?? 0);
        static long PersonChars(Person person) => (long)person.Id.Length + person.Name.Length + person.Username.Length + person.DisplayName.Length + (person.AvatarUrl?.Length ?? 0);
        if (m.Mentions != null) foreach (var person in m.Mentions) chars += PersonChars(person);
        if (m.ReplyPreview is { } reply)
        {
            chars += (long)reply.MessageId.Length + reply.ChannelId.Length + reply.Content.Length;
            if (reply.Author != null) chars += PersonChars(reply.Author);
            if (reply.Mentions != null) foreach (var person in reply.Mentions) chars += PersonChars(person);
        }
        return chars * sizeof(char) + 256L + m.Attachments.Count * 96L + (m.Mentions?.Count ?? 0) * 96L +
            (m.ReplyPreview == null ? 0 : 128L + (m.ReplyPreview.Mentions?.Count ?? 0) * 96L);
    }
    private sealed class Window { public long Touch; public long Bytes; public List<ChatMessage> Messages { get; } = new(); }
    private sealed class MessageOrder : IComparer<ChatMessage>
    {
        public static readonly MessageOrder Instance = new();
        public int Compare(ChatMessage? x, ChatMessage? y)
        {
            int comparison = Nullable.Compare(x?.Timestamp, y?.Timestamp);
            return comparison != 0 ? comparison : string.Compare(x?.Id, y?.Id, StringComparison.Ordinal);
        }
    }
}
