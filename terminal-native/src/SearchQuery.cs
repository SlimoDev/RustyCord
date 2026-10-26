using System.Globalization;
using System.Text.RegularExpressions;

namespace NotDiscord.Terminal;

public sealed record SearchContext(string CurrentUserId, IReadOnlyList<Person> People, IReadOnlyList<NavItem> Channels);
public sealed record ParsedSearch(string Content, IReadOnlyList<KeyValuePair<string, string>> Filters, string AuthorType = "")
{
    public string QueryString => string.Join('&', Filters.Concat(Content.Length > 0 ? new[] { new KeyValuePair<string, string>("content", Content) } : Array.Empty<KeyValuePair<string, string>>())
        .Select(pair => Uri.EscapeDataString(pair.Key) + "=" + Uri.EscapeDataString(pair.Value)));
    public bool Matches(ChatMessage message) => AuthorType switch { "user" => !message.Author.IsBot && message.WebhookId.Length == 0,
        "bot" => message.Author.IsBot && message.WebhookId.Length == 0, "webhook" => message.WebhookId.Length > 0, _ => true };
}

/// <summary>Pure search parser; names resolve only against bounded, already-known data.</summary>
public static class SearchQuery
{
    private static readonly Regex Filter = new("(?<!\\S)(?<key>from|has|in|before|after|during|mentions|pinned|author|author_type|author-type):\\s*(?<value>\"[^\"]*\"|[^\\s]+)", RegexOptions.Compiled | RegexOptions.IgnoreCase | RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(100));
    private static readonly Regex MissingValue = new("(?<!\\S)(from|has|in|before|after|during|mentions|pinned|author|author_type|author-type):\\s*$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(100));
    private static readonly HashSet<string> HasValues = new(StringComparer.Ordinal) { "link", "embed", "file", "image", "video", "sound", "sticker", "poll", "snapshot" };
    private static readonly DateTimeOffset DiscordEpoch = new(2015, 1, 1, 0, 0, 0, TimeSpan.Zero);
    public static ParsedSearch Parse(string input, SearchContext context)
    {
        if (string.IsNullOrWhiteSpace(input) || input.Length > 2000) throw new ArgumentException("Escribe una búsqueda de entre 1 y 2000 caracteres.");
        input = SearchCompletion.NormalizeAliases(input);
        if (MissingValue.IsMatch(input)) throw new ArgumentException("Hay un filtro sin valor. Ejemplo: from:usuario has:image before:2026-10-01.");
        var filters = new List<KeyValuePair<string, string>>();
        var unique = new Dictionary<string, string>(StringComparer.Ordinal);
        string authorType = "";
        void AddFilter(string apiKey, string apiValue, string name)
        {
            if (apiKey is "pinned" or "max_id" or "min_id")
            {
                if (unique.TryGetValue(apiKey, out var old) && old != apiValue) throw new ArgumentException("Hay filtros incompatibles para " + name + ".");
                unique[apiKey] = apiValue;
            }
            if (!filters.Contains(new(apiKey, apiValue))) filters.Add(new(apiKey, apiValue));
            if (filters.Count > 20) throw new ArgumentException("Usa como máximo 20 filtros por búsqueda.");
        }
        string text = Filter.Replace(input, match =>
        {
            string key = match.Groups["key"].Value.ToLowerInvariant();
            string value = match.Groups["value"].Value;
            if (value.StartsWith('"')) value = value[1..^1];
            if (value.Length == 0 || value.Contains('"')) throw new ArgumentException("El filtro contiene comillas incompletas o un valor vacío.");
            string apiKey, apiValue;
            switch (key)
            {
                case "from": case "mentions": apiKey = key == "from" ? "author_id" : "mentions"; apiValue = Resolve(value, context.People.Take(5000).SelectMany(p => new[] { (p.Id, p.Name), (p.Id, p.Username) }), context.CurrentUserId, true); break;
                case "in": apiKey = "channel_id"; apiValue = Resolve(value, context.Channels.Take(2000).Select(c => (c.Id, c.Name)), "", false); break;
                case "has":
                    apiKey = "has"; apiValue = value.ToLowerInvariant();
                    if (apiValue == "forward") apiValue = "snapshot";
                    if (!HasValues.Contains(apiValue)) throw new ArgumentException("has: admite link, embed, file, image, video, sound, sticker, poll o forward.");
                    break;
                case "author": case "author_type": case "author-type":
                    string type = value.ToLowerInvariant();
                    if (type is not ("user" or "bot" or "webhook")) throw new ArgumentException("author: admite user, bot o webhook.");
                    if (authorType.Length > 0 && authorType != type) throw new ArgumentException("Hay filtros incompatibles de tipo de autor.");
                    authorType = type; return " ";
                case "during":
                    DateOnly from, until;
                    if (DateOnly.TryParseExact(value, "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out from)) until = from.AddDays(1);
                    else if (DateOnly.TryParseExact(value + "-01", "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out from)) until = from.AddMonths(1);
                    else if (DateOnly.TryParseExact(value + "-01-01", "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out from)) until = from.AddYears(1);
                    else throw new ArgumentException("during: usa AAAA-MM-DD, AAAA-MM o AAAA.");
                    AddFilter("min_id", DateSnowflake(from), key); AddFilter("max_id", DateSnowflake(until), key); return " ";
                case "pinned":
                    apiKey = "pinned"; apiValue = value.ToLowerInvariant();
                    if (apiValue is not ("true" or "false")) throw new ArgumentException("Usa pinned:true o pinned:false.");
                    break;
                default:
                    apiKey = key == "before" ? "max_id" : "min_id";
                    if (!DateOnly.TryParseExact(value, "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out var date)) throw new ArgumentException("Las fechas deben usar AAAA-MM-DD.");
                    apiValue = DateSnowflake(date);
                    break;
            }
            AddFilter(apiKey, apiValue, key);
            return " ";
        });
        if (unique.TryGetValue("min_id", out var min) && unique.TryGetValue("max_id", out var max) && long.Parse(min, CultureInfo.InvariantCulture) >= long.Parse(max, CultureInfo.InvariantCulture))
            throw new ArgumentException("after: debe ser anterior a before:.");
        text = Regex.Replace(text, @"\s+", " ").Trim();
        return new ParsedSearch(text, filters.AsReadOnly(), authorType);
    }
    private static string DateSnowflake(DateOnly date)
    {
        var instant = new DateTimeOffset(date.ToDateTime(TimeOnly.MinValue), TimeSpan.Zero);
        long milliseconds = (long)(instant - DiscordEpoch).TotalMilliseconds;
        if (milliseconds < 0 || milliseconds > (long.MaxValue >> 22)) throw new ArgumentException("La fecha está fuera del intervalo de búsqueda de Discord.");
        return (milliseconds << 22).ToString(CultureInfo.InvariantCulture);
    }
    private static string Resolve(string value, IEnumerable<(string Id, string Name)> known, string ownId, bool person)
    {
        if (person && value.Equals("me", StringComparison.OrdinalIgnoreCase)) value = ownId;
        value = value.Trim();
        if (value.StartsWith("<@") || value.StartsWith("<#")) value = value.Trim('<', '>', '@', '#', '!');
        else value = value.TrimStart(person ? '@' : '#');
        if (value.Length is >= 1 and <= 20 && value.All(char.IsAsciiDigit)) return value;
        var ids = known.Where(item => item.Name.Equals(value, StringComparison.OrdinalIgnoreCase)).Select(item => item.Id).Distinct().Take(2).ToArray();
        if (ids.Length != 1) throw new ArgumentException(ids.Length == 0 ? "No conozco «" + value + "». Usa su ID o un nombre visible en esta sesión." : "Hay varios «" + value + "». Usa su ID para distinguirlos.");
        return ids[0];
    }
}
