using System.Globalization;
using System.Text.Json;

namespace NotDiscord.Terminal;

/// <summary>Discord's documented owner/role/overwrite order; missing data grants nothing.</summary>
public sealed class GuildAccess
{
    public string GuildId { get; }
    public string UserId { get; }
    public ulong BasePermissions { get; }
    public IReadOnlyList<GuildRole> Roles { get; private init; } = Array.Empty<GuildRole>();
    public bool TimedOut { get; private init; }
    private bool EveryoneViews { get; init; }
    private readonly HashSet<string> roleIds;
    private GuildAccess(string guild, string user, ulong permissions, HashSet<string> roles)
    { GuildId = guild; UserId = user; BasePermissions = permissions; roleIds = roles; }
    public static ulong ReadBits(JsonElement item, string key) => item.ValueKind == JsonValueKind.Object && item.TryGetProperty(key, out var value)
        && value.ValueKind == JsonValueKind.String && ulong.TryParse(value.GetString(), NumberStyles.None, CultureInfo.InvariantCulture, out var bits) ? bits : 0;
    public static GuildAccess From(JsonElement guild, JsonElement member, string userId)
    {
        string guildId = Controller.Str(guild, "id");
        if (guildId.Length == 0 || userId.Length == 0) return new(guildId, userId, 0, new HashSet<string>(StringComparer.Ordinal));
        var roles = new HashSet<string>(member.ValueKind == JsonValueKind.Object && member.TryGetProperty("roles", out var memberRoles)
            ? Controller.Items(memberRoles).Where(r => r.ValueKind == JsonValueKind.String).Select(r => r.GetString()!).Take(500) : Array.Empty<string>(), StringComparer.Ordinal);
        ulong permissions = 0;
        if (userId.Length > 0 && Controller.Str(guild, "owner_id") == userId) permissions = ulong.MaxValue;
        else if (guild.ValueKind == JsonValueKind.Object && guild.TryGetProperty("roles", out var guildRoles))
            foreach (var role in Controller.Items(guildRoles).Take(1000))
                if (Controller.Str(role, "id") == guildId || roles.Contains(Controller.Str(role, "id"))) permissions |= ReadBits(role, "permissions");
        if ((permissions & 8) != 0) permissions = ulong.MaxValue;
        var catalog = guild.TryGetProperty("roles", out var catalogRoles) ? Controller.Items(catalogRoles).Take(1000).Select(r => new GuildRole(Controller.Str(r, "id"), Controller.Str(r, "name"), Controller.Int(r, "position"), Controller.Int(r, "color"), Controller.Bool(r, "hoist"))).OrderByDescending(r => r.Position).ThenBy(r => r.Id, StringComparer.Ordinal).ToArray() : Array.Empty<GuildRole>();
        bool timeout = DateTimeOffset.TryParse(Controller.Str(member, "communication_disabled_until"), CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var until) && until > DateTimeOffset.UtcNow && permissions != ulong.MaxValue;
        bool everyoneViews = guild.TryGetProperty("roles", out var allRoles) && Controller.Items(allRoles).Any(r => Controller.Str(r, "id") == guildId && (ReadBits(r, "permissions") & 1024) != 0);
        return new(guildId, userId, permissions, roles) { Roles = catalog, TimedOut = timeout, EveryoneViews = everyoneViews };
    }
    public ulong ChannelPermissions(JsonElement channel)
    {
        if (GuildId.Length == 0 || UserId.Length == 0) return 0;
        if (BasePermissions == ulong.MaxValue) return BasePermissions;
        ulong permissions = BasePermissions, roleAllow = 0, roleDeny = 0, ownAllow = 0, ownDeny = 0;
        if (channel.ValueKind != JsonValueKind.Object || !channel.TryGetProperty("permission_overwrites", out var values)) return TimedOut ? permissions & ((1UL << 10) | (1UL << 16)) : permissions;
        foreach (var overwrite in Controller.Items(values).Take(1000))
        {
            string id = Controller.Str(overwrite, "id");
            if (overwrite.ValueKind != JsonValueKind.Object || !overwrite.TryGetProperty("type", out var kind) || kind.ValueKind != JsonValueKind.Number || !kind.TryGetInt32(out int type) || type is not (0 or 1)) continue;
            ulong allow = ReadBits(overwrite, "allow"), deny = ReadBits(overwrite, "deny");
            if (type == 0 && id == GuildId) permissions = (permissions & ~deny) | allow;
            else if (type == 0 && roleIds.Contains(id)) { roleAllow |= allow; roleDeny |= deny; }
            else if (type == 1 && id == UserId) { ownAllow |= allow; ownDeny |= deny; }
        }
        permissions = (((permissions & ~roleDeny) | roleAllow) & ~ownDeny) | ownAllow;
        return TimedOut ? permissions & ((1UL << 10) | (1UL << 16)) : permissions;
    }
    public ChannelAccess Evaluate(JsonElement channel, JsonElement parent = default)
    {
        int type = Controller.Int(channel, "type"); bool thread = type is 10 or 11 or 12;
        var effective = thread ? parent : channel;
        if (thread && effective.ValueKind != JsonValueKind.Object) return ChannelAccess.Unknown;
        if (BasePermissions != ulong.MaxValue && effective.ValueKind == JsonValueKind.Object && effective.TryGetProperty("permission_overwrites", out var rawOverwrites) && rawOverwrites.ValueKind != JsonValueKind.Array) return ChannelAccess.Unknown;
        ulong bits = ChannelPermissions(effective); bool view = (bits & (1UL << 10)) != 0;
        bool admin = BasePermissions == ulong.MaxValue, manageThreads = (bits & (1UL << 34)) != 0;
        if (type == 12 && !admin && !manageThreads && Controller.Str(channel, "owner_id") != UserId && (!channel.TryGetProperty("member", out var member) || member.ValueKind != JsonValueKind.Object)) view = false;
        bool send = view && (bits & (1UL << (thread ? 38 : 11))) != 0;
        if (thread && channel.TryGetProperty("thread_metadata", out var metadata) && (Controller.Bool(metadata, "archived") || Controller.Bool(metadata, "locked")) && !manageThreads && !admin) send = false;
        bool history = view && (bits & (1UL << 16)) != 0;
        string reason = !view ? "No tienes permiso para ver este canal." : TimedOut ? "No puedes escribir ni reaccionar mientras se aplica la restricción temporal del servidor." : !send ? "No tienes permiso para enviar mensajes en este canal." : "";
        return new(true, view, history, send, send && (bits & (1UL << 15)) != 0, history && (bits & (1UL << 6)) != 0, view, reason, send && (bits & (1UL << 14)) != 0);
    }
    public string ConfirmedMemberListId(JsonElement channel)
    {
        string explicitId = Controller.Str(channel, "member_list_id"); if (explicitId.Length > 0) return explicitId;
        if (Controller.Int(channel, "type") is 10 or 11 or 12 || !EveryoneViews) return "";
        if (channel.TryGetProperty("permission_overwrites", out var values) && Controller.Items(values).Any(o => (ReadBits(o, "deny") & 1024) != 0)) return "";
        return "everyone";
    }
}
