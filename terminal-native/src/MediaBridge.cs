using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;

namespace NotDiscord.Terminal;

public sealed class MediaBridge : IAsyncDisposable
{
    private readonly Process process;
    private readonly NamedPipeServerStream pipe;
    private readonly StreamReader pipeInput;
    private readonly StreamWriter pipeOutput;
    private readonly Task connected;
    private readonly ConcurrentDictionary<string, TaskCompletionSource<JsonElement>> waiting = new();
    private readonly SemaphoreSlim writer = new(1, 1);
    private readonly CancellationTokenSource lifetime = new();
    private readonly Task reader;
    private long sequence;
    private bool disposed;
    public event Action<string, JsonElement>? Event;
    public int ProcessId => process.Id;
    public JsonElement? LastFailure { get; private set; }
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetNamedPipeClientProcessId(SafePipeHandle handle, out uint processId);
    public static string MediaHostPath(string root) => Path.Combine(root, "media-host", "runtime", "NotDiscord.Media.exe");
    public static bool IsAvailable(string root) => File.Exists(MediaHostPath(root)) && File.Exists(Path.Combine(root, "media-helper", "main.cjs"));

    public MediaBridge(string root)
    {
        if (!IsAvailable(root)) throw new ServiceException("No se encontró el servicio multimedia nativo.");
        string pipeName = "NotDiscord.Media." + Guid.NewGuid().ToString("N");
        pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly, 16384, 16384);
        pipeInput = new StreamReader(pipe, new UTF8Encoding(false, true), false, 2048, leaveOpen: true);
        pipeOutput = new StreamWriter(pipe, new UTF8Encoding(false), 2048, leaveOpen: true);
        var start = new ProcessStartInfo(MediaHostPath(root)) { UseShellExecute = false, CreateNoWindow = true, RedirectStandardOutput = true, RedirectStandardError = true, WorkingDirectory = root };
        // The addon uses V8 callbacks that require the Electron main-thread scope.
        // Run only the minimal owned host; ordinary Node aborts during MLS key creation.
        start.Environment.Remove("ELECTRON_RUN_AS_NODE");
        start.Environment.Remove("NODE_OPTIONS");
        start.Environment["NOTDISCORD_MEDIA_PIPE"] = @"\\.\pipe\" + pipeName;
        try { process = Process.Start(start) ?? throw new ServiceException("No se pudo iniciar el servicio multimedia."); }
        catch { pipeInput.Dispose(); pipeOutput.Dispose(); pipe.Dispose(); throw; }
        connected = ConnectPipeAsync();
        // Native diagnostic output is never a protocol channel or a credential log.
        _ = Task.Run(() => DiscardAsync(process.StandardError));
        _ = Task.Run(() => DiscardAsync(process.StandardOutput));
        reader = Task.Run(ReadAsync);
    }

    private async Task DiscardAsync(TextReader source)
    {
        var buffer = new char[2048];
        try { while (await source.ReadAsync(buffer, lifetime.Token) > 0) { } }
        catch (Exception e) when (e is IOException or OperationCanceledException or InvalidOperationException) { }
    }

    private async Task ConnectPipeAsync()
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(lifetime.Token);
        timeout.CancelAfter(TimeSpan.FromSeconds(10));
        await pipe.WaitForConnectionAsync(timeout.Token);
        if (!GetNamedPipeClientProcessId(pipe.SafePipeHandle, out uint clientId) || clientId != process.Id)
            throw new ServiceException("No se pudo verificar la identidad del servicio multimedia.");
    }

    private async Task ReadAsync()
    {
        string failureCode = "MEDIA_EOF";
        try
        {
            await connected;
            var line = new System.Text.StringBuilder(); var buffer = new char[2048];
            int n;
            while ((n = await pipeInput.ReadAsync(buffer, lifetime.Token)) > 0)
            {
                for (int i = 0; i < n; i++)
                {
                    char c = buffer[i];
                    if (c == '\n') { if (line.Length > 0) ProcessFrame(line.ToString()); line.Clear(); }
                    else if (c != '\r') { if (line.Length >= 262144) throw new IOException("Media IPC frame too large"); line.Append(c); }
                }
            }
        }
        catch (JsonException) { failureCode = "MEDIA_IPC_INVALID_JSON"; }
        catch (IOException) { failureCode = "MEDIA_IPC_IO"; }
        catch (OperationCanceledException) { failureCode = "MEDIA_IPC_CANCELLED"; }
        catch (InvalidOperationException) { failureCode = "MEDIA_IPC_INVALID_STATE"; }
        catch (ServiceException) { failureCode = "MEDIA_PIPE_IDENTITY"; }
        catch (DecoderFallbackException) { failureCode = "MEDIA_IPC_INVALID_UTF8"; }
        finally
        {
            if (!disposed)
            {
                bool exited = process.HasExited;
                int? exitCode = exited ? process.ExitCode : null;
                LastFailure = JsonSerializer.SerializeToElement(new { reasonCode = failureCode, processExited = exited, exitCode });
            }
            foreach (var pending in waiting.Values) pending.TrySetException(new ServiceException("El servicio multimedia se detuvo."));
            waiting.Clear();
            if (!disposed && LastFailure is JsonElement failure) Event?.Invoke("fatal", failure);
        }
    }

    private void ProcessFrame(string line)
    {
        using var document = JsonDocument.Parse(line);
        var root = document.RootElement;
        if (root.TryGetProperty("id", out var id) && id.ValueKind == JsonValueKind.String && waiting.TryRemove(id.GetString()!, out var completion))
        {
            if (root.TryGetProperty("ok", out var ok) && ok.ValueKind == JsonValueKind.True) completion.TrySetResult(root.GetProperty("result").Clone());
            else completion.TrySetException(new ServiceException("El motor multimedia rechazó la operación: " + (root.TryGetProperty("error", out var error) ? Controller.Str(error, "message") : "error interno")));
        }
        else if (root.TryGetProperty("event", out var type)) Event?.Invoke(type.GetString() ?? "", root.Clone());
    }

    public async Task<JsonElement> CommandAsync(string command, object? parameters = null, CancellationToken ct = default)
    {
        if (disposed || process.HasExited) throw new ServiceException("El servicio multimedia no está activo.");
        if (waiting.Count >= 16) throw new ServiceException("El servicio multimedia está ocupado.");
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(ct, lifetime.Token);
        timeout.CancelAfter(TimeSpan.FromSeconds(35));
        await connected.WaitAsync(timeout.Token);
        string id = Interlocked.Increment(ref sequence).ToString();
        var pending = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        waiting[id] = pending;
        using var registration = timeout.Token.Register(() => { if (waiting.TryRemove(id, out var p)) p.TrySetCanceled(timeout.Token); });
        await writer.WaitAsync(timeout.Token);
        try { await pipeOutput.WriteLineAsync(JsonSerializer.Serialize(new { id, command, @params = parameters ?? new { } }).AsMemory(), timeout.Token); await pipeOutput.FlushAsync(timeout.Token); }
        catch { waiting.TryRemove(id, out _); throw; }
        finally { writer.Release(); }
        return await pending.Task;
    }

    public async ValueTask DisposeAsync()
    {
        if (disposed) return;
        try { using var close = new CancellationTokenSource(2500); await CommandAsync("shutdown", ct: close.Token); } catch (Exception) { }
        disposed = true;
        try { pipe.Close(); } catch (Exception) { }
        try { using var close = new CancellationTokenSource(3000); await process.WaitForExitAsync(close.Token); } catch (OperationCanceledException) { if (!process.HasExited) process.Kill(entireProcessTree: true); }
        lifetime.Cancel();
        try { await reader; } catch (Exception) { }
        pipeInput.Dispose();
        try { pipeOutput.Dispose(); } catch (Exception error) when (error is IOException or ObjectDisposedException) { }
        pipe.Dispose();
        process.Dispose(); lifetime.Dispose();
    }
}
