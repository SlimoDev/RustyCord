using System.Net;
using System.Net.Http;

namespace NotDiscord.Terminal;

/// <summary>Explicit media saving. Streams to disk with a fixed buffer, never to the thumbnail cache.</summary>
public static class MediaDownloads
{
    private static readonly HttpClient Client = new(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false }) { Timeout = TimeSpan.FromSeconds(60) };
    private static readonly SemaphoreSlim Slots = new(2);
    public static async Task SaveAsync(string source, string destination, CancellationToken cancellation = default)
    {
        if (GifSources.IsPage(source)) source = await GifSources.ResolveAsync(source, cancellation) ?? throw new IOException("No se pudo encontrar el archivo de ese GIF.");
        if (!MediaImages.IsAllowed(source)) throw new IOException("No se permite descargar este origen de imagen.");
        string temporary = destination + "." + Guid.NewGuid().ToString("N") + ".download";
        await Slots.WaitAsync(cancellation).ConfigureAwait(false);
        try
        {
            using var response = await Client.GetAsync(source, HttpCompletionOption.ResponseHeadersRead, cancellation).ConfigureAwait(false);
            string kind = response.Content.Headers.ContentType?.MediaType ?? "";
            if (response.StatusCode != HttpStatusCode.OK || !kind.StartsWith("image/", StringComparison.OrdinalIgnoreCase) || kind.Contains("svg", StringComparison.OrdinalIgnoreCase)) throw new IOException("El servidor no devolvió una imagen o un GIF compatible.");
            const long maximum = 25 * 1024 * 1024;
            if (response.Content.Headers.ContentLength > maximum) throw new IOException("La imagen supera el límite local de 25 MiB para guardar.");
            using (var input = await response.Content.ReadAsStreamAsync(cancellation).ConfigureAwait(false))
            using (var output = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None, 32768, useAsync: true))
            {
                var buffer = new byte[32768]; long total = 0; int count;
                while ((count = await input.ReadAsync(buffer.AsMemory(), cancellation).ConfigureAwait(false)) > 0)
                { total += count; if (total > maximum) throw new IOException("La imagen supera el límite local de 25 MiB para guardar."); await output.WriteAsync(buffer.AsMemory(0, count), cancellation).ConfigureAwait(false); }
            }
            File.Move(temporary, destination, overwrite: true);
        }
        finally { Slots.Release(); try { File.Delete(temporary); } catch (IOException) { } catch (UnauthorizedAccessException) { } }
    }
}
