using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private readonly SortedDictionary<int, JsonElement> memberListEntries = new();
    private string memberListChannel = "", memberListId = "";
    private int memberRangeEnd = 99, memberTotal;

    private void ClearGuildMemberView()
    {
        memberListEntries.Clear(); memberListChannel = ""; memberListId = ""; memberRangeEnd = 99; memberTotal = 0;
        state = state with { GuildMembers = Array.Empty<ChannelMember>(), GuildMemberGroups = Array.Empty<GuildMemberGroup>(), GuildMembersTotal = 0, GuildMembersHasMore = false, GuildMembersPartial = false };
    }
    private void UpdateSearchPeople()
    {
        IEnumerable<Person> people;
        if (currentChannel.Length == 0 || !SelectedAccess().CanView) people = Array.Empty<Person>();
        else if (currentGuild.Length > 0)
            people = state.GuildMembers.Select(m => m.User).Concat(state.Messages.Where(m => m.ChannelId == currentChannel).Select(m => m.Author));
        else if (state.SelectedChannelKind == "group") people = state.GroupMembers.Select(m => m.User);
        else
        {
            var dm = state.DirectMessages.FirstOrDefault(c => c.Id == currentChannel);
            people = dm == null ? Array.Empty<Person>() : new[] { knownPeople.GetValueOrDefault(dm.RecipientId), state.CurrentUser }.Where(p => p != null).Cast<Person>();
        }
        if (currentChannel.Length > 0 && SelectedAccess().CanView && state.CurrentUser != null) people = people.Append(state.CurrentUser);
        state = state with { SearchPeople = people.Where(p => p.Id.Length > 0).DistinctBy(p => p.Id).Take(2000).ToArray() };
    }
    private ChannelMember ParseGuildMember(JsonElement member)
    {
        var person = member.TryGetProperty("user", out var user) ? ParsePerson(user) : knownPeople.GetValueOrDefault(Str(member, "user_id")) ?? new Person(Str(member, "user_id"), "Usuario");
        if (person.Id.Length > 0) RememberPeople(new[] { person });
        var ids = member.TryGetProperty("roles", out var values) ? Items(values).Where(v => v.ValueKind == JsonValueKind.String).Select(v => v.GetString()!).ToHashSet(StringComparer.Ordinal) : new HashSet<string>(StringComparer.Ordinal);
        var roles = (selectedGuildAccess?.Roles ?? Array.Empty<GuildRole>()).Where(r => ids.Contains(r.Id)).OrderByDescending(r => r.Position).ThenBy(r => r.Id, StringComparer.Ordinal).ToArray();
        richPresences.TryGetValue(person.Id, out var presence);
        string nickname = Str(member, "nick");
        if (nickname.Length > 0) person = person with { Name = nickname };
        return new(person, person.Id == state.CurrentUser?.Id ? state.SelfPresence : presence?.Status ?? "", presence?.CustomStatus ?? "", presence?.CustomStatusEmoji, presence?.Activities ?? Array.Empty<PresenceActivity>(),
            Roles: roles, Nickname: nickname, RoleColor: roles.FirstOrDefault(r => r.Color != 0)?.Color ?? 0, HoistRoleId: roles.FirstOrDefault(r => r.Hoist)?.Id ?? "");
    }
    private void UpdateKnownGuildMember(JsonElement member)
    {
        if (currentGuild.Length == 0 || !SelectedAccess().CanViewMembers) return;
        string id = member.TryGetProperty("user", out var user) ? Str(user, "id") : Str(member, "user_id");
        foreach (var key in memberListEntries.Where(e => e.Value.TryGetProperty("member", out var value) && value.TryGetProperty("user", out var u) && Str(u, "id") == id).Select(e => e.Key).ToArray())
            memberListEntries[key] = JsonSerializer.SerializeToElement(new { member = member.Clone() });
        RefreshGuildMemberView();
    }
    private void RefreshGuildMemberView()
    {
        if (currentGuild.Length == 0 || currentChannel.Length == 0 || !SelectedAccess().CanViewMembers) return;
        var members = memberListEntries.Values.Where(e => e.ValueKind == JsonValueKind.Object && e.TryGetProperty("member", out var member) && member.ValueKind == JsonValueKind.Object)
            .Select(e => ParseGuildMember(e.GetProperty("member"))).Where(m => m.User.Id.Length > 0).DistinctBy(m => m.User.Id).Take(2000).ToArray();
        state = state with { GuildMembers = members, GuildRoles = selectedGuildAccess?.Roles ?? Array.Empty<GuildRole>(), GuildMembersTotal = memberTotal,
            GuildMembersHasMore = memberRangeEnd < 1999 && (memberTotal == 0 ? memberListEntries.Count >= memberRangeEnd : members.Length < memberTotal), GuildMembersPartial = memberTotal > members.Length };
        UpdateSearchPeople(); Refresh();
    }
    private void ApplyGuildMemberList(JsonElement data)
    {
        if (currentGuild.Length == 0 || currentChannel.Length == 0 || Str(data, "guild_id") != currentGuild || !SelectedAccess().CanViewMembers) return;
        string packetChannel = Str(data, "channel_id");
        if (packetChannel.Length > 0 && packetChannel != currentChannel) return;
        string list = Str(data, "id");
        string expected = selectedChannelMetadata.TryGetValue(currentChannel, out var channel) ? selectedGuildAccess?.ConfirmedMemberListId(channel) ?? "" : "";
        if (packetChannel.Length == 0 && (expected.Length == 0 || list != expected)) return;
        if (memberListChannel != currentChannel) { ClearGuildMemberView(); memberListChannel = currentChannel; }
        if (memberListId.Length == 0) memberListId = list;
        if (list.Length > 0 && memberListId.Length > 0 && list != memberListId) return;
        if (data.TryGetProperty("member_count", out var count) && count.ValueKind == JsonValueKind.Number) memberTotal = Math.Clamp(Int(data, "member_count"), 0, 10000000);
        if (data.TryGetProperty("groups", out var groups)) state = state with { GuildMemberGroups = Items(groups).Take(1000).Select(group => {
            string id = Str(group, "id"); var role = selectedGuildAccess?.Roles.FirstOrDefault(r => r.Id == id);
            return new GuildMemberGroup(id, role?.Name ?? (id == "online" ? "En línea" : id == "offline" ? "Desconectados" : "Miembros"), Int(group, "count"), role?.Color ?? 0);
        }).ToArray() };
        if (data.TryGetProperty("ops", out var operations)) foreach (var operation in Items(operations).Take(100))
        {
            string op = Str(operation, "op"); int index = Int(operation, "index");
            if (op is "SYNC" or "INVALIDATE")
            {
                var range = operation.TryGetProperty("range", out var r) ? Items(r).Take(2).ToArray() : Array.Empty<JsonElement>();
                if (range.Length != 2 || !range[0].TryGetInt32(out int start) || !range[1].TryGetInt32(out int end) || start < 0 || end < start || start > 1999) continue;
                end = Math.Min(end, 1999); foreach (int key in memberListEntries.Keys.Where(k => k >= start && k <= end).ToArray()) memberListEntries.Remove(key);
                if (op == "SYNC" && operation.TryGetProperty("items", out var items)) foreach (var entry in Items(items).Take(end - start + 1)) memberListEntries[start++] = entry.Clone();
            }
            else if (index is >= 0 and <= 1999 && op == "DELETE")
            { memberListEntries.Remove(index); foreach (int key in memberListEntries.Keys.Where(k => k > index).ToArray()) { memberListEntries[key - 1] = memberListEntries[key]; memberListEntries.Remove(key); } }
            else if (index is >= 0 and <= 1999 && op is "UPDATE" or "INSERT" && operation.TryGetProperty("item", out var entry))
            {
                if (op == "INSERT") foreach (int key in memberListEntries.Keys.Where(k => k >= index).OrderDescending().ToArray()) { if (key < 1999) memberListEntries[key + 1] = memberListEntries[key]; memberListEntries.Remove(key); }
                memberListEntries[index] = entry.Clone();
            }
        }
        foreach (var entry in memberListEntries.Values)
            if (entry.ValueKind == JsonValueKind.Object && entry.TryGetProperty("member", out var member) && member.ValueKind == JsonValueKind.Object && member.TryGetProperty("presence", out var presence))
                ApplyPresence(presence, updateView: false, fallbackUserId: member.TryGetProperty("user", out var user) ? Str(user, "id") : "");
        RefreshGuildMemberView();
    }
    private async Task LoadMoreMembersAsync()
    {
        RequirePermission("members"); var active = session; if (active == null || currentGuild.Length == 0 || currentChannel.Length == 0 || memberRangeEnd >= 1999) return;
        memberRangeEnd = Math.Min(1999, memberRangeEnd + 100);
        await active.SubscribeGuildAsync(currentGuild, currentChannel, memberRangeEnd);
        state = state with { Status = "Solicitando más miembros visibles del canal…" }; Refresh();
    }
}
