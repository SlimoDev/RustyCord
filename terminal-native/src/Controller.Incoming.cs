using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private readonly Dictionary<string, IncomingCall> activeCalls = new(StringComparer.Ordinal);
    private readonly Dictionary<string, Task> privateChannelLoads = new(StringComparer.Ordinal);
    private readonly Dictionary<string, DateTimeOffset> privateChannelFailures = new(StringComparer.Ordinal);
    private readonly HashSet<string> notifiedMessageIds = new(StringComparer.Ordinal), notifiedCalls = new(StringComparer.Ordinal);
    private readonly Queue<string> notifiedMessageOrder = new();

    private Task EnsureIncomingPrivateChannelAsync(string channelId, DiscordSession active, int generation)
    {
        if (!IsCurrent(active, generation) || state.DirectMessages.Any(n => n.Id == channelId) || channelId.Length is < 1 or > 24 || !channelId.All(char.IsAsciiDigit)) return Task.CompletedTask;
        if (privateChannelLoads.TryGetValue(channelId, out var pending)) return pending;
        if (privateChannelLoads.Count >= 8 || privateChannelFailures.TryGetValue(channelId, out var failedUntil) && failedUntil > DateTimeOffset.UtcNow) return Task.CompletedTask;
        var work = HydrateIncomingPrivateChannelAsync(channelId, active, generation);
        privateChannelLoads[channelId] = work; return work;
    }

    private async Task HydrateIncomingPrivateChannelAsync(string channelId, DiscordSession active, int generation)
    {
        // Yield once so the bounded in-flight entry exists even with a completed fake HTTP response.
        await Task.Yield();
        try
        {
            var channel = await active.GetAsync($"channels/{channelId}", sessionWork.Token);
            if (!IsCurrent(active, generation) || Int(channel, "type") is not (1 or 3) || Str(channel, "id") != channelId) return;
            RememberChannelRecipients(channel);
            var item = ParseChannel(channel);
            string latest = latestObservedMessages.GetValueOrDefault(channelId) ?? "";
            if (SnowflakeOrder(latest) > SnowflakeOrder(item.LastMessageId)) item = item with { LastMessageId = latest };
            state = state with { DirectMessages = OrderDms(state.DirectMessages.Where(n => n.Id != channelId).Append(item)).Take(2000).ToArray() };
            if (activeCalls.TryGetValue(channelId, out var call))
            {
                activeCalls[channelId] = call with { ChannelName = item.Name, Kind = item.Kind, IconUrl = item.IconUrl, Caller = call.Caller ?? (item.Kind == "dm" ? knownPeople.GetValueOrDefault(item.RecipientId) : null) };
                RefreshIncomingCalls();
            }
            RefreshUnreadState(); Refresh();
        }
        catch (ServiceException)
        {
            if (IsCurrent(active, generation))
            {
                if (privateChannelFailures.Count >= 128 && !privateChannelFailures.ContainsKey(channelId)) privateChannelFailures.Remove(privateChannelFailures.Keys.First());
                privateChannelFailures[channelId] = DateTimeOffset.UtcNow.AddSeconds(30);
            }
        }
        finally { if (IsCurrent(active, generation)) privateChannelLoads.Remove(channelId); }
    }

    private bool RememberNotificationMessage(string messageId)
    {
        if (messageId.Length == 0 || !notifiedMessageIds.Add(messageId)) return false;
        notifiedMessageOrder.Enqueue(messageId);
        while (notifiedMessageOrder.Count > 1024) notifiedMessageIds.Remove(notifiedMessageOrder.Dequeue());
        return true;
    }

    private void OfferDesktopNotice(DesktopNotice notice)
    {
        // This reducer is invoked by the UI Dispatcher; notification display policy belongs to the window.
        var handlers = DesktopNoticeRequested;
        if (handlers == null) return;
        foreach (Action<DesktopNotice> handler in handlers.GetInvocationList())
            try { handler(notice); }
            catch (Exception error) when (error is InvalidOperationException or System.Runtime.InteropServices.COMException or System.ComponentModel.Win32Exception) { }
    }

    private void ObserveIncomingCallDispatch(string type, JsonElement data, DiscordSession active, int generation)
    {
        if (type is not ("CALL_CREATE" or "CALL_UPDATE" or "CALL_DELETE")) return;
        string channel = Str(data, "channel_id");
        if (channel.Length == 0 || channel.Length > 24 || !channel.All(char.IsAsciiDigit)) return;
        if (type == "CALL_DELETE")
        {
            activeCalls.Remove(channel); notifiedCalls.Remove(channel); RefreshIncomingCalls(); return;
        }
        activeCalls.TryGetValue(channel, out var previous);
        var item = state.DirectMessages.FirstOrDefault(n => n.Id == channel);
        var people = data.TryGetProperty("voice_states", out var states) && states.ValueKind == JsonValueKind.Array
            ? Items(states).Take(25).Select(s => Str(s, "user_id")).Where(id => id.Length > 0).Distinct().Select(id => knownPeople.GetValueOrDefault(id) ?? new Person(id, id == state.CurrentUser?.Id ? state.CurrentUser.Name : "Participante")).ToArray()
            : previous?.Participants ?? Array.Empty<Person>();
        bool selfJoined = people.Any(p => p.Id == state.CurrentUser?.Id) || voiceChannel == channel;
        bool ringing = data.TryGetProperty("ringing", out var ringingValues) && ringingValues.ValueKind == JsonValueKind.Array
            ? Items(ringingValues).Take(25).Any(id => id.ValueKind == JsonValueKind.String && id.GetString() == state.CurrentUser?.Id)
            : previous?.Ringing == true;
        ringing &= !selfJoined;
        string messageId = Str(data, "message_id");
        Person? caller = previous?.Caller ?? (messageId.Length > 0 ? cache.Get(channel, messageId)?.Author : null) ?? (item?.Kind == "dm" ? knownPeople.GetValueOrDefault(item.RecipientId) : null);
        var started = previous?.StartedAt ?? AccountCreatedAt(messageId) ?? DateTimeOffset.UtcNow;
        var call = new IncomingCall(channel, item?.Name ?? previous?.ChannelName ?? "Llamada entrante", item?.Kind ?? previous?.Kind ?? "dm", item?.IconUrl ?? previous?.IconUrl, caller,
            started, people, ringing, !selfJoined && previous?.Unread != false);
        if (!activeCalls.ContainsKey(channel) && activeCalls.Count >= 32)
        { string oldest = activeCalls.OrderBy(p => p.Value.StartedAt).First().Key; activeCalls.Remove(oldest); notifiedCalls.Remove(oldest); }
        activeCalls[channel] = call;
        if (messageId.Length > 0) RememberLatestMessage(channel, messageId);
        RefreshIncomingCalls();
        if (ringing && !mutedChannels.GetValueOrDefault(channel) && notifiedCalls.Add(channel))
            OfferDesktopNotice(new(channel, caller?.Name is { Length: > 0 } callerName && call.Kind == "group" ? callerName + " · " + call.ChannelName : caller?.Name ?? call.ChannelName, "Llamada entrante" + (call.Kind == "group" ? " de grupo" : ""), true, caller?.AvatarUrl ?? call.IconUrl));
        if (item == null) _ = GuardAsync(() => EnsureIncomingPrivateChannelAsync(channel, active, generation));
    }

    private void RefreshIncomingCalls()
    {
        state = state with { IncomingCalls = activeCalls.Values.OrderByDescending(c => c.Ringing).ThenByDescending(c => c.StartedAt).Take(32).ToArray(),
            DirectMessages = OrderDms(state.DirectMessages).ToArray() };
        RefreshUnreadState(); Refresh();
    }

    private void DismissIncomingCall(string channel)
    {
        if (!activeCalls.TryGetValue(channel, out var call)) return;
        activeCalls[channel] = call with { Unread = false }; RefreshIncomingCalls();
    }

    private async Task OpenNotificationChannelAsync(string channelId, DiscordSession active, int generation)
    {
        if (state.DirectMessages.Any(n => n.Id == channelId) || state.Channels.Any(n => n.Id == channelId) && currentGuild.Length > 0)
        { await SelectChannelAsync(channelId, connectVoice: false); return; }
        ResetNavigation(); int version = navigationVersion;
        var channel = await active.GetAsync($"channels/{channelId}", channelLoad.Token);
        if (!IsCurrent(active, generation) || version != navigationVersion || Str(channel, "id") != channelId) return;
        if (Int(channel, "type") is 1 or 3)
        {
            RememberChannelRecipients(channel); var item = ParseChannel(channel);
            string latest = latestObservedMessages.GetValueOrDefault(channelId) ?? "";
            if (SnowflakeOrder(latest) > SnowflakeOrder(item.LastMessageId)) item = item with { LastMessageId = latest };
            state = state with { DirectMessages = OrderDms(state.DirectMessages.Where(n => n.Id != channelId).Append(item)).Take(2000).ToArray() };
            await SelectChannelAsync(channelId, connectVoice: false); return;
        }
        string guildId = Str(channel, "guild_id");
        if (guildId.Length == 0 || !state.Guilds.Any(g => g.Id == guildId)) throw new ServiceException("El servidor de esta notificación ya no está disponible.");
        int expectedGuildVersion = navigationVersion + 1;
        await SelectGuildAsync(guildId);
        if (!IsCurrent(active, generation) || navigationVersion != expectedGuildVersion || currentGuild != guildId) return;
        if (!state.Channels.Any(c => c.Id == channelId))
        {
            selectedChannelMetadata[channelId] = channel.Clone();
            state = state with { Channels = state.Channels.Append(DecorateChannel(channel, selectedGuildAccess)).Take(2000).ToArray() };
        }
        await SelectChannelAsync(channelId, connectVoice: false);
    }
}
