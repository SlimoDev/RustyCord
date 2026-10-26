using System.Net;
using System.IO;
using System.Net.Http;
using System.Text;
using System.Text.RegularExpressions;

namespace NotDiscord.Terminal;

/// <summary>Resolves known GIF share pages without scripts, cookies or an embedded browser.</summary>
public static class GifSources
{
    private sealed record Cached(string? Url, DateTimeOffset Expires);
    private static readonly Dictionary<string, Cached> Cache = new(StringComparer.Ordinal);
    private static readonly SemaphoreSlim Slots = new(2);
    private static readonly HttpClient Client = new(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false, AutomaticDecompression = DecompressionMethods.GZip | DecompressionMethods.Deflate }) { Timeout = TimeSpan.FromSeconds(12) };
    private static readonly TimeSpan MatchTimeout = TimeSpan.FromMilliseconds(100);
    private const int MaxHtmlBytes = 512 * 1024;

    internal static bool SafeUri(string? value, out Uri uri)
    {
        uri = null!;
        return value is { Length: > 0 and <= 4096 } && Uri.TryCreate(value, UriKind.Absolute, out uri!)
            && uri.Scheme == "https" && uri.IsDefaultPort && string.IsNullOrEmpty(uri.UserInfo);
    }
    public static bool IsMediaHost(string host) => host is "static.klipy.com" or "static1.klipy.com" or "static2.klipy.com" or "media.tenor.co" or "c.tenor.com"
        || Regex.IsMatch(host, @"^media\d{0,2}\.tenor\.com$|^(?:media\d{0,2}|i)\.giphy\.com$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant, MatchTimeout);
    public static bool IsPage(string? url)
    {
        if (!SafeUri(url, out var uri)) return false;
        return ((uri.Host is "tenor.com" or "www.tenor.com") && uri.AbsolutePath.StartsWith("/view/", StringComparison.Ordinal))
            || ((uri.Host is "giphy.com" or "www.giphy.com" or "klipy.com" or "www.klipy.com") && uri.AbsolutePath.StartsWith("/gifs/", StringComparison.Ordinal));
    }
    public static bool IsDirectGif(string? url)
    {
        if (!SafeUri(url, out var uri) || !(IsMediaHost(uri.Host) || uri.Host is "cdn.discordapp.com" or "media.discordapp.net")
            || !uri.AbsolutePath.EndsWith(".gif", StringComparison.OrdinalIgnoreCase)) return false;
        foreach (string part in uri.Query.TrimStart('?').Split('&', StringSplitOptions.RemoveEmptyEntries))
        {
            string[] pair = part.Split('=', 2);
            if (Uri.UnescapeDataString(pair[0]).Equals("format", StringComparison.OrdinalIgnoreCase)
                && (pair.Length != 2 || !Uri.UnescapeDataString(pair[1]).Equals("gif", StringComparison.OrdinalIgnoreCase))) return false;
        }
        return true;
    }
    // The authenticated Discord GIF catalogue supplies both canonical share URL
    // and gif_src. Reuse that observed pair; never guess hashes or run page scripts.
    public static void RememberKnownSource(string page, string directGif)
    {
        if (!IsPage(page) || !IsDirectGif(directGif)) return;
        lock (Cache)
        {
            if (!Cache.ContainsKey(page) && Cache.Count >= 128) Cache.Remove(Cache.Keys.First());
            Cache[page] = new(directGif, DateTimeOffset.UtcNow.AddMinutes(15));
        }
    }

    public static async Task<string?> ResolveAsync(string url, CancellationToken ct)
    {
        if (IsDirectGif(url)) return url;
        if (!IsPage(url)) return null;
        lock (Cache) if (Cache.TryGetValue(url, out var entry) && entry.Expires > DateTimeOffset.UtcNow) return entry.Url;
        await Slots.WaitAsync(ct).ConfigureAwait(false);
        try
        {
            lock (Cache) if (Cache.TryGetValue(url, out var entry) && entry.Expires > DateTimeOffset.UtcNow) return entry.Url;
            string page = url;
            string? result = null;
            for (int redirects = 0; redirects <= 2; redirects++)
            {
                using var request = new HttpRequestMessage(HttpMethod.Get, page);
                request.Headers.Accept.ParseAdd("text/html");
                using var response = await Client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, ct).ConfigureAwait(false);
                if ((int)response.StatusCode is 301 or 302 or 303 or 307 or 308)
                {
                    var location = response.Headers.Location;
                    if (location == null) break;
                    var next = location.IsAbsoluteUri ? location : new Uri(new Uri(page), location);
                    if (!IsPage(next.AbsoluteUri)) break;
                    page = next.AbsoluteUri; continue;
                }
                if (response.StatusCode != HttpStatusCode.OK || response.Content.Headers.ContentLength > MaxHtmlBytes
                    || !string.Equals(response.Content.Headers.ContentType?.MediaType, "text/html", StringComparison.OrdinalIgnoreCase)) break;
                using var stream = await response.Content.ReadAsStreamAsync(ct).ConfigureAwait(false);
                using var body = new MemoryStream();
                var bytes = new byte[8192]; int count;
                while ((count = await stream.ReadAsync(bytes.AsMemory(), ct).ConfigureAwait(false)) > 0)
                {
                    if (body.Length + count > MaxHtmlBytes) return null;
                    body.Write(bytes, 0, count);
                }
                result = ExtractGifUrl(Encoding.UTF8.GetString(body.ToArray()));
                break;
            }
            lock (Cache)
            {
                if (Cache.Count >= 128) Cache.Remove(Cache.Keys.First());
                Cache[url] = new(result, DateTimeOffset.UtcNow.Add(result == null ? TimeSpan.FromSeconds(30) : TimeSpan.FromMinutes(15)));
            }
            return result;
        }
        catch (Exception error) when (error is HttpRequestException or IOException or RegexMatchTimeoutException) { return null; }
        finally { Slots.Release(); }
    }

    internal static string? ExtractGifUrl(string html)
    {
        // Read only metadata; never execute the provider's scripts or traverse links.
        foreach (Match tag in Regex.Matches(html, @"<meta\b[^>]{0,4096}>", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant, MatchTimeout))
        {
            var attributes = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            foreach (Match attribute in Regex.Matches(tag.Value, "([a-zA-Z_:][-a-zA-Z0-9_:.]*)\\s*=\\s*([\"'])(.*?)\\2", RegexOptions.CultureInvariant, MatchTimeout))
                attributes[attribute.Groups[1].Value] = WebUtility.HtmlDecode(attribute.Groups[3].Value);
            string name = attributes.GetValueOrDefault("property") ?? attributes.GetValueOrDefault("name") ?? "";
            if (name is not ("og:image" or "og:image:secure_url" or "twitter:image" or "twitter:image:src")) continue;
            string? value = attributes.GetValueOrDefault("content");
            if (IsDirectGif(value) && SafeUri(value, out var uri) && IsMediaHost(uri.Host)) return value;
        }
        return null;
    }
}
