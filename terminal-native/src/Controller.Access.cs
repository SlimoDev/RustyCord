using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private readonly Dictionary<string, JsonElement> selectedChannelMetadata = new(StringComparer.Ordinal);
    private readonly Dictionary<string, (string Guild, GuildAccess Access, JsonElement Channel, JsonElement Parent)> forwardAccess = new(StringComparer.Ordinal);
    private ChannelAccess AccessFor(JsonElement channel, GuildAccess? access, IReadOnlyDictionary<string, JsonElement>? catalog = null)
    {
        if (Int(channel, "type") is 1 or 3) return ChannelAccess.Private;
        if (access == null) return ChannelAccess.Unknown;
        JsonElement parent = default;
        (catalog ?? selectedChannelMetadata).TryGetValue(Str(channel, "parent_id"), out parent);
        return access.Evaluate(channel, parent);
    }
    private void RememberSelectedChannelMetadata(JsonElement channels)
    {
        selectedChannelMetadata.Clear();
        foreach (var channel in Items(channels).Take(2000))
            if (Str(channel, "id") is { Length: > 0 } id) selectedChannelMetadata[id] = channel.Clone();
    }
    private ChannelAccess SelectedAccess()
    {
        if (currentChannel.Length == 0) return ChannelAccess.Unknown;
        var item = state.DirectMessages.FirstOrDefault(n => n.Id == currentChannel);
        if (item?.Kind is "dm" or "group") return ChannelAccess.Private;
        if (currentGuild.Length == 0) return state.SelectedChannelId == currentChannel ? state.ChannelAccess : ChannelAccess.Unknown;
        return selectedChannelMetadata.TryGetValue(currentChannel, out var channel) ? AccessFor(channel, selectedGuildAccess) : ChannelAccess.Unknown;
    }
    private void ApplySelectedAccess()
    {
        var permission = SelectedAccess();
        state = state with { ChannelAccess = permission, CanPinMessages = permission.CanView && (currentGuild.Length == 0 || state.Channels.FirstOrDefault(n => n.Id == currentChannel)?.CanPin == true), GuildRoles = currentGuild.Length > 0 ? selectedGuildAccess?.Roles ?? Array.Empty<GuildRole>() : Array.Empty<GuildRole>() };
        UpdateSearchPeople();
    }
    private void RequirePermission(string capability)
    {
        var permission = SelectedAccess();
        bool allowed = capability switch { "view" => permission.CanView, "history" => permission.CanReadHistory, "send" => permission.CanSend, "attach" => permission.CanAttach, "members" => permission.CanViewMembers, "react" => permission.CanReact, _ => false };
        if (!permission.Known || !allowed) throw new ServiceException(!permission.Known ? permission.Reason : capability switch {
            "attach" => "No tienes permiso para adjuntar archivos en este canal.", "react" => "No tienes permiso para añadir reacciones nuevas en este canal.",
            "history" => "No tienes permiso para consultar el historial de este canal.", "members" => "No tienes permiso para consultar miembros de este canal.",
            _ => permission.Reason.Length > 0 ? permission.Reason : "No tienes permiso para realizar esta acción en el canal." });
    }
    private async Task<bool> HandleAccessDispatchAsync(string type, JsonElement data)
    {
        if (type == "CHANNEL_DELETE") { string id = Str(data, "id"); selectedChannelMetadata.Remove(id); forwardAccess.Remove(id); forwardDestinations.Remove(id); if (id == currentChannel) ClearGuildMemberView(); return false; }
        if (type is "CHANNEL_UPDATE" or "CHANNEL_CREATE")
        {
            if (Str(data, "guild_id") != currentGuild || currentGuild.Length == 0) return false;
            string id = Str(data, "id"); if (id.Length == 0) return true;
            bool existing = selectedChannelMetadata.TryGetValue(id, out var previous);
            bool aclChanged = data.TryGetProperty("permission_overwrites", out var overwrites) && (!existing || !previous.TryGetProperty("permission_overwrites", out var priorOverwrites) || overwrites.GetRawText() != priorOverwrites.GetRawText());
            var fields = existing ? previous.EnumerateObject().ToDictionary(p => p.Name, p => p.Value.Clone(), StringComparer.Ordinal) : new Dictionary<string, JsonElement>(StringComparer.Ordinal);
            foreach (var property in data.EnumerateObject()) fields[property.Name] = property.Value.Clone();
            selectedChannelMetadata[id] = JsonSerializer.SerializeToElement(fields);
            if (selectedChannelMetadata.Count > 2000) selectedChannelMetadata.Remove(selectedChannelMetadata.Keys.First());
            bool selectedOrParent = id == currentChannel || selectedChannelMetadata.TryGetValue(currentChannel, out var selected) && Str(selected, "parent_id") == id;
            if (selectedOrParent && (aclChanged || data.TryGetProperty("member_list_id", out _))) ClearGuildMemberView();
            RebuildSelectedChannels();
            if (selectedOrParent && session is { } active) await SubscribeVisibleGuildAsync(active, sessionGeneration);
            return true;
        }
        if (type is "GUILD_ROLE_CREATE" or "GUILD_ROLE_UPDATE" or "GUILD_ROLE_DELETE" or "GUILD_MEMBER_UPDATE")
        {
            if (Str(data, "guild_id") != currentGuild || currentGuild.Length == 0) return true;
            if (type == "GUILD_MEMBER_UPDATE" && data.TryGetProperty("user", out var user) && Str(user, "id") != state.CurrentUser?.Id) { UpdateKnownGuildMember(data); return true; }
            ClearGuildMemberView();
            var active = session; int generation = sessionGeneration; string guild = currentGuild;
            if (active != null) { var access = await FetchGuildAccessAsync(active, generation, guild, sessionWork.Token); if (IsCurrent(active, generation) && guild == currentGuild) { selectedGuildAccess = access; RebuildSelectedChannels(); await SubscribeVisibleGuildAsync(active, generation); } }
            return true;
        }
        return false;
    }
    private void RebuildSelectedChannels()
    {
        state = state with { Channels = selectedChannelMetadata.Values.Where(c => AccessFor(c, selectedGuildAccess).CanView).OrderBy(c => Int(c, "position")).Select(c => DecorateChannel(c, selectedGuildAccess)).ToArray() };
        ApplySelectedAccess();
        if ((!state.ChannelAccess.CanView || !state.ChannelAccess.CanReadHistory) && currentChannel.Length > 0)
        { cache.Replace(currentChannel, Array.Empty<ChatMessage>()); state = state with { Messages = Array.Empty<ChatMessage>(), Status = state.ChannelAccess.Reason }; }
        if (!state.ChannelAccess.CanView)
        {
            ClearGuildMemberView();
            state = state with { SearchPeople = Array.Empty<Person>(), ProfileDetails = state.ProfileDetails == null ? null : state.ProfileDetails with { GuildId = "", GuildName = "", GuildRoles = null, Nickname = "" } };
        }
        Refresh();
    }
}
