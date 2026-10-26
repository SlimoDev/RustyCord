using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using NotDiscord.Terminal;

var errors = new List<string>(); int assertions = 0;
void Check(bool passed, string name) { assertions++; if (!passed) errors.Add(name); }
string source = "https://tenor.com/view/fortnite-gaming-pubg-royale-yes-gif-11570059";
string media = "https://media1.tenor.com/m/vc-R0YJSOQAAAAAC/fortnite-gaming.gif";
Check(MediaImages.IsGifSource(source), "Tenor share page accepted");
Check(MediaImages.IsGifSource("https://giphy.com/gifs/example-abc"), "Giphy share page accepted");
Check(MediaImages.IsAllowed(media), "Tenor media host accepted");
Check(MediaImages.IsAllowed("https://static.klipy.com/ii/public/cat.gif") && MediaImages.IsAllowed("https://static1.klipy.com/ii/public/cat.gif") && MediaImages.IsAllowed("https://static2.klipy.com/ii/public/cat.gif"), "Current Klipy media hosts accepted");
Check(MediaImages.IsGifSource("https://klipy.com/gifs/cat-example"), "Current Klipy share page accepted");
GifSources.RememberKnownSource("https://klipy.com/gifs/synthetic-map", "https://static.klipy.com/ii/synthetic-map.gif");
Check(await GifSources.ResolveAsync("https://klipy.com/gifs/synthetic-map", CancellationToken.None) == "https://static.klipy.com/ii/synthetic-map.gif", "Observed catalogue gif_src resolves the canonical Klipy page without a network request or script evaluation");
Check(!MediaImages.IsAllowed("https://static.klipy.com.evil.invalid/cat.gif") && !MediaImages.IsAllowed("https://api.klipy.com/v2/search"), "Klipy support never broadens media downloads to arbitrary subdomains or API endpoints");
Check(MediaImages.IsAllowed("https://cdn.discordapp.com/channel-icons/123/abc.png?size=64"), "Group channel icon accepted");
Check(MediaImages.IsGifSource("https://media.discordapp.net/attachments/1/2/test.gif?width=300"), "Discord GIF query accepted");
Check(!MediaImages.IsGifSource("https://media.discordapp.net/attachments/1/2/test.gif?format=webp"), "Transcoded WebP is not advertised as GIF");
Check(MediaImages.IsGifSource("https://media.discordapp.net/attachments/1/2/test.gif?format=gif"), "Explicit GIF format accepted");
foreach (string invalid in new[] { "http://tenor.com/view/test", "https://tenor.com.evil.invalid/view/a", "https://evil.invalid/a.gif", "https://media1.tenor.com.evil.invalid/a.gif", "https://user:password@media.tenor.com/a.gif", "https://media.tenor.com:8443/a.gif", "file:///C:/private.gif", "https://127.0.0.1/a.gif", "https://tenor.com/search", "https://giphy.com/" })
    Check(!MediaImages.IsGifSource(invalid), "Rejected unsupported source");
Check(GifSources.ExtractGifUrl("<meta content='" + media + "' property='og:image'>") == media, "Metadata attribute order and single quotes");
Check(GifSources.ExtractGifUrl("<meta name=\"twitter:image\" content=\"" + media + "?a=1&amp;b=2\">") == media + "?a=1&b=2", "HTML encoded query decoded");
Check(GifSources.ExtractGifUrl("<script>\"" + media + "\"</script>") == null, "Scripts not used as media metadata");
Check(GifSources.ExtractGifUrl("<meta property='og:image' content='https://127.0.0.1/private.gif'>") == null, "Metadata cannot broaden download hosts");
Check(GifSources.ExtractGifUrl("<meta property='og:image' content='https://media1.tenor.com/static.webp'><meta property='og:image:secure_url' content='" + media + "'>") == media, "Static poster does not hide GIF variant");
object? publicFixture = null;
if (args.Contains("--public-fixture"))
{
    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
    string? resolved = await GifSources.ResolveAsync(source, timeout.Token);
    Check(resolved != null && GifSources.IsDirectGif(resolved), "Reported public Tenor page resolves");
    byte[]? bytes = await MediaImages.GetGifBytesAsync(source, timeout.Token);
    Check(bytes is { Length: > 6 }, "Reported GIF downloads actual GIF bytes");
    var poster = await MediaImages.GetAsync(source, 384, timeout.Token);
    Check(poster is { PixelWidth: > 0, PixelHeight: > 0 }, "Reported GIF has a decoded poster");
    if (bytes != null)
    {
        var decode = typeof(UiGifImage).GetMethod("Decode", BindingFlags.NonPublic | BindingFlags.Static)!;
        object decoded = decode.Invoke(null, new object[] { bytes, timeout.Token })!;
        var frames = (System.Collections.IEnumerable)decoded.GetType().GetProperty("Frames")!.GetValue(decoded)!;
        int count = frames.Cast<object>().Count(); Check(count > 1, "Reported GIF decodes animated frames");
        publicFixture = new { provider = "Tenor", bytes = bytes.Length, frames = count, width = poster?.PixelWidth, height = poster?.PixelHeight };
    }
}
if (args.Contains("--klipy-public-fixture"))
{
    const string publicGif = "https://static.klipy.com/ii/39f2394ae36df6e199be9eb7c9fa1012/d7/83/dkpdf8yW.gif";
    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
    byte[]? bytes = await MediaImages.GetGifBytesAsync(publicGif, timeout.Token);
    var poster = await MediaImages.GetAsync(publicGif, 384, timeout.Token);
    Check(bytes is { Length: > 6 } && poster is { PixelWidth: > 0, PixelHeight: > 0 }, "Official public Klipy sample delivers actual GIF bytes and a WPF decoded poster");
    if (bytes != null)
    {
        object decoded = typeof(UiGifImage).GetMethod("Decode", BindingFlags.NonPublic | BindingFlags.Static)!.Invoke(null, new object[] { bytes, timeout.Token })!;
        int count = ((System.Collections.IEnumerable)decoded.GetType().GetProperty("Frames")!.GetValue(decoded)!).Cast<object>().Count();
        Check(count > 1, "Official Klipy sample decodes multiple animated frames");
        string output = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "evidence", "public-klipy-20261020.gif"));
        File.WriteAllBytes(output, bytes);
        publicFixture = new { provider = "Klipy", source = publicGif, officialDocumentation = "https://docs.klipy.com/migrate-from-tenor/response-objects/content-formats", bytes = bytes.Length, frames = count, width = poster?.PixelWidth, height = poster?.PixelHeight, sha256 = Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(bytes)), artifact = output };
    }
}
Console.WriteLine(JsonSerializer.Serialize(new { success = errors.Count == 0, assertions, errors, publicFixture, userAccountUsed = false }, new JsonSerializerOptions { WriteIndented = true }));
return errors.Count == 0 ? 0 : 1;

namespace NotDiscord.Terminal
{
    public record UiAction(string Kind, string Id = "", string Text = "", string Extra = "", string ChannelId = "");
    public static class TerminalWindow
    {
        public static System.Windows.Media.Brush Muted => System.Windows.Media.Brushes.LightGray;
        public static Button Button(string text, Action action) { var button = new Button { Content = text }; button.Click += (_, _) => action(); return button; }
        internal static ContextMenu MediaMenu(string url, Action<UiAction> action) => new();
    }
}
