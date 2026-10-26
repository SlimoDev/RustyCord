using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed record PickerValue(string Value, string Name, string ImageUrl = "");
public sealed record GifImportResult(int Imported, int Existing, int Rejected);
public sealed class PickerHistory
{
    public sealed record Values
    {
        public List<PickerValue> EmojiRecent { get; init; } = new();
        public List<PickerValue> GifFavorites { get; init; } = new();
        public List<PickerValue> GifRecent { get; init; } = new();
    }
    private readonly string path;
    public Values Data { get; private set; } = new();
    public PickerHistory(string directory, string account)
    {
        string scope = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(account)))[..24];
        path = Path.Combine(directory, "pickers", scope + ".json");
        try
        {
            var file = new FileInfo(path);
            if (file.Exists && file.Length <= 256 * 1024) Data = JsonSerializer.Deserialize<Values>(File.ReadAllText(path)) ?? new();
            Data = new() { EmojiRecent = Sanitize(Data.EmojiRecent, false, 10), GifFavorites = Sanitize(Data.GifFavorites, true, 100), GifRecent = Sanitize(Data.GifRecent, true, 32) };
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException or ArgumentException or NullReferenceException) { Data = new(); }
    }
    private static List<PickerValue> Sanitize(IEnumerable<PickerValue>? values, bool gif, int limit) => (values ?? Enumerable.Empty<PickerValue>())
        .Where(v => v != null && Valid(v, gif)).DistinctBy(v => v.Value).Take(limit).ToList();
    public static bool Valid(PickerValue value, bool gif) => value.Value is { Length: > 0 and <= 4096 } && value.Name is { Length: <= 160 }
        && value.ImageUrl is { Length: <= 4096 } && !value.Value.Any(char.IsControl)
        && (gif ? DiscordGifCatalog.SafeLink(value.Value) : value.Value.Length <= 160)
        && (value.ImageUrl.Length == 0 || DiscordGifCatalog.SafeLink(value.ImageUrl));
    public bool FavoriteGif(PickerValue value)
    {
        if (!Valid(value, true)) return false;
        var values = Data.GifFavorites;
        bool was = values.RemoveAll(v => v.Value == value.Value) > 0;
        if (!was) values.Insert(0, value);
        if (values.Count > 100) values.RemoveRange(100, values.Count - 100);
        Save(); return !was;
    }
    public void Recent(PickerValue value, bool gif)
    {
        if (!Valid(value, gif)) return;
        var values = gif ? Data.GifRecent : Data.EmojiRecent;
        values.RemoveAll(v => v.Value == value.Value); values.Insert(0, value);
        int max = gif ? 32 : 10; if (values.Count > max) values.RemoveRange(max, values.Count - max); Save();
    }
    public GifImportResult ImportGifLinks(string text)
    {
        if (text.Length > 256 * 1024) throw new ArgumentException("La lista debe ocupar como máximo 256 KiB.");
        var imported = new List<PickerValue>(); int existing = 0, rejected = 0;
        var known = Data.GifFavorites.Select(v => v.Value).ToHashSet(StringComparer.Ordinal);
        foreach (string line in text.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries).Take(500))
        {
            string link = DiscordGifCatalog.NormalizeLink(line);
            if (link.Length == 0 || !(GifSources.IsDirectGif(link) || GifSources.IsPage(link))) { rejected++; continue; }
            if (!known.Add(link)) { existing++; continue; }
            if (imported.Count + Data.GifFavorites.Count >= 100) { rejected++; continue; }
            imported.Add(new(link, "GIF importado", link));
        }
        if (imported.Count > 0) { Data.GifFavorites.InsertRange(0, imported); Save(); }
        return new(imported.Count, existing, rejected);
    }
    private void Save()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        string temporary = path + ".tmp";
        File.WriteAllText(temporary, JsonSerializer.Serialize(Data)); File.Move(temporary, path, true);
    }
}
