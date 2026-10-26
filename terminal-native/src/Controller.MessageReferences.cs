using System.Net.Http;
using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    public static IReadOnlyList<Person> ParseMessageMentions(JsonElement message)
    {
        if (!message.TryGetProperty("mentions", out var entries)) return Array.Empty<Person>();
        return Items(entries).Take(100).Where(e => ValidMessageId(Str(e, "id"))).Select(e =>
        {
            var person = ParsePerson(e);
            string nickname = e.TryGetProperty("member", out var member) ? Str(member, "nick") : "";
            return person with { Name = Bounded(nickname.Length > 0 ? nickname : person.Name, 128), Username = Bounded(person.Username, 128), DisplayName = Bounded(person.DisplayName, 128) };
        }).DistinctBy(p => p.Id).ToArray();
    }

    public static MessageReplyPreview? ParseReplyPreview(JsonElement message, string fallbackReplyId = "", string fallbackChannel = "")
    {
        bool hasReference = message.TryGetProperty("message_reference", out var reference);
        if (hasReference && (reference.ValueKind != JsonValueKind.Object || Int(reference, "type") == 1)) return null;
        string id = hasReference ? Str(reference, "message_id") : fallbackReplyId, channel = Str(message, "channel_id");
        if (channel.Length == 0) channel = fallbackChannel;
        if (!ValidMessageId(id)) return null;
        string referenceChannel = Str(reference, "channel_id");
        if (referenceChannel.Length > 0 && referenceChannel != channel) return new(id, referenceChannel, null, "", Unavailable: true);
        if (!message.TryGetProperty("referenced_message", out var source)) return new(id, channel, null, "", Unavailable: true);
        if (source.ValueKind == JsonValueKind.Null) return new(id, channel, null, "", Deleted: true);
        if (source.ValueKind != JsonValueKind.Object || Str(source, "id") != id || Str(source, "channel_id") is { Length: > 0 } sourceChannel && sourceChannel != channel)
            return new(id, channel, null, "", Unavailable: true);
        var person = source.TryGetProperty("author", out var author) && author.ValueKind == JsonValueKind.Object ? ParsePerson(author) : null;
        string content = ReplyContent(Str(source, "content"), source.TryGetProperty("attachments", out var attachments) ? Items(attachments).Any() : false,
            source.TryGetProperty("embeds", out var embeds) ? Items(embeds).Any() : false);
        return new(id, channel, person, content, Mentions: ParseMessageMentions(source));
    }

    private static ChatMessage ApplyReplyUpdate(ChatMessage previous, JsonElement data)
    {
        bool hasReference = data.TryGetProperty("message_reference", out var reference);
        if (!hasReference && !data.TryGetProperty("referenced_message", out _)) return previous;
        if (hasReference && (reference.ValueKind != JsonValueKind.Object || Int(reference, "type") == 1))
            return previous with { ReplyTo = null, ReplyPreview = null, Forwarded = reference.ValueKind == JsonValueKind.Object && Int(reference, "type") == 1 };
        string id = hasReference ? Str(reference, "message_id") : previous.ReplyTo ?? "";
        if (!ValidMessageId(id)) return previous with { ReplyTo = null, ReplyPreview = null };
        var preview = ParseReplyPreview(data, id, previous.ChannelId);
        // A reference-only edit must not discard an already received source body.
        if (!data.TryGetProperty("referenced_message", out _) && previous.ReplyTo == id && previous.ReplyPreview != null && preview?.ChannelId == previous.ReplyPreview.ChannelId) preview = previous.ReplyPreview;
        return previous with { ReplyTo = id, ReplyPreview = preview, Forwarded = hasReference ? false : previous.Forwarded };
    }

    private static bool ValidMessageId(string id) => id.Length is >= 1 and <= 24 && id.All(char.IsAsciiDigit);
    private static string Bounded(string text, int max) => text.Length <= max ? text : text[..max];
    private static string ReplyContent(string content, bool files, bool embeds)
        => content.Length > 0 ? Bounded(content, 500) : files ? "Archivo adjunto" : embeds ? "Contenido incrustado" : "Mensaje sin texto";
    private static MessageReplyPreview PreviewOf(ChatMessage message)
        => new(message.Id, message.ChannelId, message.Author, ReplyContent(message.Content, message.Attachments.Any(a => !a.Id.StartsWith("embed-", StringComparison.Ordinal)), message.Attachments.Any(a => a.Id.StartsWith("embed-", StringComparison.Ordinal))), message.Deleted, Mentions: message.Mentions);

    private IReadOnlyList<ChatMessage> ReadContextMessages(string channel)
    {
        var items = cache.Read(channel);
        if (!items.Any(m => m.ReplyTo != null)) return items;
        var lookup = items.ToDictionary(m => m.Id, StringComparer.Ordinal);
        return items.Select(message => message.ReplyTo is { Length: > 0 } id && message.ReplyPreview?.Deleted != true &&
            (message.ReplyPreview == null || message.ReplyPreview.ChannelId == channel) && lookup.TryGetValue(id, out var original)
                ? message with { ReplyPreview = PreviewOf(original) } : message).ToArray();
    }

    private void MarkReplyReferencesDeleted(string channel, string id)
    {
        foreach (var reply in cache.Read(channel).Where(m => m.ReplyTo == id && (m.ReplyPreview == null || m.ReplyPreview.ChannelId == channel)))
            cache.Upsert(reply with { ReplyPreview = new(id, channel, null, "", Deleted: true) });
    }

    private async Task JumpToMessageAsync(UiAction action, DiscordSession active, int generation)
    {
        RequireSelectedSocialChannel(action); RequirePermission("view"); RequirePermission("history");
        string channel = currentChannel, target = Snowflake(action.Id);
        bool known = cache.Get(channel, target) != null || state.PinnedMessages.Any(m => m.Id == target && m.ChannelId == channel) ||
            state.Messages.Concat(state.PinnedMessages).Any(m => m.ChannelId == channel && m.ReplyTo == target && m.ReplyPreview?.Deleted != true && (m.ReplyPreview == null || m.ReplyPreview.ChannelId == channel));
        if (!known) throw new ServiceException("El mensaje de origen ya no está disponible en esta conversación.");
        ResetNavigation(); int version = navigationVersion;
        if (cache.Get(channel, target) == null)
        {
            var loading = new PendingChannelLoad(channel, version); pendingChannelLoad = loading;
            try
            {
                var around = await active.GetAsync($"channels/{channel}/messages?limit=50&around={target}", channelLoad.Token);
                if (!IsCurrent(active, generation) || version != navigationVersion || currentChannel != channel || pendingChannelLoad != loading) return;
                RequirePermission("history");
                var loaded = Items(around).Take(50).Select(ParseMessage).Where(m => m.ChannelId == channel).ToArray();
                var merged = MergeLoadedMessages(loaded, loading).ToArray();
                if (!merged.Any(m => m.Id == target)) throw new ServiceException("El mensaje original ya no está disponible o no se puede consultar.");
                cache.Replace(channel, merged); RememberPeople(merged.Select(m => m.Author));
                state = state with { HasOlderMessages = loaded.Length == 50 };
            }
            finally { if (pendingChannelLoad == loading) pendingChannelLoad = null; }
        }
        state = state with { View = "chat", Messages = ReadContextMessages(channel), FocusMessageId = target, FocusMessageVersion = state.FocusMessageVersion + 1 };
        UpdateSearchPeople(); Status("Mostrando el mensaje original");
    }
}
