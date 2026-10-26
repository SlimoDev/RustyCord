using System.Globalization;
using System.Net.Http;
using System.Text.Json;
using System.Windows;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private readonly Dictionary<string, Person> knownPeople = new(StringComparer.Ordinal);
    private readonly Dictionary<string, (string Status, string Activity)> presences = new(StringComparer.Ordinal);
    private readonly Dictionary<string, bool> mutedChannels = new(StringComparer.Ordinal), mutedGuilds = new(StringComparer.Ordinal);
    private readonly Dictionary<string, string> readMessageIds = new(StringComparer.Ordinal);
    private readonly Dictionary<string, DateTimeOffset> typingPeople = new(StringComparer.Ordinal);
    private CancellationTokenSource? typingExpiry;
    private GuildAccess? selectedGuildAccess;
    private string manualActivity = "";

    private static ulong SnowflakeOrder(string value) => ulong.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var result) ? result : 0;
    private void RememberPeople(IEnumerable<Person> people)
    {
        foreach (var person in people.Take(3000))
        {
            if (person.Id.Length == 0) continue;
            if (!knownPeople.ContainsKey(person.Id) && knownPeople.Count >= 3000) knownPeople.Remove(knownPeople.Keys.First());
            knownPeople[person.Id] = person;
        }
    }
    private void RememberRecipients(JsonElement channels)
    {
        foreach (var channel in Items(channels).Take(2000))
            RememberChannelRecipients(channel);
    }
    private NavItem DecorateDm(NavItem item)
    {
        presences.TryGetValue(item.RecipientId, out var presence);
        richPresences.TryGetValue(item.RecipientId, out var rich);
        activeCalls.TryGetValue(item.Id, out var call);
        return item with { IsPinned = features.IsPinned(item.Id), IsMuted = mutedChannels.GetValueOrDefault(item.Id),
            Presence = presence.Status ?? "", Subtitle = presence.Activity ?? "",
            CustomStatus = rich?.CustomStatus ?? "", CustomStatusEmoji = rich?.CustomStatusEmoji,
            ActivityText = rich == null ? "" : PresenceSummary(rich.Activities), Activities = rich?.Activities,
            MentionCount = mentionCounts.GetValueOrDefault(item.Id),
            HasActiveCall = call != null, IncomingCallRinging = call?.Ringing == true,
            Unread = item.Unread || unreadGuilds.ContainsKey(item.Id) || (readMessageIds.TryGetValue(item.Id, out var readId) && SnowflakeOrder(item.LastMessageId) > SnowflakeOrder(readId)) };
    }
    private static NavItem ParseGuild(JsonElement guild)
    {
        ulong bits = GuildAccess.ReadBits(guild, "permissions");
        bool administrator = Bool(guild, "owner") || (bits & 8) != 0;
        return new NavItem(Str(guild, "id"), Str(guild, "name"), "guild", "", GuildIcon(guild),
            CanInvite: administrator || (bits & 1) != 0, CanManage: administrator || (bits & 32) != 0);
    }
    private async Task<GuildAccess?> FetchGuildAccessAsync(DiscordSession active, int generation, string guildId, CancellationToken ct)
    {
        if (state.CurrentUser == null) return null;
        string userId = state.CurrentUser.Id;
        try
        {
            var guild = await active.GetAsync($"guilds/{Snowflake(guildId)}", ct);
            if (!IsCurrent(active, generation)) return null;
            var member = await active.GetAsync($"guilds/{Snowflake(guildId)}/members/{Snowflake(userId)}", ct);
            if (!IsCurrent(active, generation)) return null;
            return GuildAccess.From(guild, member, userId);
        }
        catch (ServiceException) { return null; } // Unknown access stays unavailable; never assume a role.
    }
    private NavItem DecorateChannel(JsonElement channel, GuildAccess? access)
    {
        var item = ParseChannel(channel);
        ulong bits = access?.ChannelPermissions(channel) ?? 0;
        return item with { IsMuted = mutedChannels.GetValueOrDefault(item.Id), CanInvite = (bits & 1025) == 1025,
            MentionCount = mentionCounts.GetValueOrDefault(item.Id), Unread = unreadGuilds.ContainsKey(item.Id) || (readMessageIds.TryGetValue(item.Id, out var readId) && SnowflakeOrder(item.LastMessageId) > SnowflakeOrder(readId)),
            CanManage = (bits & 1024) != 0 && (bits & 16) != 0, CanPin = (bits & 1024) != 0 && (bits & (1UL << 51)) != 0, Access = AccessFor(channel, access) };
    }
    private void ClearNavigationMetadata()
    {
        knownPeople.Clear(); presences.Clear(); mutedChannels.Clear(); mutedGuilds.Clear(); readMessageIds.Clear(); typingPeople.Clear();
        richPresences.Clear(); groupRecipients.Clear(); forwardDestinations.Clear(); ++profileVersion; ++pinsVersion; ++forwardDestinationsVersion;
        activeCalls.Clear(); privateChannelLoads.Clear(); privateChannelFailures.Clear(); notifiedMessageIds.Clear(); notifiedMessageOrder.Clear(); notifiedCalls.Clear();
        mentionCounts.Clear(); unreadGuilds.Clear(); latestObservedMessages.Clear();
        selectedChannelMetadata.Clear(); forwardAccess.Clear(); ClearGuildMemberView();
        typingExpiry?.Cancel(); typingExpiry?.Dispose(); typingExpiry = null; selectedGuildAccess = null; manualActivity = "";
    }
    private async Task<bool> TryHandleNavigationActionAsync(UiAction action, DiscordSession active, int generation)
    {
        NavItem? dm = state.DirectMessages.FirstOrDefault(n => n.Id == action.Id);
        switch (action.Kind)
        {
            case "copy-id": Clipboard.SetText(Snowflake(action.Id)); Status("ID copiado"); return true;
            case "dm-profile":
                if (dm == null || dm.RecipientId.Length == 0) throw new ServiceException("Selecciona una conversación individual para ver su perfil.");
                await LoadProfileAsync(active, generation, dm.RecipientId); return true;
            case "mark-read":
                await MarkChannelReadAsync(active, generation, Snowflake(action.Id)); return true;
            case "mute-channel":
                string mutedId = Snowflake(action.Id);
                bool mute = !mutedChannels.GetValueOrDefault(mutedId);
                if (!state.DirectMessages.Any(n => n.Id == mutedId) && !state.Channels.Any(n => n.Id == mutedId))
                    throw new ServiceException("La lista de canales cambió. Abre el canal y repite la acción.");
                string guildId = state.DirectMessages.Any(n => n.Id == mutedId) ? "@me" : currentGuild;
                if (guildId.Length == 0) throw new ServiceException("Abre el servidor de ese canal antes de cambiar sus notificaciones.");
                await active.SendAsync(HttpMethod.Patch, $"users/@me/guilds/{guildId}/settings", new { channel_overrides = new Dictionary<string, object> { [mutedId] = new { muted = mute } } }, sessionWork.Token);
                if (!IsCurrent(active, generation)) return true;
                if (mutedChannels.Count < 2000 || mutedChannels.ContainsKey(mutedId)) mutedChannels[mutedId] = mute;
                state = state with { DirectMessages = OrderDms(state.DirectMessages).ToArray(), Channels = state.Channels.Select(n => n.Id == mutedId ? n with { IsMuted = mute } : n).ToArray() };
                Status(mute ? "Notificaciones del canal silenciadas" : "Notificaciones del canal activadas"); return true;
            case "close-dm":
                if (dm?.Kind != "dm") throw new ServiceException("Esta acción cierra conversaciones individuales. No abandona grupos.");
                await active.SendAsync(HttpMethod.Delete, $"channels/{Snowflake(dm.Id)}", ct: sessionWork.Token);
                if (!IsCurrent(active, generation)) return true;
                state = state with { DirectMessages = state.DirectMessages.Where(n => n.Id != dm.Id).ToArray() };
                if (currentChannel == dm.Id) { ResetNavigation(); currentChannel = ""; state = state with { SelectedChannelId = "", Messages = Array.Empty<ChatMessage>(), View = "friends" }; }
                Status("Conversación cerrada; su historial permanece en Discord"); return true;
            case "block-user":
                string blocked = Snowflake(action.Id);
                if (blocked == state.CurrentUser?.Id) throw new ServiceException("No puedes bloquear tu propia cuenta.");
                await active.SendAsync(HttpMethod.Put, $"users/@me/relationships/{blocked}", new { type = 2 }, sessionWork.Token);
                if (!IsCurrent(active, generation)) return true;
                await RefreshRelationshipsAsync(); if (IsCurrent(active, generation)) Status("Usuario bloqueado"); return true;
            case "guild-mute":
                string muteGuildId = Snowflake(action.Id); bool guildMute = !mutedGuilds.GetValueOrDefault(muteGuildId);
                await active.SendAsync(HttpMethod.Patch, $"users/@me/guilds/{muteGuildId}/settings", new { muted = guildMute }, sessionWork.Token);
                if (!IsCurrent(active, generation)) return true;
                if (mutedGuilds.Count < 500 || mutedGuilds.ContainsKey(muteGuildId)) mutedGuilds[muteGuildId] = guildMute;
                state = state with { Guilds = state.Guilds.Select(n => n.Id == muteGuildId ? n with { IsMuted = guildMute } : n).ToArray() };
                Status(guildMute ? "Notificaciones del servidor silenciadas" : "Notificaciones del servidor activadas"); return true;
            case "guild-leave":
                string leaveId = Snowflake(action.Id);
                await active.SendAsync(HttpMethod.Delete, $"users/@me/guilds/{leaveId}", ct: sessionWork.Token);
                if (!IsCurrent(active, generation)) return true;
                state = state with { Guilds = state.Guilds.Where(n => n.Id != leaveId).ToArray() };
                if (currentGuild == leaveId) { ResetNavigation(); currentGuild = currentChannel = ""; selectedGuildAccess = null; state = state with { SelectedGuildId = "", SelectedChannelId = "", Channels = Array.Empty<NavItem>(), Messages = Array.Empty<ChatMessage>(), View = "friends" }; }
                Status("Has salido del servidor"); return true;
            case "guild-settings":
                Status("El editor de roles y ajustes del servidor todavía no está implementado. No se ha cambiado ningún ajuste."); return true;
            case "guild-invite":
                await CreateGuildInviteAsync(active, generation, Snowflake(action.Id)); return true;
            case "guild-mark-read":
                await MarkGuildReadAsync(active, generation, Snowflake(action.Id)); return true;
            case "configure-game-activity": case "toggle-game-activity": case "custom-activity":
                await HandleGameActivityActionAsync(action, active, generation); return true;
        }
        return false;
    }

    private async Task MarkChannelReadAsync(DiscordSession active, int generation, string channelId)
    {
        var item = state.DirectMessages.Concat(state.Channels).FirstOrDefault(n => n.Id == channelId);
        string last = item?.LastMessageId ?? "";
        if (last.Length == 0)
        {
            var messages = await active.GetAsync($"channels/{channelId}/messages?limit=1", sessionWork.Token);
            if (!IsCurrent(active, generation)) return;
            last = Items(messages).Select(m => Str(m, "id")).FirstOrDefault() ?? "";
        }
        if (last.Length == 0) { Status("El canal no tiene mensajes para marcar."); return; }
        await active.SendAsync(HttpMethod.Post, $"channels/{channelId}/messages/{Snowflake(last)}/ack", new { token = (string?)null, manual = true }, sessionWork.Token);
        if (!IsCurrent(active, generation)) return;
        ClearUnread(channelId, last);
        Status("Lectura del canal actualizada");
    }
    private async Task MarkGuildReadAsync(DiscordSession active, int generation, string guildId)
    {
        var access = await FetchGuildAccessAsync(active, generation, guildId, sessionWork.Token);
        if (!IsCurrent(active, generation)) return;
        if (access == null) throw new ServiceException("No se pudieron comprobar tus canales accesibles. No se ha marcado ningún canal como leído.");
        var channels = await active.GetAsync($"guilds/{guildId}/channels", sessionWork.Token);
        if (!IsCurrent(active, generation)) return;
        var targets = Items(channels).Where(c => Str(c, "last_message_id").Length > 0 && (access.ChannelPermissions(c) & 1024) != 0).Take(2000)
            .Select(c => new { channel_id = Str(c, "id"), message_id = Str(c, "last_message_id"), read_state_type = 0 }).ToArray();
        if (targets.Length == 0) { Status("No hay mensajes de canales disponibles para marcar."); return; }
        foreach (var chunk in targets.Chunk(100))
        {
            await active.SendAsync(HttpMethod.Post, "read-states/ack-bulk", new { read_states = chunk }, sessionWork.Token);
            if (!IsCurrent(active, generation)) return;
            foreach (var target in chunk) ClearUnread(target.channel_id, target.message_id);
        }
        if (currentGuild == guildId) state = state with { Channels = state.Channels.Select(n => n with { Unread = false }).ToArray() };
        state = state with { Guilds = state.Guilds.Select(n => n.Id == guildId ? n with { Unread = false } : n).ToArray() };
        Status("Canales del servidor marcados como leídos");
    }
    private void RememberRead(string channelId, string messageId)
    {
        if (channelId.Length == 0 || messageId.Length == 0) return;
        if (readMessageIds.TryGetValue(channelId, out var previous) && SnowflakeOrder(previous) >= SnowflakeOrder(messageId)) return;
        if (!readMessageIds.ContainsKey(channelId) && readMessageIds.Count >= 2000) readMessageIds.Remove(readMessageIds.Keys.First());
        readMessageIds[channelId] = messageId;
    }
    private async Task CreateGuildInviteAsync(DiscordSession active, int generation, string guildId)
    {
        var access = await FetchGuildAccessAsync(active, generation, guildId, sessionWork.Token);
        if (!IsCurrent(active, generation)) return;
        if (access == null) throw new ServiceException("No se pudieron comprobar tus permisos. No se ha creado ninguna invitación.");
        var data = await active.GetAsync($"guilds/{guildId}/channels", sessionWork.Token);
        if (!IsCurrent(active, generation)) return;
        var options = Items(data).Select(c => DecorateChannel(c, access)).Where(c => c.CanInvite && c.Kind is "text" or "voice").Take(500).ToArray();
        if (options.Length == 0) throw new ServiceException("No tienes permiso para crear una invitación en los canales disponibles.");
        string? chosen = Choose("Canal de destino de la invitación", options.Select(c => (c.Id, c.Name)).ToArray());
        if (chosen == null || !IsCurrent(active, generation)) return;
        var invite = await active.SendAsync(HttpMethod.Post, $"channels/{Snowflake(chosen)}/invites", new { max_age = 86400, max_uses = 0, temporary = false, unique = false }, sessionWork.Token);
        if (!IsCurrent(active, generation)) return;
        Clipboard.SetText("https://discord.gg/" + Str(invite, "code")); Status("Invitación copiada · caduca en 24 horas");
    }
}
