using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private async Task<bool> HandlePresenceDispatchAsync(string type, JsonElement data)
    {
        if (type == "SELF_PRESENCE")
        {
            state = state with { SelfPresence = Str(data, "status") }; Refresh();
            var active = session; int generation = sessionGeneration;
            if (active != null && state.Connection == "En línea") await StartGameActivityAsync(active, generation);
            return true;
        }
        if (type == "PRESENCE_UPDATE")
        {
            if (ApplyPresence(data)) Refresh(); return true;
        }
        if (type == "READY_METADATA")
        {
            if (Str(data, "self_status") is { Length: > 0 } ownStatus) state = state with { SelfPresence = ownStatus };
            if (data.TryGetProperty("presences", out var values)) foreach (var presence in Items(values).Take(2000)) ApplyPresence(presence, updateView: false);
            if (data.TryGetProperty("settings", out var settings)) foreach (var setting in Items(settings).Take(500)) ApplyNotificationSettings(setting, updateView: false);
            if (data.TryGetProperty("read_states", out var readStates)) foreach (var read in Items(readStates).Take(2000))
            { RememberRead(Str(read, "channel_id"), Str(read, "last_message_id")); RememberMentionCount(Str(read, "channel_id"), Int(read, "mention_count")); }
            RefreshNotificationMetadata();
            RefreshGroupMemberView();
            SyncActivityState();
            Refresh(); return true;
        }
        if (type == "GUILD_MEMBER_LIST_UPDATE")
        {
            ApplyGuildMemberList(data); return true;
        }
        if (type == "USER_GUILD_SETTINGS_UPDATE") { ApplyNotificationSettings(data); Refresh(); return true; }
        if (type == "TYPING_START")
        {
            if (Str(data, "channel_id") != currentChannel || state.View != "chat") return true;
            string userId = Str(data, "user_id");
            if (userId.Length == 0 || userId == state.CurrentUser?.Id) return true;
            if (data.TryGetProperty("member", out var member) && member.TryGetProperty("user", out var user)) RememberPeople(new[] { ParsePerson(user) });
            if (typingPeople.Count < 32 || typingPeople.ContainsKey(userId)) typingPeople[userId] = DateTimeOffset.UtcNow.AddSeconds(8);
            UpdateTypingText();
            typingExpiry?.Cancel(); typingExpiry?.Dispose(); typingExpiry = new();
            var ct = typingExpiry.Token; int generation = sessionGeneration; string channel = currentChannel;
            try
            {
                // An expiring local indicator; this does not poll Discord or run while idle.
                _ = ExpireTypingAsync(channel, generation, ct);
            }
            catch (OperationCanceledException) { }
            return true;
        }
        if (type == "MESSAGE_ACK")
        {
            string channelId = Str(data, "channel_id");
            ClearUnread(channelId, Str(data, "message_id"));
            Refresh(); return true;
        }
        if (type == "USER_UPDATE")
        {
            var user = ParsePerson(data); RememberPeople(new[] { user });
            if (user.Id == state.CurrentUser?.Id) state = state with { CurrentUser = user };
            state = state with { DirectMessages = OrderDms(state.DirectMessages.Select(n => n.RecipientId == user.Id ? n with { Name = user.Name, IconUrl = user.AvatarUrl } : n)).ToArray() };
            Refresh(); return true;
        }
        await Task.CompletedTask;
        return false;
    }
    private async Task SubscribeVisibleGuildAsync(DiscordSession active, int generation)
    {
        if (!IsCurrent(active, generation) || state.Connection != "En línea") return;
        string guild = currentGuild, channel = currentChannel;
        try { await active.SubscribeGuildAsync(guild, guild.Length > 0 && SelectedAccess().CanViewMembers ? channel : "", memberRangeEnd); }
        catch (ServiceException) { if (IsCurrent(active, generation)) Status("La vista se abrió, pero Discord aún no confirmó su suscripción en tiempo real."); }
    }
    private bool ApplyPresence(JsonElement data, bool updateView = true, string fallbackUserId = "")
    {
        string id = data.TryGetProperty("user", out var user) ? Str(user, "id") : Str(data, "user_id");
        if (id.Length == 0) id = fallbackUserId;
        if (id.Length == 0) return false;
        if (!presences.ContainsKey(id) && presences.Count >= 2000) presences.Remove(presences.Keys.First());
        if (user.ValueKind == JsonValueKind.Object && Str(user, "username").Length > 0) RememberPeople(new[] { ParsePerson(user) });
        var details = ParsePresenceDetails(data);
        string activity = details.CustomStatus.Length > 0 ? details.CustomStatus : PresenceSummary(details.Activities);
        bool richUnchanged = richPresences.TryGetValue(id, out var rich) && rich.Status == details.Status && rich.CustomStatus == details.CustomStatus &&
            rich.CustomStatusEmoji == details.CustomStatusEmoji && rich.Activities.SequenceEqual(details.Activities);
        if (presences.TryGetValue(id, out var old) && old == (details.Status, activity) && richUnchanged) return false;
        presences[id] = (details.Status, activity);
        if (!richPresences.ContainsKey(id) && richPresences.Count >= 2000) richPresences.Remove(richPresences.Keys.First());
        richPresences[id] = details;
        state = state with { UserPresences = richPresences, PresenceVersion = state.PresenceVersion + 1 };
        if (!updateView) return false;
        // Presence changes neither activity order nor pins; only replace the matching row.
        state = state with { DirectMessages = state.DirectMessages.Select(n => n.RecipientId == id ? DecorateDm(n) : n).ToArray() };
        if (state.ProfileDetails?.User.Id == id)
            state = state with { ProfileDetails = state.ProfileDetails with { Presence = details.Status, CustomStatus = details.CustomStatus, CustomStatusEmoji = details.CustomStatusEmoji, Activities = details.Activities } };
        RefreshGroupMemberView();
        RefreshGuildMemberView();
        return true;
    }
    private void ApplyNotificationSettings(JsonElement data, bool updateView = true)
    {
        string guildId = Str(data, "guild_id");
        if (guildId.Length > 0 && (mutedGuilds.Count < 500 || mutedGuilds.ContainsKey(guildId))) mutedGuilds[guildId] = Bool(data, "muted");
        if (data.TryGetProperty("channel_overrides", out var overrides))
            foreach (var channel in Items(overrides).Take(2000))
            {
                string id = Str(channel, "channel_id");
                if (id.Length > 0 && (mutedChannels.Count < 2000 || mutedChannels.ContainsKey(id))) mutedChannels[id] = Bool(channel, "muted");
            }
        if (updateView) RefreshNotificationMetadata();
    }
    private void RefreshNotificationMetadata()
    {
        state = state with { Guilds = state.Guilds.Select(n => n with { IsMuted = mutedGuilds.GetValueOrDefault(n.Id) }).ToArray(),
            Channels = state.Channels.Select(n => n with { IsMuted = mutedChannels.GetValueOrDefault(n.Id) }).ToArray(), DirectMessages = OrderDms(state.DirectMessages).ToArray() };
        RefreshUnreadState();
    }
    private void ClearTyping()
    {
        typingExpiry?.Cancel(); typingExpiry?.Dispose(); typingExpiry = null; typingPeople.Clear();
        if (state.TypingText.Length > 0) state = state with { TypingText = "" };
    }
    private void UpdateTypingText()
    {
        var now = DateTimeOffset.UtcNow;
        foreach (var id in typingPeople.Where(p => p.Value <= now).Select(p => p.Key).ToArray()) typingPeople.Remove(id);
        var names = typingPeople.Keys.Take(3).Select(id => knownPeople.TryGetValue(id, out var person) ? person.Name : "Alguien").ToArray();
        state = state with { TypingText = names.Length == 0 ? "" : string.Join(", ", names) + (names.Length == 1 ? " está escribiendo…" : " están escribiendo…") };
        Refresh();
    }
    private async Task ExpireTypingAsync(string channel, int generation, CancellationToken ct)
    {
        try
        {
            while (!disposed && generation == sessionGeneration && channel == currentChannel && typingPeople.Count > 0)
            {
                TimeSpan remaining = typingPeople.Values.Min() - DateTimeOffset.UtcNow;
                if (remaining > TimeSpan.Zero) await Task.Delay(remaining, ct);
                ct.ThrowIfCancellationRequested();
                if (!disposed && generation == sessionGeneration && channel == currentChannel) UpdateTypingText();
            }
        }
        catch (OperationCanceledException) { }
    }
}
