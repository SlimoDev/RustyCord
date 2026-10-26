using System.Globalization;
using System.Net.Http;
using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private readonly Dictionary<string, UserPresence> richPresences = new(StringComparer.Ordinal);
    private readonly Dictionary<string, (Person[] People, string Owner)> groupRecipients = new(StringComparer.Ordinal);
    private int profileVersion, pinsVersion;
    private readonly Dictionary<string, NavItem> forwardDestinations = new(StringComparer.Ordinal);
    private int forwardDestinationsVersion;

    private void ClearChannelSocialView() => state = state with { SelectedChannelKind = "", CanPinMessages = false, ChannelAccess = ChannelAccess.Unknown,
        GroupMembers = Array.Empty<ChannelMember>(), GroupRecipientCandidates = Array.Empty<Person>(), PinnedMessages = Array.Empty<ChatMessage>(), SearchPeople = Array.Empty<Person>() };

    private void RememberChannelRecipients(JsonElement channel)
    {
        if (!channel.TryGetProperty("recipients", out var recipients)) return;
        var people = Items(recipients).Take(25).Select(ParsePerson).Where(p => p.Id.Length > 0).ToArray();
        RememberPeople(people);
        string id = Str(channel, "id");
        if (Int(channel, "type") != 3 || id.Length == 0) return;
        if (!groupRecipients.ContainsKey(id) && groupRecipients.Count >= 200) groupRecipients.Remove(groupRecipients.Keys.First());
        groupRecipients[id] = (people, Str(channel, "owner_id"));
        if (id == currentChannel) RefreshGroupMemberView();
    }

    private ChannelMember DecorateMember(Person person, string owner)
    {
        richPresences.TryGetValue(person.Id, out var presence);
        string status = person.Id == state.CurrentUser?.Id ? state.SelfPresence : presence?.Status ?? "";
        return new(person, status, presence?.CustomStatus ?? "", presence?.CustomStatusEmoji, presence?.Activities ?? Array.Empty<PresenceActivity>(), person.Id == owner);
    }

    private void RefreshGroupMemberView()
    {
        if (state.SelectedChannelKind != "group") return;
        groupRecipients.TryGetValue(currentChannel, out var group);
        var people = (group.People ?? Array.Empty<Person>()).Concat(state.CurrentUser == null ? Array.Empty<Person>() : new[] { state.CurrentUser })
            .GroupBy(p => p.Id).Select(g => knownPeople.GetValueOrDefault(g.Key) ?? g.First()).Take(25).ToArray();
        var members = people.Select(p => DecorateMember(p, group.Owner ?? "")).OrderByDescending(p => p.IsOwner)
            .ThenBy(p => p.Presence is "online" or "idle" or "dnd" ? 0 : 1).ThenBy(p => p.User.Name, StringComparer.CurrentCultureIgnoreCase).ToArray();
        var ids = people.Select(p => p.Id).ToHashSet(StringComparer.Ordinal);
        state = state with { GroupMembers = members, GroupRecipientCandidates = state.Relationships.Where(r => r.Type == 1 && !ids.Contains(r.User.Id)).Select(r => r.User).Take(1000).ToArray() };
        UpdateSearchPeople();
    }

    public async Task<IReadOnlyList<NavItem>> GetForwardChannelsAsync(string guildId, CancellationToken ct = default)
    {
        var active = session; int generation = sessionGeneration, version = ++forwardDestinationsVersion;
        if (active == null || !state.SignedIn || sessionChanging) return Array.Empty<NavItem>();
        if (guildId.Length == 0) return state.DirectMessages.Take(1000).Select(n => n with { Access = ChannelAccess.Private }).ToArray();
        guildId = Snowflake(guildId);
        if (!state.Guilds.Any(g => g.Id == guildId)) throw new ServiceException("El servidor ya no está disponible.");
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct, sessionWork.Token);
        var access = await FetchGuildAccessAsync(active, generation, guildId, linked.Token);
        if (access == null || !IsCurrent(active, generation) || version != forwardDestinationsVersion) return Array.Empty<NavItem>();
        var channels = await active.GetAsync($"guilds/{guildId}/channels", linked.Token);
        if (!IsCurrent(active, generation) || version != forwardDestinationsVersion) return Array.Empty<NavItem>();
        var catalog = Items(channels).Take(2000).ToDictionary(c => Str(c, "id"), c => c.Clone(), StringComparer.Ordinal);
        var result = catalog.Values.Where(c => AccessFor(c, access, catalog).CanView).Select(c => DecorateChannel(c, access) with { Access = AccessFor(c, access, catalog) })
            .Where(c => c.Kind is "text" or "thread").Take(500).ToArray();
        foreach (var item in result)
        {
            if (forwardDestinations.Count >= 2000 && !forwardDestinations.ContainsKey(item.Id)) { string oldest = forwardDestinations.Keys.First(); forwardDestinations.Remove(oldest); forwardAccess.Remove(oldest); }
            forwardDestinations[item.Id] = item; var channel = catalog[item.Id]; catalog.TryGetValue(Str(channel, "parent_id"), out var parent);
            forwardAccess[item.Id] = (guildId, access, channel, parent);
        }
        return result;
    }

    public static Relationship ParseRelationship(JsonElement relation)
    {
        DateTimeOffset? since = null;
        if (Int(relation, "type") == 1 && DateTimeOffset.TryParse(Str(relation, "since"), CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out var parsed)) since = parsed;
        return new(ParsePerson(relation.TryGetProperty("user", out var user) ? user : default), Int(relation, "type"), since);
    }

    public static DateTimeOffset? AccountCreatedAt(string id)
    {
        if (!ulong.TryParse(id, NumberStyles.None, CultureInfo.InvariantCulture, out var snowflake)) return null;
        try { return DateTimeOffset.FromUnixTimeMilliseconds((long)(snowflake >> 22) + 1420070400000L); }
        catch (ArgumentOutOfRangeException) { return null; }
    }

    public static UserPresence ParsePresenceDetails(JsonElement data)
    {
        string status = Str(data, "status");
        if (status is not ("online" or "idle" or "dnd" or "offline" or "invisible")) status = "";
        var activities = data.TryGetProperty("activities", out var values) ? Items(values).Take(8).Where(a => a.ValueKind == JsonValueKind.Object).Select(ParseActivity).ToArray() : Array.Empty<PresenceActivity>();
        var custom = activities.FirstOrDefault(a => a.Type == 4);
        return new(status, custom?.State ?? "", custom?.Emoji, activities.Where(a => a.Type != 4).ToArray());
    }

    private static PresenceActivity ParseActivity(JsonElement data)
    {
        StatusEmoji? emoji = null;
        if (data.TryGetProperty("emoji", out var value) && value.ValueKind == JsonValueKind.Object)
            emoji = new(Limited(Str(value, "name"), 80), Str(value, "id"), Bool(value, "animated"));
        string application = Str(data, "application_id"), image = "";
        if (data.TryGetProperty("assets", out var assets))
        {
            string large = Str(assets, "large_image");
            if (large.StartsWith("spotify:", StringComparison.Ordinal)) image = "https://i.scdn.co/image/" + large[8..];
            else if (application.Length > 0 && large.Length > 0 && large.All(char.IsAsciiDigit)) image = $"https://cdn.discordapp.com/app-assets/{application}/{large}.png?size=128";
        }
        DateTimeOffset? start = null, end = null;
        if (data.TryGetProperty("timestamps", out var times))
        { start = ActivityTimestamp(times, "start"); end = ActivityTimestamp(times, "end"); }
        return new(Int(data, "type"), Limited(Str(data, "name"), 160), Limited(Str(data, "details"), 300), Limited(Str(data, "state"), 300), emoji, application, image, start, end);
    }
    private static DateTimeOffset? ActivityTimestamp(JsonElement value, string key)
    {
        if (value.ValueKind != JsonValueKind.Object || !value.TryGetProperty(key, out var property) || property.ValueKind != JsonValueKind.Number || !property.TryGetInt64(out long milliseconds)) return null;
        try { return DateTimeOffset.FromUnixTimeMilliseconds(milliseconds); } catch (ArgumentOutOfRangeException) { return null; }
    }
    private static string Limited(string value, int max) => value.Length > max ? value[..max] : value;
    public static string PresenceSummary(IReadOnlyList<PresenceActivity> activities)
    {
        var activity = activities.FirstOrDefault();
        if (activity == null) return "";
        string title = activity.Type switch { 0 => "Jugando a " + activity.Name, 1 => "Transmitiendo " + activity.Name,
            2 => "Escuchando " + (activity.Details.Length > 0 ? activity.Details : activity.Name), 3 => "Viendo " + activity.Name,
            5 => "Compitiendo en " + activity.Name, _ => activity.Name };
        return Limited(title, 220);
    }

    private ProfileDetails KnownProfile(Person person)
    {
        richPresences.TryGetValue(person.Id, out var presence);
        var member = state.GuildMembers.FirstOrDefault(m => m.User.Id == person.Id);
        bool context = currentGuild.Length > 0 && SelectedAccess().CanView && (member != null || state.Messages.Any(m => m.ChannelId == currentChannel && m.Author.Id == person.Id) || person.Id == state.CurrentUser?.Id);
        return new(person, AccountCreatedAt(person.Id), state.Relationships.FirstOrDefault(r => r.User.Id == person.Id && r.Type == 1)?.Since,
            "", person.Id == state.CurrentUser?.Id ? state.SelfPresence : presence?.Status ?? "", presence?.CustomStatus ?? "", presence?.CustomStatusEmoji, presence?.Activities ?? Array.Empty<PresenceActivity>(),
            Array.Empty<Person>(), Array.Empty<NavItem>(), false, false, context ? currentGuild : "", context ? state.Guilds.FirstOrDefault(g => g.Id == currentGuild)?.Name ?? "Servidor" : "", context ? member?.Roles : null, context ? member?.Nickname ?? "" : "");
    }

    private async Task LoadProfileAsync(DiscordSession active, int generation, string userId)
    {
        userId = Snowflake(userId); ResetNavigation(); int version = ++profileVersion;
        var person = userId == state.CurrentUser?.Id ? state.CurrentUser! : knownPeople.GetValueOrDefault(userId) ?? new Person(userId, "Usuario");
        state = state with { View = "profile", ProfileUser = person, ProfileDetails = KnownProfile(person), ProfileSubtitle = "Cargando perfil…" }; Refresh();
        string query = "?with_mutual_guilds=true&with_mutual_friends=true";
        string profileGuild = KnownProfile(person).GuildId;
        if (profileGuild.Length > 0) query += "&guild_id=" + Snowflake(profileGuild);
        try
        {
            var profile = await active.GetAsync($"users/{userId}/profile" + query, channelLoad.Token);
            if (!IsCurrent(active, generation) || version != profileVersion || state.View != "profile" || state.ProfileUser?.Id != userId) return;
            var details = ParseProfileDetails(profile, KnownProfile(person), state.Guilds, knownPeople.Values.ToArray());
            if (profileGuild.Length > 0 && profileGuild == currentGuild && SelectedAccess().CanView)
            {
                JsonElement serverMember = profile.TryGetProperty("guild_member", out var suppliedMember) ? suppliedMember : default;
                if (serverMember.ValueKind != JsonValueKind.Object)
                {
                    try { serverMember = await active.GetAsync($"guilds/{profileGuild}/members/{userId}", channelLoad.Token); }
                    catch (ServiceException) { }
                }
                if (!IsCurrent(active, generation) || version != profileVersion || state.View != "profile" || currentGuild != profileGuild) return;
                if (serverMember.ValueKind == JsonValueKind.Object)
                {
                    var roles = serverMember.TryGetProperty("roles", out var roleValues) ? Items(roleValues).Where(r => r.ValueKind == JsonValueKind.String).Select(r => r.GetString()).ToHashSet(StringComparer.Ordinal) : new HashSet<string?>();
                    details = details with { GuildRoles = (selectedGuildAccess?.Roles ?? Array.Empty<GuildRole>()).Where(r => roles.Contains(r.Id)).ToArray(), Nickname = Str(serverMember, "nick") };
                }
            }
            RememberPeople(new[] { details.User }.Concat(details.MutualFriends));
            state = state with { ProfileUser = details.User, ProfileDetails = details, ProfileSubtitle = "Perfil de Discord" }; Refresh();
        }
        catch (ServiceException)
        {
            if (IsCurrent(active, generation) && version == profileVersion && state.View == "profile" && state.ProfileUser?.Id == userId)
            { state = state with { ProfileSubtitle = "Discord no permite cargar todos los datos de este perfil. Se muestran los datos conocidos." }; Refresh(); }
        }
    }

    public static ProfileDetails ParseProfileDetails(JsonElement profile, ProfileDetails fallback, IReadOnlyList<NavItem> guilds, IReadOnlyList<Person> people)
    {
        var person = profile.TryGetProperty("user", out var user) ? ParsePerson(user) : fallback.User;
        string bio = profile.TryGetProperty("user_profile", out var publicProfile) ? Str(publicProfile, "bio") : "";
        if (bio.Length == 0) bio = Str(user, "bio");
        bool hasFriends = profile.TryGetProperty("mutual_friends", out var mutualFriends) && mutualFriends.ValueKind == JsonValueKind.Array;
        var friends = hasFriends ? Items(mutualFriends).Take(1000).Select(entry => entry.ValueKind == JsonValueKind.Object ? ParsePerson(entry.TryGetProperty("user", out var nested) ? nested : entry) :
            entry.ValueKind == JsonValueKind.String ? people.FirstOrDefault(p => p.Id == entry.GetString()) : null).Where(p => p != null && p.Id.Length > 0).Cast<Person>().ToArray() : Array.Empty<Person>();
        bool hasGuilds = profile.TryGetProperty("mutual_guilds", out var mutualGuilds) && mutualGuilds.ValueKind == JsonValueKind.Array;
        var mutual = hasGuilds ? Items(mutualGuilds).Take(500).Select(g => ParseMutualGuild(g, guilds)).Where(g => g.Id.Length > 0).ToArray() : Array.Empty<NavItem>();
        return fallback with { User = person, CreatedAt = AccountCreatedAt(person.Id), Bio = Limited(bio, 4000), MutualFriends = friends,
            MutualGuilds = mutual, MutualFriendsAvailable = hasFriends, MutualGuildsAvailable = hasGuilds };
    }

    private async Task<bool> TryHandleSocialActionAsync(UiAction action, DiscordSession active, int generation)
    {
        switch (action.Kind)
        {
            case "profile": await LoadProfileAsync(active, generation, action.Id); return true;
            case "profile-self":
                if (state.CurrentUser != null) await LoadProfileAsync(active, generation, state.CurrentUser.Id);
                return true;
            case "dismiss-call": DismissIncomingCall(Snowflake(action.Id)); return true;
            case "notification-open": await OpenNotificationChannelAsync(Snowflake(action.Id), active, generation); return true;
            case "group-members":
                RequireSelectedSocialChannel(action, "group");
                var channel = await active.GetAsync($"channels/{Snowflake(currentChannel)}", sessionWork.Token);
                if (!IsCurrent(active, generation) || action.ChannelId != currentChannel) return true;
                RememberChannelRecipients(channel); RefreshGroupMemberView(); Refresh(); return true;
            case "group-add":
                RequireSelectedSocialChannel(action, "group");
                string groupId = currentChannel, recipient = Snowflake(action.Id);
                if (!state.Relationships.Any(r => r.Type == 1 && r.User.Id == recipient)) throw new ServiceException("Sólo puedes añadir un amigo desde esta lista.");
                if (state.GroupMembers.Any(m => m.User.Id == recipient)) throw new ServiceException("Esta persona ya está en el grupo.");
                if (state.GroupMembers.Count >= 10) throw new ServiceException("El grupo ya tiene diez participantes.");
                await active.SendAsync(HttpMethod.Put, $"channels/{groupId}/recipients/{recipient}", new { }, sessionWork.Token);
                if (!IsCurrent(active, generation) || currentChannel != groupId) return true;
                var updated = await active.GetAsync($"channels/{groupId}", sessionWork.Token);
                if (!IsCurrent(active, generation) || currentChannel != groupId) return true;
                RememberChannelRecipients(updated); RefreshGroupMemberView(); Status("Participante añadido al grupo"); return true;
            case "pins":
                RequireSelectedSocialChannel(action);
                if (state.View == "pins") { ++pinsVersion; state = state with { View = "chat", Messages = ReadContextMessages(currentChannel) }; Status("Chat restaurado"); return true; }
                await LoadPinnedMessagesAsync(action, active, generation); return true;
            case "pin-message": case "unpin-message":
                RequireSelectedSocialChannel(action);
                if (!state.CanPinMessages) throw new ServiceException("No se han confirmado permisos para fijar mensajes en este canal.");
                if (cache.Get(currentChannel, action.Id) == null && !state.PinnedMessages.Any(m => m.Id == action.Id)) throw new ServiceException("El mensaje ya no está en la vista. Vuelve a cargarlo.");
                string pinChannel = currentChannel, message = Snowflake(action.Id); bool pin = action.Kind == "pin-message";
                await active.SendAsync(pin ? HttpMethod.Put : HttpMethod.Delete, $"channels/{pinChannel}/messages/pins/{message}", ct: sessionWork.Token);
                if (!IsCurrent(active, generation) || currentChannel != pinChannel) return true;
                var cached = cache.Get(pinChannel, message); if (cached != null) cache.Upsert(cached with { Pinned = pin });
                state = state with { PinnedMessages = pin ? state.PinnedMessages : state.PinnedMessages.Where(m => m.Id != message).ToArray() };
                UpdateMessages(); Status(pin ? "Mensaje fijado en Discord" : "Mensaje desfijado en Discord"); return true;
            case "jump-message":
                await JumpToMessageAsync(action, active, generation); return true;
            case "forward":
            case "forward-multiple":
                await ForwardManyAsync(action, active, generation); return true;
        }
        return false;
    }

    private static NavItem ParseMutualGuild(JsonElement value, IReadOnlyList<NavItem> knownGuilds)
    {
        var guild = value.ValueKind == JsonValueKind.Object && value.TryGetProperty("guild", out var nested) && nested.ValueKind == JsonValueKind.Object ? nested : value;
        string id = Str(guild, "id"); if (id.Length == 0) id = Str(value, "guild_id");
        var known = knownGuilds.FirstOrDefault(g => g.Id == id);
        string name = Str(guild, "name"), icon = Str(guild, "icon");
        string? url = icon.Length > 0 ? $"https://cdn.discordapp.com/icons/{id}/{icon}.png?size=64" : known?.IconUrl;
        return known == null ? new(id, name.Length > 0 ? name : "Servidor en común", "guild", IconUrl: url) : known with { Name = name.Length > 0 ? name : known.Name, IconUrl = url };
    }

    public static object ForwardPayload(ChatMessage source, string guildId) => new
    {
        message_reference = new { type = 1, message_id = Snowflake(source.Id), channel_id = Snowflake(source.ChannelId), guild_id = guildId.Length > 0 ? Snowflake(guildId) : null },
        allowed_mentions = new { parse = Array.Empty<string>(), replied_user = false }
    };

    private void RequireSelectedSocialChannel(UiAction action, string? kind = null)
    {
        if (currentChannel.Length == 0 || action.ChannelId != currentChannel || state.SelectedChannelId != currentChannel || state.View is not ("chat" or "pins") || kind != null && state.SelectedChannelKind != kind)
            throw new ServiceException("La vista cambió. Abre el canal y repite la acción; no se ha enviado nada.");
    }

    private async Task LoadPinnedMessagesAsync(UiAction action, DiscordSession active, int generation)
    {
        RequireSelectedSocialChannel(action); RequirePermission("history"); string id = currentChannel; int version = ++pinsVersion, navigation = navigationVersion;
        var response = await active.GetAsync($"channels/{Snowflake(id)}/messages/pins?limit=50", channelLoad.Token);
        if (!IsCurrent(active, generation) || currentChannel != id || action.ChannelId != currentChannel || version != pinsVersion || navigation != navigationVersion || state.View is not ("chat" or "pins")) return;
        var messages = ParsePinnedMessages(response);
        RememberPeople(messages.Select(m => m.Author));
        state = state with { View = "pins", PinnedMessages = messages, Status = messages.Length == 0 ? "No hay mensajes fijados" : "Mensajes fijados · hasta 50 más recientes" }; Refresh();
    }
    public static ChatMessage[] ParsePinnedMessages(JsonElement response)
    {
        var entries = response.ValueKind == JsonValueKind.Object && response.TryGetProperty("items", out var items) ? Items(items).Take(50).ToArray() : Items(response).Take(50).ToArray();
        bool dated = entries.Length > 0 && entries.All(e => DateTimeOffset.TryParse(Str(e, "pinned_at"), CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out _));
        var ordered = dated ? entries.OrderByDescending(e => DateTimeOffset.Parse(Str(e, "pinned_at"), CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind)).ToArray() : entries;
        return ordered.Select(e => e.ValueKind == JsonValueKind.Object && e.TryGetProperty("message", out var message) ? message : e).Select(ParseMessage).Select(m => m with { Pinned = true }).ToArray();
    }

    private async Task<bool> HandleSocialDispatchAsync(string type, JsonElement data)
    {
        if (type is "CHANNEL_RECIPIENT_ADD" or "CHANNEL_RECIPIENT_REMOVE")
        {
            string channel = Str(data, "channel_id");
            if (!groupRecipients.TryGetValue(channel, out var group)) return true;
            var user = data.TryGetProperty("user", out var supplied) ? ParsePerson(supplied) : new Person(Str(data, "user_id"), "Usuario");
            if (user.Id.Length == 0) return true;
            var people = group.People.Where(p => p.Id != user.Id);
            if (type == "CHANNEL_RECIPIENT_ADD") { people = people.Append(user); RememberPeople(new[] { user }); }
            groupRecipients[channel] = (people.Take(25).ToArray(), group.Owner);
            RefreshGroupMemberView(); Refresh(); return true;
        }
        if (type == "CHANNEL_PINS_UPDATE")
        {
            if (Str(data, "channel_id") == currentChannel && state.View == "pins")
            { var active = session; if (active != null) await LoadPinnedMessagesAsync(new UiAction("pins", ChannelId: currentChannel), active, sessionGeneration); }
            return true;
        }
        return false;
    }
}
