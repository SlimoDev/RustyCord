using System.Globalization;
using System.Reflection;
using System.Text;

namespace NotDiscord.Terminal;

public sealed record EmojiEntry(string Value, string Category, string Name, string Keywords);

/// <summary>Unicode 16 RGI sequences, rendered by Windows; loaded only on first picker use.</summary>
public static class EmojiCatalog
{
    private static readonly Lazy<IReadOnlyList<EmojiEntry>> Catalog = new(Load);
    public static IReadOnlyList<EmojiEntry> All => Catalog.Value;
    public static readonly IReadOnlyDictionary<string, string> Categories = new Dictionary<string, string>
    {
        ["Smileys & Emotion"] = "Caras y emociones", ["People & Body"] = "Personas y cuerpo",
        ["Animals & Nature"] = "Animales y naturaleza", ["Food & Drink"] = "Comida y bebida",
        ["Travel & Places"] = "Viajes y lugares", ["Activities"] = "Actividades",
        ["Objects"] = "Objetos", ["Symbols"] = "Símbolos", ["Flags"] = "Banderas"
    };
    public static string Normalize(string value)
    {
        var result = new StringBuilder(value.Length);
        foreach (char c in value.Normalize(NormalizationForm.FormD))
            if (CharUnicodeInfo.GetUnicodeCategory(c) != UnicodeCategory.NonSpacingMark) result.Append(char.ToLowerInvariant(c));
        return result.ToString().Normalize(NormalizationForm.FormC).Replace('_', ' ');
    }
    public static IEnumerable<EmojiEntry> Search(string category, string text)
    {
        string query = Normalize(text.Trim());
        return All.Where(e => (category.Length == 0 || category == e.Category)
            && (query.Length == 0 || e.Value.Contains(text, StringComparison.Ordinal) || e.Keywords.Contains(query, StringComparison.Ordinal)));
    }
    private static IReadOnlyList<EmojiEntry> Load()
    {
        using var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("NotDiscord.EmojiCatalog");
        if (stream == null) return Array.Empty<EmojiEntry>();
        using var reader = new StreamReader(stream, Encoding.UTF8);
        var result = new List<EmojiEntry>(3800);
        while (reader.ReadLine() is { } line)
        {
            string[] fields = line.Split('\t');
            if (fields.Length < 3 || fields[0].Length > 64) continue;
            string es = fields.Length > 3 ? fields[3] : "";
            string name = es.Split('|')[0].Trim();
            result.Add(new(fields[0], fields[1], name.Length == 0 ? fields[2] : name,
                Normalize(fields[2] + " " + es)));
        }
        return result.AsReadOnly();
    }
}
