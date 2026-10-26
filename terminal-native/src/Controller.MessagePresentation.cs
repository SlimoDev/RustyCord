using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private static ChatMessage ApplyCallMetadata(ChatMessage message, JsonElement call)
    {
        var ended = call.ValueKind == JsonValueKind.Object && call.TryGetProperty("ended_timestamp", out _)
            ? DateTimeOffset.TryParse(Str(call, "ended_timestamp"), out var end) ? end : (DateTimeOffset?)null
            : message.CallEndedAt;
        string[] participants = call.ValueKind == JsonValueKind.Object && call.TryGetProperty("participants", out var values)
            ? Items(values).Where(p => p.ValueKind == JsonValueKind.String).Select(p => p.GetString()!).Take(100).ToArray() : message.CallParticipantIds?.ToArray() ?? Array.Empty<string>();
        var started = message.CallStartedAt ?? message.Timestamp;
        string summary = ended.HasValue ? (participants.Length == 0 ? "Llamada finalizada sin participantes" : "Llamada finalizada") : "Llamada iniciada";
        if (ended.HasValue && ended >= started && started != default)
        {
            var elapsed = ended.Value - started;
            summary += " · " + (elapsed.TotalHours >= 1 ? ((int)elapsed.TotalHours) + ":" + elapsed.Minutes.ToString("00") + ":" + elapsed.Seconds.ToString("00") : ((int)elapsed.TotalMinutes) + ":" + elapsed.Seconds.ToString("00"));
        }
        return message with { MessageType = 3, Content = summary, CallStartedAt = started == default ? null : started, CallEndedAt = ended, CallParticipantIds = participants };
    }
    private static bool TrustedMedia(string url) => GifSources.SafeUri(url, out var uri) && (uri.Host is "cdn.discordapp.com" or "media.discordapp.net" || GifSources.IsMediaHost(uri.Host));
    private static bool PreservesGif(string url)
    {
        if (!GifSources.IsDirectGif(url)) return false;
        var uri = new Uri(url);
        foreach (string item in uri.Query.TrimStart('?').Split('&'))
        {
            var parts = item.Split('=', 2);
            if (parts.Length == 2 && parts[0].Equals("format", StringComparison.OrdinalIgnoreCase) && !parts[1].Equals("gif", StringComparison.OrdinalIgnoreCase)) return false;
        }
        return true;
    }
    private static IReadOnlyList<ChatAttachment> ParseAttachments(JsonElement files) => Items(files).Take(20).Select(f =>
    {
        string original = Str(f, "url"), proxy = Str(f, "proxy_url"), contentType = Str(f, "content_type");
        bool gif = PreservesGif(original) || PreservesGif(proxy);
        string chosen = gif ? (PreservesGif(original) ? original : proxy) : proxy.Length > 0 ? proxy : original;
        if (!gif && contentType.Equals("image/gif", StringComparison.OrdinalIgnoreCase)) contentType = "image/unknown";
        return new ChatAttachment(Str(f, "id"), Str(f, "filename"), chosen, gif ? "image/gif" : contentType, Int(f, "width"), Int(f, "height"),
            f.TryGetProperty("size", out var size) && size.ValueKind == JsonValueKind.Number && size.TryGetInt64(out long count) ? count : 0, Animated: gif, PreviewUrl: proxy.Length > 0 && proxy != chosen ? proxy : null);
    }).ToArray();
    private static IReadOnlyList<ChatAttachment> ParseEmbeds(JsonElement embeds)
    {
        var output = new List<ChatAttachment>();
        foreach (var embed in Items(embeds).Take(10))
        {
            if (embed.ValueKind != JsonValueKind.Object) continue;
            var image = embed.TryGetProperty("image", out var primary) ? primary : default;
            var thumbnail = embed.TryGetProperty("thumbnail", out var thumb) ? thumb : default;
            string page = Str(embed, "url");
            string preview = Str(thumbnail, "proxy_url"); if (preview.Length == 0) preview = Str(thumbnail, "url");
            string direct = new[] { Str(image, "url"), Str(image, "proxy_url"), Str(thumbnail, "url"), Str(thumbnail, "proxy_url") }.FirstOrDefault(PreservesGif) ?? "";
            if (GifSources.IsPage(page))
            {
                if (direct.Length > 0) GifSources.RememberKnownSource(page, direct);
                output.Add(new("embed-" + output.Count, "GIF", page, "image/gif", Int(thumbnail, "width"), Int(thumbnail, "height"), Animated: true, PreviewUrl: TrustedMedia(preview) ? preview : null));
                continue;
            }
            if (direct.Length > 0)
            {
                output.Add(new("embed-" + output.Count, "GIF", direct, "image/gif", Int(image, "width"), Int(image, "height"), Animated: true, PreviewUrl: TrustedMedia(preview) && preview != direct ? preview : null));
                continue;
            }
            foreach (var source in new[] { image, thumbnail })
            {
                string url = Str(source, "proxy_url"); if (!TrustedMedia(url)) url = Str(source, "url");
                if (!TrustedMedia(url) || output.Any(a => a.Url == url)) continue;
                output.Add(new("embed-" + output.Count, "Imagen", url, "image/png", Int(source, "width"), Int(source, "height")));
            }
        }
        return output;
    }
}
