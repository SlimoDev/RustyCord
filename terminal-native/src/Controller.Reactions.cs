using System.Net.Http;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    public static IReadOnlyList<MessageReaction> ParseReactions(JsonElement reactions) => Items(reactions).Take(100)
        .Where(r => r.ValueKind == JsonValueKind.Object && r.TryGetProperty("emoji", out var e) && e.ValueKind == JsonValueKind.Object)
        .Select(r => new MessageReaction(ParseReactionEmoji(r.GetProperty("emoji")), Math.Clamp(Int(r, "count"), 0, 1000000), Bool(r, "me") || Bool(r, "me_burst"), Bool(r, "me"), Bool(r, "me_burst")))
        .Where(r => r.Count > 0 && r.Emoji.Name.Length > 0).ToArray();
    private static StatusEmoji ParseReactionEmoji(JsonElement emoji)
    {
        string id = Str(emoji, "id"), name = Str(emoji, "name"); if (id.Length > 24 || !id.All(char.IsAsciiDigit)) id = "";
        if (id.Length > 0 && name.Length == 0) name = "emoji";
        return new(name.Length > 100 ? name[..100] : name, id, Bool(emoji, "animated"));
    }
    public static string ReactionKey(StatusEmoji emoji) => emoji.Id.Length > 0 ? emoji.Name + ":" + emoji.Id : emoji.Name;
    private static StatusEmoji ReactionFromText(string text)
    {
        text = text.Trim(); var custom = Regex.Match(text, "^<?(?:(?<animated>a)?:)?(?<name>[A-Za-z0-9_]{1,100}):(?<id>[0-9]{1,24})>?$", RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(50));
        if (custom.Success) return new(custom.Groups["name"].Value, custom.Groups["id"].Value, custom.Groups["animated"].Success);
        if (text.Length is < 1 or > 80 || text.Any(char.IsControl) || text.Contains('/') || text.Contains('<')) throw new ServiceException("Selecciona un emoji válido para reaccionar.");
        return new(text);
    }
    private bool HandleReactionDispatch(string type, JsonElement data)
    {
        if (type is not ("MESSAGE_REACTION_ADD" or "MESSAGE_REACTION_REMOVE" or "MESSAGE_REACTION_REMOVE_ALL" or "MESSAGE_REACTION_REMOVE_EMOJI")) return false;
        string channel = Str(data, "channel_id"), id = Str(data, "message_id");
        var message = cache.Get(channel, id) ?? state.PinnedMessages.FirstOrDefault(m => m.ChannelId == channel && m.Id == id);
        if (message == null) return true;
        var values = (message.Reactions ?? Array.Empty<MessageReaction>()).ToList();
        if (type == "MESSAGE_REACTION_REMOVE_ALL") values.Clear();
        else if (data.TryGetProperty("emoji", out var raw))
        {
            var emoji = ParseReactionEmoji(raw); string key = ReactionKey(emoji); int index = values.FindIndex(r => ReactionKey(r.Emoji) == key);
            if (type == "MESSAGE_REACTION_REMOVE_EMOJI") { if (index >= 0) values.RemoveAt(index); }
            else
            {
                bool own = Str(data, "user_id") == state.CurrentUser?.Id, added = type == "MESSAGE_REACTION_ADD", burst = Bool(data, "burst") || Int(data, "type") == 1;
                if (index < 0 && added && values.Count < 100) values.Add(new(emoji, 1, own, own && !burst, own && burst));
                else if (index >= 0)
                {
                    var prior = values[index]; int count = Math.Clamp(prior.Count + (added ? 1 : -1), 0, 1000000);
                    bool normalMe = prior.NormalMe ?? prior.Me, burstMe = prior.BurstMe;
                    if (own) { if (burst) burstMe = added; else normalMe = added; }
                    if (count == 0) values.RemoveAt(index); else values[index] = prior with { Count = count, Me = normalMe || burstMe, NormalMe = normalMe, BurstMe = burstMe };
                }
            }
        }
        var changed = message with { Reactions = values.ToArray() };
        if (cache.ContainsChannel(channel)) cache.Upsert(changed);
        state = state with { PinnedMessages = state.PinnedMessages.Select(m => m.Id == id && m.ChannelId == channel ? changed : m).ToArray() };
        UpdateMessages(); Refresh(); return true;
    }
    private async Task ToggleReactionAsync(UiAction action, DiscordSession active, int generation, bool toggle)
    {
        RequireSelectedSocialChannel(action); RequirePermission("history");
        var message = cache.Get(currentChannel, action.Id) ?? throw new ServiceException("El mensaje ya no está cargado.");
        var emoji = ReactionFromText(action.Text); string key = ReactionKey(emoji);
        var existing = message.Reactions?.FirstOrDefault(r => ReactionKey(r.Emoji) == key);
        bool remove = toggle && existing?.Me == true;
        string reactionSuffix = remove && existing?.NormalMe == false && existing.BurstMe ? "1/@me?burst=true" : "@me";
        // ADD_REACTIONS is needed only for a new emoji, not an already existing bucket.
        if (!remove && existing == null) RequirePermission("react");
        if (!remove && selectedGuildAccess?.TimedOut == true && currentGuild.Length > 0) throw new ServiceException("No puedes reaccionar mientras se aplica la restricción temporal.");
        string channel = currentChannel;
        await active.SendAsync(remove ? HttpMethod.Delete : HttpMethod.Put, $"channels/{Snowflake(channel)}/messages/{Snowflake(action.Id)}/reactions/{Uri.EscapeDataString(key)}/{reactionSuffix}", ct: sessionWork.Token);
        if (!IsCurrent(active, generation)) return;
        // Gateway is authoritative. Refresh only this explicitly acted-on message if
        // the event was absent; never poll an entire channel or guess reaction counts.
        var response = await active.GetAsync($"channels/{channel}/messages/{Snowflake(action.Id)}", sessionWork.Token);
        if (!IsCurrent(active, generation)) return;
        if (Str(response, "id") == action.Id) cache.Upsert(ParseMessage(response));
        UpdateMessages(); Status(remove ? "Reacción retirada" : "Reacción añadida");
    }
}
