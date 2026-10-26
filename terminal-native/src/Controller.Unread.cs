using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private readonly Dictionary<string, int> mentionCounts = new(StringComparer.Ordinal);
    private readonly Dictionary<string, string> unreadGuilds = new(StringComparer.Ordinal);
    private readonly Dictionary<string, string> latestObservedMessages = new(StringComparer.Ordinal);
    private void RememberMentionCount(string channel, int count)
    {
        if (channel.Length == 0) return;
        if (count <= 0) { mentionCounts.Remove(channel); return; }
        if (mentionCounts.Count >= 2000 && !mentionCounts.ContainsKey(channel)) mentionCounts.Remove(mentionCounts.Keys.First());
        mentionCounts[channel] = Math.Clamp(count, 0, 9999);
    }
    private void RefreshUnreadState()
    {
        if (currentGuild.Length > 0)
            foreach (var channel in state.Channels.Where(n => n.Unread).Take(2000))
                if (unreadGuilds.Count < 2000 || unreadGuilds.ContainsKey(channel.Id)) unreadGuilds[channel.Id] = currentGuild;
        var unread = state.DirectMessages.Concat(state.Channels).Where(n => n.Unread).Select(n => n.Id).ToHashSet(StringComparer.Ordinal);
        state = state with { UnreadMessageCount = unread.Union(unreadGuilds.Keys).Union(activeCalls.Values.Where(c => c.Unread).Select(c => c.ChannelId)).Count(),
            Guilds = state.Guilds.Select(g => g with { Unread = unreadGuilds.Values.Contains(g.Id) }).ToArray() };
    }
    private void NoteIncomingMessage(JsonElement data)
    {
        string channel = Str(data, "channel_id"), guild = Str(data, "guild_id");
        bool firstNotice = RememberNotificationMessage(Str(data, "id"));
        RememberLatestMessage(channel, Str(data, "id"));
        if (!firstNotice) return;
        if (channel.Length == 0 || !data.TryGetProperty("author", out var author)) return;
        if (Str(author, "id") == state.CurrentUser?.Id)
        {
            // Keep an existing unread badge, but do not derive a new one from our own last-message ID.
            bool alreadyUnread = unreadGuilds.ContainsKey(channel) || state.DirectMessages.Concat(state.Channels).Any(n => n.Id == channel && n.Unread);
            if (!alreadyUnread) RememberRead(channel, Str(data, "id"));
            return;
        }
        if (unreadGuilds.Count >= 2000 && !unreadGuilds.ContainsKey(channel)) unreadGuilds.Remove(unreadGuilds.Keys.First());
        unreadGuilds[channel] = guild;
        bool mentioned = Bool(data, "mention_everyone") || (data.TryGetProperty("mentions", out var mentions) && Items(mentions).Take(100).Any(p => Str(p, "id") == state.CurrentUser?.Id));
        if (mentioned) RememberMentionCount(channel, mentionCounts.GetValueOrDefault(channel) + 1);
        state = state with { DirectMessages = state.DirectMessages.Select(n => n.Id == channel ? n with { Unread = true, MentionCount = mentionCounts.GetValueOrDefault(channel) } : n).ToArray(),
            Channels = state.Channels.Select(n => n.Id == channel ? n with { Unread = true, MentionCount = mentionCounts.GetValueOrDefault(channel) } : n).ToArray() };
        var item = state.DirectMessages.Concat(state.Channels).FirstOrDefault(n => n.Id == channel);
        if (!mutedChannels.GetValueOrDefault(channel) && !mutedGuilds.GetValueOrDefault(guild))
        {
            var sender = ParsePerson(author); string name = sender.Name;
            state = state with { NewMessageNotice = "Nuevo mensaje de " + name + (item != null ? " en " + item.Name : guild.Length == 0 ? " en una conversación" : " en un servidor") };
            if (firstNotice && Int(data, "type") is 0 or 19 && (guild.Length == 0 || mentioned))
            {
                string origin = item?.Kind == "dm" ? "" : item?.Name ?? (guild.Length > 0 ? "servidor" : "grupo");
                string title = name + (origin.Length > 0 ? " · " + (guild.Length > 0 ? "#" : "") + origin : "");
                string fallback = data.TryGetProperty("attachments", out var files) && Items(files).Any() ? "Archivo adjunto" : "Nuevo mensaje";
                OfferDesktopNotice(new(channel, title, NotificationText.Preview(Str(data, "content"), fallback), AvatarUrl: sender.AvatarUrl));
            }
        }
        RefreshUnreadState(); Refresh();
    }
    private void ClearUnread(string channel, string messageId)
    {
        var latest = state.DirectMessages.Concat(state.Channels).FirstOrDefault(n => n.Id == channel)?.LastMessageId ?? "";
        if (SnowflakeOrder(latestObservedMessages.GetValueOrDefault(channel) ?? "") > SnowflakeOrder(latest)) latest = latestObservedMessages[channel];
        RememberRead(channel, messageId);
        // An acknowledgement of an older view must not erase messages that arrived later.
        if (SnowflakeOrder(latest) > SnowflakeOrder(messageId)) return;
        RememberMentionCount(channel, 0); unreadGuilds.Remove(channel);
        state = state with { DirectMessages = state.DirectMessages.Select(n => n.Id == channel ? n with { Unread = false, MentionCount = 0 } : n).ToArray(),
            Channels = state.Channels.Select(n => n.Id == channel ? n with { Unread = false, MentionCount = 0 } : n).ToArray(), NewMessageNotice = "" };
        if (activeCalls.TryGetValue(channel, out var call) && call.Unread)
        { activeCalls[channel] = call with { Unread = false }; RefreshIncomingCalls(); return; }
        RefreshUnreadState(); Refresh();
    }
    private void RememberLatestMessage(string channel, string id)
    {
        if (channel.Length == 0 || id.Length == 0 || SnowflakeOrder(id) <= SnowflakeOrder(latestObservedMessages.GetValueOrDefault(channel) ?? "")) return;
        if (latestObservedMessages.Count >= 2000 && !latestObservedMessages.ContainsKey(channel)) latestObservedMessages.Remove(latestObservedMessages.Keys.First());
        latestObservedMessages[channel] = id;
    }
}
