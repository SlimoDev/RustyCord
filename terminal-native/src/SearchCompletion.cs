using System.Globalization;
using System.Text.RegularExpressions;

namespace NotDiscord.Terminal;

public sealed record SearchSuggestion(string Key, string Value, string Title, string Description);
public sealed record SearchToken(string Key, string Partial, int Start, int End);

/// <summary>Pure bounded completion over already-known people/channels; never queries Discord.</summary>
public static class SearchCompletion
{
    private const string Keys = "from|has|mentions|in|before|after|during|pinned|author|author_type|author-type";
    private static readonly Regex Active = new("(?<!\\S)(?:(?<key>" + Keys + "):\\s*|:(?<alias>" + Keys + ")(?::|\\s+|$)\\s*)(?<value>\"[^\"]*\"?|[^\\s]*)$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(80));
    private static readonly Regex PrefixAlias = new("(?<!\\S):(?<key>" + Keys + ")(?=[:\\s]|$):?", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(80));
    public static string NormalizeAliases(string input) => PrefixAlias.Replace(input, m => m.Groups["key"].Value + ":");
    public static SearchToken? At(string input, int caret)
    {
        if (input.Length > 2000) return null;
        caret = Math.Clamp(caret, 0, input.Length);
        var match = Active.Match(input[..caret]);
        if (!match.Success) return null;
        string key = match.Groups["key"].Success ? match.Groups["key"].Value : match.Groups["alias"].Value;
        string value = match.Groups["value"].Value;
        int end = caret;
        bool quote = value.StartsWith('"') && !value.EndsWith('"') || value == "\"";
        while (end < input.Length && (quote || !char.IsWhiteSpace(input[end])))
        { if (input[end] == '"' && quote) { end++; break; } end++; }
        return new(key.ToLowerInvariant(), value.Trim('"'), match.Index, end);
    }
    public static IReadOnlyList<SearchSuggestion> Suggest(string input, int caret, SearchContext context, DateOnly? today = null)
    {
        var token = At(input, caret);
        if (token == null) return new SearchSuggestion[]
        {
            new("from", "", "De una persona", "from: usuario"), new("has", "", "Contiene un tipo de contenido", "has: enlace, imagen, archivo…"),
            new("mentions", "", "Menciona a una persona", "mentions: usuario"), new("in", "", "En un canal", "in: canal"),
            new("before", "", "Antes de una fecha", "before: AAAA-MM-DD"), new("after", "", "Después de una fecha", "after: AAAA-MM-DD"),
            new("during", "", "Durante un día, mes o año", "during: AAAA-MM-DD · AAAA-MM · AAAA"), new("pinned", "", "Mensajes fijados", "pinned: true o false")
        };
        string partial = token.Partial;
        bool Match(string text) => partial.Length == 0 || text.Contains(partial, StringComparison.OrdinalIgnoreCase);
        IEnumerable<SearchSuggestion> result;
        switch (token.Key)
        {
            case "from": case "mentions":
                var people = context.People.Where(p => p.Id.Length > 0).DistinctBy(p => p.Id).Take(2000);
                result = new[] { new SearchSuggestion(token.Key, "me", "Yo", "Tu cuenta") }.Where(_ => Match("me yo " + context.CurrentUserId))
                    .Concat(people.Where(p => Match(p.Name + " " + p.Username + " " + p.Id)).Select(p => new SearchSuggestion(token.Key, p.Id, p.Name, p.Username.Length > 0 ? "@" + p.Username : "Persona conocida en esta sesión")));
                break;
            case "has":
                result = new[] { ("link", "Enlace"), ("image", "Imagen"), ("video", "Vídeo"), ("file", "Archivo"), ("embed", "Contenido incrustado"), ("sound", "Audio"), ("poll", "Encuesta"), ("forward", "Reenvío"), ("sticker", "Sticker") }
                    .Where(p => Match(p.Item1 + " " + p.Item2)).Select(p => new SearchSuggestion(token.Key, p.Item1, p.Item2, "has:" + p.Item1));
                break;
            case "in":
                result = context.Channels.Where(c => c.Kind is "text" or "thread" && Match(c.Name + " " + c.Id)).DistinctBy(c => c.Id).Take(2000)
                    .Select(c => new SearchSuggestion(token.Key, c.Id, "# " + c.Name, "Sólo este canal"));
                break;
            case "pinned":
                result = new[] { new SearchSuggestion(token.Key, "true", "Sí, fijados", "pinned:true"), new SearchSuggestion(token.Key, "false", "Sin fijar", "pinned:false") }.Where(p => Match(p.Value + " " + p.Title));
                break;
            case "author": case "author_type": case "author-type":
                result = new[] { ("user", "Personas"), ("bot", "Bots"), ("webhook", "Webhooks") }.Where(p => Match(p.Item1 + " " + p.Item2)).Select(p => new SearchSuggestion(token.Key, p.Item1, p.Item2, "Filtro de resultados cargados"));
                break;
            default:
                DateOnly date = today ?? DateOnly.FromDateTime(DateTime.Today);
                string day = date.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture), yesterday = date.AddDays(-1).ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);
                result = token.Key == "during" ? new[]
                {
                    new SearchSuggestion(token.Key, day, "Hoy", day), new SearchSuggestion(token.Key, date.ToString("yyyy-MM", CultureInfo.InvariantCulture), "Este mes", "AAAA-MM"), new SearchSuggestion(token.Key, date.Year.ToString(CultureInfo.InvariantCulture), "Este año", "AAAA")
                } : new[] { new SearchSuggestion(token.Key, day, "Hoy", day), new SearchSuggestion(token.Key, yesterday, "Ayer", yesterday) };
                result = result.Where(p => Match(p.Value + " " + p.Title));
                break;
        }
        return result.Take(12).ToArray();
    }
    public static (string Text, int Caret) Complete(string input, int caret, string key, string value)
    {
        if (input.Length > 2000 || !Regex.IsMatch(key, "^(?:" + Keys + ")$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(80))) return (input, Math.Clamp(caret, 0, input.Length));
        caret = Math.Clamp(caret, 0, input.Length);
        var token = At(input, caret);
        string before = token?.Key == key ? input[..token.Start] : input[..caret];
        string after = token?.Key == key ? input[token.End..] : input[caret..];
        string encoded = value.Any(char.IsWhiteSpace) ? '"' + value.Replace("\"", "") + '"' : value;
        string insertion = (before.Length > 0 && !char.IsWhiteSpace(before[^1]) ? " " : "") + key + ":" + encoded;
        if (value.Length > 0 && (after.Length == 0 || !char.IsWhiteSpace(after[0]))) insertion += " ";
        string output = before + insertion + after;
        return output.Length > 2000 ? (input, caret) : (output, before.Length + insertion.Length);
    }
}
