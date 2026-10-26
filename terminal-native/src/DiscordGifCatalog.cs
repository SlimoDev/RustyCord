using System.Text;
using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed record GifQuery(string Mode = "trending", string Text = "", int Limit = 20);
public sealed record GifItem(string Id, string Url, string ThumbnailUrl, string Title = "GIF", int Width = 0, int Height = 0, string AnimationUrl = "");
public sealed record GifCategory(string Name, string ThumbnailUrl);
public sealed record GifPage(IReadOnlyList<GifItem> Items, IReadOnlyList<GifCategory>? Categories = null, bool HasMore = false, string Note = "");

/// <summary>Bounded parsers for the GIF API and the observed FrecencyUserSettings favorite_gifs field.</summary>
public static class DiscordGifCatalog
{
    public static string Endpoint(GifQuery query)
    {
        int limit = RemoteLimit(query.Limit);
        string parameters = $"media_format=gif&locale=es-ES&limit={limit}";
        return query.Mode switch
        {
            "categories" => "gifs/trending?" + parameters,
            "trending" => "gifs/trending-gifs?" + parameters,
            "search" when query.Text.Trim().Length is > 0 and <= 150 => "gifs/search?q=" + Uri.EscapeDataString(query.Text.Trim()) + "&" + parameters,
            "favorites" => "users/@me/settings-proto/2",
            _ => throw new ArgumentException("Consulta de GIF no válida.")
        };
    }
    // These are accepted catalogue sizes, verified against the current service.
    // The native picker retains at most96 items even when the service returns100.
    public static int RemoteLimit(int requested) => requested <= 20 ? 20 : requested <= 50 ? 50 : 100;
    public static int NextLimit(int current) => current < 20 ? 20 : current < 50 ? 50 : 96;
    public static GifPage ForDisplay(GifPage page, GifQuery query)
    {
        if (query.Mode is "favorites" or "categories") return page;
        int limit = Math.Clamp(query.Limit, 1, 96);
        return page with { Items = page.Items.Take(limit).ToArray(), HasMore = limit < 96 && (page.HasMore || page.Items.Count > limit) };
    }
    public static GifPage Parse(JsonElement value, GifQuery query)
    {
        if (query.Mode == "favorites")
        {
            // URL fields are capped at4096, but this is an entire base64 protobuf.
            // ReadFavorites applies its own bounded binary length before decoding.
            string encoded = value.ValueKind == JsonValueKind.Object && value.TryGetProperty("settings", out var settings) && settings.ValueKind == JsonValueKind.String ? settings.GetString() ?? "" : "";
            return new(ReadFavorites(encoded), Note: "Incluye tus guardados de Discord. Las estrellas nuevas se guardan en NotDiscord para esta cuenta.");
        }
        var categories = new List<GifCategory>();
        if (value.ValueKind == JsonValueKind.Object && value.TryGetProperty("categories", out var groups) && groups.ValueKind == JsonValueKind.Array)
            foreach (var group in groups.EnumerateArray().Take(50))
            {
                string name = Text(group, "name");
                if (name.Length is > 0 and <= 150) categories.Add(new(name, Preview(NormalizeLink(Text(group, "src")))));
            }
        JsonElement data = value;
        if (value.ValueKind == JsonValueKind.Object && value.TryGetProperty("gifs", out var list)) data = list;
        var items = new List<GifItem>();
        if (data.ValueKind == JsonValueKind.Array)
            foreach (var item in data.EnumerateArray().Take(Math.Clamp(query.Limit, 1, 96)))
            {
                string link = NormalizeLink(Text(item, "url")), src = NormalizeLink(Text(item, "gif_src"));
                if (src.Length == 0) src = NormalizeLink(Text(item, "gifSrc"));
                if (src.Length == 0) src = NormalizeLink(Text(item, "src"));
                if (!SafeLink(link)) link = SafeLink(src) ? src : "";
                if (link.Length == 0) continue;
                string preview = NormalizeLink(Text(item, "preview"));
                if (Preview(preview).Length == 0) preview = src;
                string title = Text(item, "title");
                string thumbnail = Preview(preview); if (thumbnail.Length == 0 && GifSources.IsPage(link)) thumbnail = link;
                items.Add(new(Text(item, "id"), link, thumbnail, title.Length == 0 ? "GIF" : title, Number(item, "width"), Number(item, "height"), GifSources.IsDirectGif(src) ? src : ""));
            }
        return new(items, categories, items.Count >= query.Limit && query.Limit < 96);
    }
    public static bool SafeLink(string? value)
    {
        if (!Uri.TryCreate(value, UriKind.Absolute, out var uri) || uri.Scheme != "https" || !uri.IsDefaultPort || uri.UserInfo.Length != 0 || value!.Length > 4096) return false;
        return uri.Host is "tenor.com" or "www.tenor.com" or "giphy.com" or "www.giphy.com" or "klipy.com" or "www.klipy.com" or "cdn.discordapp.com" or "media.discordapp.net" || GifSources.IsMediaHost(uri.Host);
    }
    public static string NormalizeLink(string? value)
    {
        string candidate = value?.Trim() ?? "";
        if (candidate.StartsWith("//", StringComparison.Ordinal)) candidate = "https:" + candidate;
        return SafeLink(candidate) ? candidate : "";
    }
    private static string Preview(string? value) => SafeLink(value) && !GifSources.IsPage(value)
        && !new Uri(value!).AbsolutePath.EndsWith(".mp4", StringComparison.OrdinalIgnoreCase)
        && !new Uri(value!).AbsolutePath.EndsWith(".webm", StringComparison.OrdinalIgnoreCase) ? value! : "";
    private static string Text(JsonElement item, string name) => item.ValueKind == JsonValueKind.Object && item.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? (v.GetString() ?? "")[..Math.Min(v.GetString()!.Length, 4096)] : "";
    private static int Number(JsonElement item, string name) => item.ValueKind == JsonValueKind.Object && item.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.Number && v.TryGetInt32(out var n) ? Math.Clamp(n, 0, 8192) : 0;

