namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private readonly Dictionary<string, (DateTimeOffset At, IReadOnlyList<CustomEmoji> Items)> pickerEmojis = new();
    private readonly Dictionary<string, (DateTimeOffset At, GifPage Page)> pickerGifs = new();
    private int pickerAccountGeneration = -1;
    private void ResetPickerAccount()
    {
        if (pickerAccountGeneration == sessionGeneration) return;
        pickerAccountGeneration = sessionGeneration; pickerEmojis.Clear(); pickerGifs.Clear();
    }
    public async Task<IReadOnlyList<CustomEmoji>> GetPickerEmojisAsync(string guildId, CancellationToken cancellation)
    {
        ResetPickerAccount();
        if (guildId.Length == 0 || !guildId.All(char.IsAsciiDigit) || !state.Guilds.Any(g => g.Id == guildId)) return Array.Empty<CustomEmoji>();
        var active = session; int generation = sessionGeneration;
        if (active == null) return Array.Empty<CustomEmoji>();
        if (pickerEmojis.TryGetValue(guildId, out var cached) && DateTimeOffset.UtcNow - cached.At < TimeSpan.FromMinutes(5)) return cached.Items;
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellation, sessionWork.Token);
        var value = await active.GetAsync($"guilds/{guildId}/emojis", linked.Token);
        if (!IsCurrent(active, generation)) throw new OperationCanceledException();
        var items = Items(value).Take(500).Where(e => e.TryGetProperty("id", out var id) && id.ValueKind == System.Text.Json.JsonValueKind.String)
            .Select(e => new CustomEmoji(Str(e, "id"), Str(e, "name"), e.TryGetProperty("animated", out var a) && a.ValueKind == System.Text.Json.JsonValueKind.True, guildId))
            .Where(e => e.Id.All(char.IsAsciiDigit) && e.Id.Length > 0 && e.Name.Length is > 0 and <= 100).ToArray();
        if (pickerEmojis.Count >= 3) pickerEmojis.Remove(pickerEmojis.OrderBy(p => p.Value.At).First().Key);
        pickerEmojis[guildId] = (DateTimeOffset.UtcNow, items); return items;
    }
    public async Task<GifPage> GetPickerGifCatalogAsync(GifQuery query, CancellationToken cancellation)
    {
        ResetPickerAccount();
        string endpoint = DiscordGifCatalog.Endpoint(query);
        var active = session; int generation = sessionGeneration;
        if (active == null) return new(Array.Empty<GifItem>(), Note: "Inicia sesión para buscar GIF.");
        if (pickerGifs.TryGetValue(endpoint, out var cached) && DateTimeOffset.UtcNow - cached.At < TimeSpan.FromMinutes(query.Mode == "favorites" ? 1 : 3)) return DiscordGifCatalog.ForDisplay(cached.Page, query);
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellation, sessionWork.Token);
        var value = await active.GetAsync(endpoint, linked.Token);
        if (!IsCurrent(active, generation)) throw new OperationCanceledException();
        var result = DiscordGifCatalog.Parse(value, query with { Limit = DiscordGifCatalog.RemoteLimit(query.Limit) });
        foreach (var gif in result.Items) GifSources.RememberKnownSource(gif.Url, gif.AnimationUrl);
        if (pickerGifs.Count >= 4) pickerGifs.Remove(pickerGifs.OrderBy(p => p.Value.At).First().Key);
        pickerGifs[endpoint] = (DateTimeOffset.UtcNow, result); return DiscordGifCatalog.ForDisplay(result, query);
    }
}
