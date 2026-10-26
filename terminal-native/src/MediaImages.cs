using System.IO;
using System.Net;
using System.Net.Http;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace NotDiscord.Terminal;

/// <summary>Small decoded-thumbnail cache for Discord and supported GIF providers; no persistent media.</summary>
public static class MediaImages
{
    private sealed record Entry(BitmapSource Image, long Bytes);
    private static readonly object Gate = new();
    private static readonly Dictionary<string, Entry> Cache = new();
    private static readonly LinkedList<string> Order = new();
    private static readonly SemaphoreSlim Slots = new(3);
    private static readonly HttpClient Client = new(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false }) { Timeout = TimeSpan.FromSeconds(15) };
    private const long Budget = 8 * 1024 * 1024;
    private const int MaxDownload = 5 * 1024 * 1024;
    private const int MaxGifDownload = 12 * 1024 * 1024;
    private static long used;

    public static bool IsAllowed(string? url)
        => GifSources.SafeUri(url, out var u) && (u.Host is "cdn.discordapp.com" or "media.discordapp.net" || GifSources.IsMediaHost(u.Host)
            || u.Host == "i.scdn.co" && u.AbsolutePath.StartsWith("/image/", StringComparison.Ordinal));

    public static bool IsGifSource(string? url) => GifSources.IsDirectGif(url) || GifSources.IsPage(url);

    public static string StaticThumbnailUrl(string url)
    {
        if (!GifSources.SafeUri(url, out var uri)) return url;
        if (uri.Host == "media.discordapp.net")
        {
            var builder = new UriBuilder(uri);
            var parts = uri.Query.TrimStart('?').Split('&', StringSplitOptions.RemoveEmptyEntries)
                .Where(p => !new[] { "format", "animated" }.Contains(Uri.UnescapeDataString(p.Split('=', 2)[0]), StringComparer.OrdinalIgnoreCase));
            builder.Query = string.Join('&', parts.Concat(new[] { "format=png", "animated=false" })); return builder.Uri.AbsoluteUri;
        }
        if (GifSources.IsMediaHost(uri.Host) && uri.Host.EndsWith(".giphy.com", StringComparison.OrdinalIgnoreCase)
            && uri.AbsolutePath.EndsWith(".webp", StringComparison.OrdinalIgnoreCase))
        { var builder = new UriBuilder(uri) { Path = uri.AbsolutePath[..^5] + ".gif" }; return builder.Uri.AbsoluteUri; }
        return url;
    }

    internal static async Task<byte[]?> GetGifBytesAsync(string url, CancellationToken cancellation)
    {
        string? resolved = await GifSources.ResolveAsync(url, cancellation).ConfigureAwait(false);
        if (resolved != null) url = resolved;
        if (!IsAllowed(url)) return null;
        await Slots.WaitAsync(cancellation).ConfigureAwait(false);
        try
        {
            using var response = await Client.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, cancellation).ConfigureAwait(false);
            if (response.StatusCode != HttpStatusCode.OK || response.Content.Headers.ContentLength > MaxGifDownload) return null;
            using var stream = await response.Content.ReadAsStreamAsync(cancellation).ConfigureAwait(false);
            using var bytes = new MemoryStream();
            byte[] buffer = new byte[16384];
            while (true)
            {
                int read = await stream.ReadAsync(buffer.AsMemory(), cancellation).ConfigureAwait(false);
                if (read == 0) break;
                if (bytes.Length + read > MaxGifDownload) return null;
                bytes.Write(buffer, 0, read);
            }
            byte[] data = bytes.ToArray();
            return data.Length >= 6 && data[0] == 'G' && data[1] == 'I' && data[2] == 'F' ? data : null;
        }
        finally { Slots.Release(); }
    }

    public static async Task<BitmapSource?> GetAsync(string? url, int width, CancellationToken cancellation = default)
    {
        if (GifSources.IsPage(url))
        {
            try { url = await GifSources.ResolveAsync(url!, cancellation).ConfigureAwait(false); }
            catch (OperationCanceledException) { return null; }
        }
        if (!IsAllowed(url)) return null;
        url = StaticThumbnailUrl(url!);
        width = Math.Clamp(width, 24, 1600);
        string key = width + "|" + url;
        lock (Gate)
        {
            if (Cache.TryGetValue(key, out var hit)) { Order.Remove(key); Order.AddLast(key); return hit.Image; }
        }
        await Slots.WaitAsync(cancellation).ConfigureAwait(false);
        try
        {
            lock (Gate) if (Cache.TryGetValue(key, out var hit)) return hit.Image;
            using var response = await Client.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, cancellation).ConfigureAwait(false);
            if (response.StatusCode != HttpStatusCode.OK || response.Content.Headers.ContentLength > MaxDownload) return null;
            var kind = response.Content.Headers.ContentType?.MediaType ?? "";
            if (!kind.StartsWith("image/", StringComparison.OrdinalIgnoreCase) || kind.Contains("svg", StringComparison.OrdinalIgnoreCase)) return null;
            using var source = await response.Content.ReadAsStreamAsync(cancellation).ConfigureAwait(false);
            using var bytes = new MemoryStream();
            byte[] buffer = new byte[16384];
            while (true)
            {
                int read = await source.ReadAsync(buffer.AsMemory(), cancellation).ConfigureAwait(false);
                if (read == 0) break;
                if (bytes.Length + read > MaxDownload) return null;
                bytes.Write(buffer, 0, read);
            }
            bytes.Position = 0;
            // Read dimensions before decoding; pathological images never reach the pixel allocation.
            var header = BitmapDecoder.Create(bytes, BitmapCreateOptions.DelayCreation | BitmapCreateOptions.IgnoreColorProfile, BitmapCacheOption.None);
            if (header.Frames.Count == 0 || header.Frames[0].PixelWidth > 8192 || header.Frames[0].PixelHeight > 8192
                || (long)header.Frames[0].PixelWidth * header.Frames[0].PixelHeight > 24_000_000) return null;
            int originalWidth = header.Frames[0].PixelWidth, originalHeight = header.Frames[0].PixelHeight;
            if (originalWidth < 1 || originalHeight < 1) return null;
            double scale = Math.Min(1d, Math.Min(width / (double)originalWidth, 1600d / originalHeight));
            bytes.Position = 0;
            var image = new BitmapImage();
            image.BeginInit();
            image.CacheOption = BitmapCacheOption.OnLoad;
            image.CreateOptions = BitmapCreateOptions.IgnoreColorProfile;
            image.DecodePixelWidth = Math.Max(1, (int)(originalWidth * scale));
            image.StreamSource = bytes;
            image.EndInit();
            image.Freeze();
            long weight = (long)image.PixelWidth * image.PixelHeight * 4;
            if (weight <= Budget / 2)
            {
                lock (Gate)
                {
                    if (!Cache.ContainsKey(key))
                    {
                        while (used + weight > Budget && Order.First != null)
                        {
                            string old = Order.First.Value; Order.RemoveFirst();
                            used -= Cache[old].Bytes; Cache.Remove(old);
                        }
                        Cache[key] = new(image, weight); Order.AddLast(key); used += weight;
                    }
                }
            }
            return image;
        }
        catch (Exception ex) when (ex is HttpRequestException or IOException or NotSupportedException or System.IO.FileFormatException or ArgumentException or InvalidOperationException or OperationCanceledException)
        { return null; }
        finally { Slots.Release(); }
    }
}

/// <summary>Downloads only when the corresponding virtualized visual is loaded.</summary>
public sealed class UiMediaImage : Image
{
    public event Action<bool>? ImageLoaded;
    private readonly string? url;
    private readonly int decodeWidth;
    private CancellationTokenSource? pending;
    public UiMediaImage(string? url, int decodeWidth)
    {
        this.url = url; this.decodeWidth = decodeWidth;
        Stretch = Stretch.UniformToFill;
        Loaded += Load;
        Unloaded += (_, _) => { pending?.Cancel(); pending?.Dispose(); pending = null; Source = null; };
    }
    private async void Load(object sender, RoutedEventArgs args)
    {
        if (pending != null || Source != null) return;
        var request = pending = new CancellationTokenSource();
        try
        {
            var value = await MediaImages.GetAsync(url, decodeWidth, request.Token);
            if (ReferenceEquals(pending, request) && !request.IsCancellationRequested && IsLoaded)
            { Source = value; ImageLoaded?.Invoke(value != null); }
        }
        catch (OperationCanceledException) { }
    }
}