    public static IReadOnlyList<GifItem> ReadFavorites(string encoded)
    {
        if (encoded.Length > 1_400_000) throw new ArgumentException("Los guardados superan el límite permitido.");
        byte[] data;
        try { data = Convert.FromBase64String(encoded); } catch (FormatException) { throw new ArgumentException("Guardados no válidos."); }
        if (data.Length > 1024 * 1024) throw new ArgumentException("Los guardados superan el límite permitido.");
        var found = new List<(GifItem Item, ulong Order)>();
        foreach (var root in Fields(data))
        {
            if (root.Number != 2 || root.Wire != 2) continue;
            foreach (var entry in Fields(root.Data))
            {
                if (entry.Number != 1 || entry.Wire != 2 || found.Count >= 400) continue;
                string link = "", src = ""; int width = 0, height = 0; ulong order = 0;
                foreach (var map in Fields(entry.Data))
                {
                    if (map.Number == 1 && map.Wire == 2) link = Encoding.UTF8.GetString(map.Data);
                    if (map.Number != 2 || map.Wire != 2) continue;
                    foreach (var value in Fields(map.Data))
                    {
                        if (value.Number == 2 && value.Wire == 2) src = Encoding.UTF8.GetString(value.Data);
                        if (value.Number == 3 && value.Wire == 0) width = (int)Math.Min(value.Scalar, 8192UL);
                        if (value.Number == 4 && value.Wire == 0) height = (int)Math.Min(value.Scalar, 8192UL);
                        if (value.Number == 5 && value.Wire == 0) order = value.Scalar;
                    }
                }
                link = NormalizeLink(link); src = NormalizeLink(src);
                if (!SafeLink(link)) link = SafeLink(src) ? src : "";
                if (link.Length > 0) { string thumbnail = Preview(src); if (thumbnail.Length == 0 && GifSources.IsPage(link)) thumbnail = link; found.Add((new(link, link, thumbnail, "Guardado en Discord", width, height, GifSources.IsDirectGif(src) ? src : ""), order)); }
            }
        }
        return found.OrderByDescending(e => e.Order).Select(e => e.Item).DistinctBy(e => e.Url).ToArray();
    }
    private readonly record struct Field(int Number, int Wire, byte[] Data, ulong Scalar);
    private static IEnumerable<Field> Fields(byte[] bytes)
    {
        int cursor = 0, count = 0;
        while (cursor < bytes.Length)
        {
            if (++count > 6000) throw new ArgumentException("Demasiados campos de guardados.");
            ulong tag = Varint(bytes, ref cursor); int field = (int)(tag >> 3), wire = (int)(tag & 7);
            if (field < 1) throw new ArgumentException("Campo de guardados no válido.");
            if (wire == 0) { yield return new(field, wire, Array.Empty<byte>(), Varint(bytes, ref cursor)); continue; }
            ulong size = wire switch { 1 => 8, 5 => 4, 2 => Varint(bytes, ref cursor), _ => throw new ArgumentException("Formato de guardados no compatible.") };
            if (size > (ulong)(bytes.Length - cursor)) throw new ArgumentException("Guardados truncados.");
            byte[] value = wire == 2 ? bytes.AsSpan(cursor, (int)size).ToArray() : Array.Empty<byte>(); cursor += (int)size;
            yield return new(field, wire, value, 0);
        }
    }
    private static ulong Varint(byte[] bytes, ref int cursor)
    {
        ulong value = 0;
        for (int shift = 0; shift < 70; shift += 7)
        {
            if (cursor >= bytes.Length) throw new ArgumentException("Guardados truncados.");
            byte part = bytes[cursor++];
            if (shift == 63 && part > 1) throw new ArgumentException("Número de guardados no válido.");
            value |= (ulong)(part & 127) << shift; if ((part & 128) == 0) return value;
        }
        throw new ArgumentException("Número de guardados no válido.");
    }
}
