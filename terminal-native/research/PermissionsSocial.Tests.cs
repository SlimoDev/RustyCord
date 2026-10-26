using System.Net;
using System.IO;
using System.Net.Http;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Threading;
using NotDiscord.Terminal;

internal static class PermissionsSocialTests
{
    static int checks; static readonly List<string> errors = new();
    static JsonElement J(string text) => JsonDocument.Parse(text).RootElement.Clone();
    static void Check(bool ok, string label) { checks++; if (!ok) errors.Add(label); }
    static object? Field(object target, string name) => target.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(target);
    static void Set(object target, string name, object? value) => target.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
    static object? Call(object target, string name, params object[] values) => target.GetType().GetMethod(name, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(target, values);
    static async Task Reject(Func<Task> work, string label) { bool rejected = false; try { await work(); } catch (ServiceException) { rejected = true; } Check(rejected, label); }
    [STAThread] static int Main()
    {
        try
        {
            var dispatcher = Dispatcher.CurrentDispatcher; SynchronizationContext.SetSynchronizationContext(new DispatcherSynchronizationContext(dispatcher));
            var work = Run(); if (!work.IsCompleted) { var frame = new DispatcherFrame(); _ = work.ContinueWith(_ => dispatcher.BeginInvoke(() => frame.Continue = false)); Dispatcher.PushFrame(frame); }
            work.GetAwaiter().GetResult(); Application.Current?.Shutdown();
        }
        catch (Exception error) { errors.Add(error.GetType().Name + ": " + error.Message); }
        Console.WriteLine(JsonSerializer.Serialize(new { success = errors.Count == 0, checks, errors, syntheticDataOnly = true, realNetworkUsed = false, realCalls = 0, realNotifications = 0 })); return errors.Count == 0 ? 0 : 1;
    }
    static async Task Run()
    {
        const ulong view = 1UL << 10, send = 1UL << 11, read = 1UL << 16, attach = 1UL << 15, react = 1UL << 6, embed = 1UL << 14;
        ulong all = view | send | read | attach | react | embed;
        JsonElement Guild(ulong everyone, ulong extra = 0) => JsonSerializer.SerializeToElement(new { id = "500", owner_id = "999", roles = new object[] { new { id = "500", name = "@everyone", permissions = everyone.ToString(), position = 0, color = 0, hoist = false }, new { id = "600", name = "Moderadores", permissions = extra.ToString(), position = 10, color = 0x33AA77, hoist = true } } });
        var member = J("{\"roles\":[\"600\"]}");
        var channel = J("{\"id\":\"700\",\"guild_id\":\"500\",\"name\":\"general\",\"type\":0,\"permission_overwrites\":[]}");
        var access = GuildAccess.From(Guild(all), member, "100");
        Check(access.Evaluate(channel) is { CanView: true, CanReadHistory: true, CanSend: true, CanAttach: true, CanReact: true, CanEmbed: true }, "Full channel capabilities share one permission evaluator");
        var overrides = J("{\"id\":\"700\",\"type\":0,\"permission_overwrites\":[{\"id\":\"600\",\"type\":0,\"allow\":\"2048\",\"deny\":\"0\"},{\"id\":\"500\",\"type\":0,\"allow\":\"0\",\"deny\":\"2048\"}]} ");
        Check(access.Evaluate(overrides).CanSend, "Role allow wins over everyone deny independently of overwrite array order");
        var ownDenied = J("{\"id\":\"700\",\"type\":0,\"permission_overwrites\":[{\"id\":\"600\",\"type\":0,\"allow\":\"2048\",\"deny\":\"0\"},{\"id\":\"100\",\"type\":1,\"allow\":\"0\",\"deny\":\"2048\"}]} ");
        Check(access.Evaluate(ownDenied) is { CanView: true, CanSend: false, CanAttach: false, CanReact: true }, "Member overwrite wins and disabled send implies disabled attachments without hiding reactions");
        var hidden = J("{\"id\":\"700\",\"type\":0,\"permission_overwrites\":[{\"id\":\"500\",\"type\":0,\"allow\":\"0\",\"deny\":\"1024\"}]} ");
        Check(access.Evaluate(hidden) is { CanView: false, CanSend: false, CanAttach: false, CanReact: false, CanViewMembers: false }, "Denied view implicitly disables all channel capabilities");
        Check(GuildAccess.From(Guild(all), member, "999").Evaluate(hidden).CanSend, "Guild owner bypasses channel overwrites");
        Check(GuildAccess.From(Guild(0, 8), member, "100").Evaluate(hidden).CanView, "Administrator bypasses channel overwrites");
        var timeout = GuildAccess.From(Guild(all), JsonSerializer.SerializeToElement(new { roles = new[] { "600" }, communication_disabled_until = DateTimeOffset.UtcNow.AddMinutes(5).ToString("O") }), "100");
        Check(timeout.Evaluate(channel) is { CanView: true, CanReadHistory: true, CanSend: false, CanAttach: false, CanReact: false }, "Timeout retains view/history and rejects sending and new reactions");
        var threads = GuildAccess.From(Guild(all | (1UL << 38)), member, "100");
        var thread = J("{\"id\":\"701\",\"type\":11,\"parent_id\":\"700\",\"thread_metadata\":{\"archived\":false,\"locked\":false}}");
        Check(threads.Evaluate(thread, channel).CanSend && !access.Evaluate(thread, channel).CanSend, "Threads use SEND_MESSAGES_IN_THREADS from parent effective permissions");
        Check(!threads.Evaluate(thread).Known, "Missing thread parent is unknown rather than implicitly granting access");
        Check(!threads.Evaluate(J("{\"id\":\"702\",\"type\":12,\"parent_id\":\"700\"}"), channel).CanView, "Private thread without membership cannot be shown");
        Check(threads.Evaluate(J("{\"id\":\"702\",\"type\":12,\"parent_id\":\"700\",\"member\":{}}"), channel).CanView, "Confirmed private-thread membership permits parent-visible thread");
        Check(!threads.Evaluate(J("{\"id\":\"701\",\"type\":11,\"thread_metadata\":{\"archived\":true}}"), channel).CanSend, "Archived thread is read only without manage threads");
        Check(access.ConfirmedMemberListId(channel) == "everyone" && access.ConfirmedMemberListId(hidden) == "", "Member lists with view-denying overwrites require a confirmed list identifier");
        Check(access.ConfirmedMemberListId(J("{\"type\":0,\"member_list_id\":\"scoped-list\"}")) == "scoped-list", "Server-provided member list id overrides inferred everyone list");
        var reactionMessage = Controller.ParseMessage(J("{\"id\":\"800\",\"channel_id\":\"700\",\"author\":{\"id\":\"101\",\"username\":\"Synthetic\"},\"timestamp\":\"2026-10-20T00:00:00Z\",\"content\":\"test\",\"reactions\":[{\"emoji\":{\"id\":null,\"name\":\"😀\"},\"count\":2,\"me\":true},{\"emoji\":{\"id\":\"900\",\"name\":\"wave\",\"animated\":true},\"count\":3,\"me\":false}]}"));
        Check(reactionMessage.Reactions is { Count: 2 } && reactionMessage.Reactions[0] is { Count: 2, Me: true } && reactionMessage.Reactions[1].Emoji.Id == "900", "Reaction parsing retains unicode/custom emoji, counts and current-user state");
        var premiumState = Controller.ParseReactions(J("[{\"emoji\":{\"name\":\"😀\"},\"count\":3,\"me\":false,\"me_burst\":true},{\"emoji\":{\"name\":null,\"id\":\"900\"},\"count\":2,\"me\":false}]").Clone());
        Check(premiumState[0] is { Me: true, NormalMe: false, BurstMe: true } && premiumState[1].Count == 2, "Super-reaction ownership is distinct and deleted custom emoji counts remain visible");
        var pins = Controller.ParsePinnedMessages(J("{\"items\":[{\"pinned_at\":\"2026-10-19T00:00:00Z\",\"message\":{\"id\":\"802\",\"timestamp\":\"2026-10-19T00:00:00Z\",\"content\":\"new message\"}},{\"pinned_at\":\"2026-10-20T00:00:00Z\",\"message\":{\"id\":\"801\",\"timestamp\":\"2020-01-01T00:00:00Z\",\"content\":\"old message newly pinned\"}}]}"));
        Check(pins[0].Id == "801" && pins.All(m => m.Pinned), "Pinned messages sort by pinned_at, not original message creation");
        Check(Controller.ParsePinnedMessages(J("[{\"id\":\"2\"},{\"id\":\"1\"}]"))[0].Id == "2", "Legacy pins without pinned_at preserve server order");
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown }; var window = new TerminalWindow();
        string directory = Path.Combine(AppContext.BaseDirectory, "synthetic-data"); Directory.CreateDirectory(directory);
        var requests = new List<(HttpMethod Method, string Path, string Body)>();
        var session = new DiscordSession("SYNTHETIC-NOT-A-REAL-CREDENTIAL", new FakeHttp(async request =>
        {
            string body = request.Content == null ? "" : await request.Content.ReadAsStringAsync(); requests.Add((request.Method, request.RequestUri!.AbsolutePath, body));
            string path = request.RequestUri.AbsolutePath;
            if (request.Method == HttpMethod.Get && (path.EndsWith("/guilds/501") || path.EndsWith("/guilds/502")))
            {
                string id = path.EndsWith("501") ? "501" : "502";
                return new(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(new { id, owner_id = "999", roles = new[] { new { id, name = "@everyone", permissions = all.ToString() } } })) };
            }
            if (request.Method == HttpMethod.Get && path.Contains("/members/")) return new(HttpStatusCode.OK) { Content = new StringContent("{\"roles\":[]}") };
            if (request.Method == HttpMethod.Get && path.EndsWith("/guilds/501/channels")) return new(HttpStatusCode.OK) { Content = new StringContent("[{\"id\":\"901\",\"guild_id\":\"501\",\"type\":0,\"name\":\"First guild destination\",\"permission_overwrites\":[]},{\"id\":\"903\",\"guild_id\":\"501\",\"type\":0,\"name\":\"Read only\",\"permission_overwrites\":[{\"id\":\"501\",\"type\":0,\"allow\":\"0\",\"deny\":\"2048\"}]}]") };
            if (request.Method == HttpMethod.Get && path.EndsWith("/guilds/502/channels")) return new(HttpStatusCode.OK) { Content = new StringContent("[{\"id\":\"902\",\"guild_id\":\"502\",\"type\":0,\"name\":\"Second guild destination\",\"permission_overwrites\":[]}]") };
            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{\"id\":\"800\",\"channel_id\":\"700\",\"timestamp\":\"2026-10-20T00:00:00Z\",\"author\":{\"id\":\"100\",\"username\":\"Synthetic\"},\"content\":\"fixture\",\"reactions\":[{\"emoji\":{\"name\":\"😀\"},\"count\":2,\"me\":true}]}") };
        }));
        await using var controller = new Controller(window, directory);
        Set(controller, "session", session); Set(controller, "currentGuild", "500"); Set(controller, "currentChannel", "700"); Set(controller, "selectedGuildAccess", access);
        Call(controller, "RememberSelectedChannelMetadata", JsonSerializer.SerializeToElement(new[] { channel }));
        var me = new Person("100", "Synthetic self"); var stranger = new Person("9999", "Unrelated friend");
        var snapshot = new AppSnapshot { SignedIn = true, CurrentUser = me, SelectedGuildId = "500", SelectedChannelId = "700", SelectedChannelKind = "text", View = "chat", ChannelAccess = access.Evaluate(channel), Channels = new[] { new NavItem("700", "general", "text", Access: access.Evaluate(channel)) }, Guilds = new[] { new NavItem("500", "Fixture guild", "guild") }, Messages = new[] { reactionMessage }, Relationships = new[] { new Relationship(stranger, 1) } };
        Set(controller, "state", snapshot); ((MessageWindowCache)Field(controller, "cache")!).Replace("700", new[] { reactionMessage });
        Call(controller, "ApplyGuildMemberList", J("{\"guild_id\":\"500\",\"id\":\"wrong-list\",\"member_count\":1,\"ops\":[{\"op\":\"SYNC\",\"range\":[0,0],\"items\":[{\"member\":{\"user\":{\"id\":\"111\",\"username\":\"Hidden\"},\"roles\":[\"600\"]}}]}]}"));
        Check(((AppSnapshot)Field(controller, "state")!).GuildMembers.Count == 0, "Uncorrelated member list cannot leak into the selected channel");
        Call(controller, "ApplyGuildMemberList", J("{\"guild_id\":\"500\",\"id\":\"everyone\",\"member_count\":120,\"groups\":[{\"id\":\"600\",\"count\":1}],\"ops\":[{\"op\":\"SYNC\",\"range\":[0,0],\"items\":[{\"member\":{\"user\":{\"id\":\"111\",\"username\":\"Visible\"},\"nick\":\"Alias\",\"roles\":[\"600\"],\"presence\":{\"status\":\"online\",\"activities\":[]}}}]}]}"));
        var scoped = (AppSnapshot)Field(controller, "state")!;
        Check(scoped.GuildMembers.Count == 1 && scoped.GuildMembers[0] is { Nickname: "Alias", RoleColor: 0x33AA77, HoistRoleId: "600", Presence: "online" }, "Visible channel member carries nickname, hierarchy, role color and state");
        Check(scoped.GuildMembersHasMore && scoped.GuildMembersTotal == 120 && scoped.GuildMembersPartial, "Member pagination reports the initial page as partial");
        Check(scoped.SearchPeople.Any(p => p.Id == "111") && scoped.SearchPeople.Any(p => p.Id == "101") && !scoped.SearchPeople.Any(p => p.Id == stranger.Id), "Server search scope contains visible members/authors rather than unrelated friends");
        await (Task<bool>)Call(controller, "HandleAccessDispatchAsync", "CHANNEL_UPDATE", J("{\"id\":\"700\",\"guild_id\":\"500\",\"topic\":\"partial change\"}"))!;
        Check(((AppSnapshot)Field(controller, "state")!).ChannelAccess.CanSend && ((AppSnapshot)Field(controller, "state")!).GuildMembers.Count == 1, "Partial channel updates preserve established ACL and current members");
        await (Task<bool>)Call(controller, "HandleAccessDispatchAsync", "CHANNEL_UPDATE", J("{\"id\":\"700\",\"guild_id\":\"500\",\"permission_overwrites\":[{\"id\":\"100\",\"type\":1,\"allow\":\"0\",\"deny\":\"2048\"}]}"))!;
        Check(!((AppSnapshot)Field(controller, "state")!).ChannelAccess.CanSend && ((AppSnapshot)Field(controller, "state")!).GuildMembers.Count == 0, "ACL changes revoke sending and discard old member-list scope immediately");
        Call(controller, "RememberSelectedChannelMetadata", JsonSerializer.SerializeToElement(new[] { channel }));
        Call(controller, "HandleReactionDispatch", "MESSAGE_REACTION_ADD", J("{\"channel_id\":\"700\",\"message_id\":\"800\",\"user_id\":\"111\",\"emoji\":{\"name\":\"😀\"}}"));
        Check(((MessageWindowCache)Field(controller, "cache")!).Get("700", "800")!.Reactions![0].Count == 3, "Gateway reaction add updates count without rebuilding unrelated messages");
        Call(controller, "HandleReactionDispatch", "MESSAGE_REACTION_REMOVE", J("{\"channel_id\":\"700\",\"message_id\":\"800\",\"user_id\":\"100\",\"emoji\":{\"name\":\"😀\"}}"));
        Check(((MessageWindowCache)Field(controller, "cache")!).Get("700", "800")!.Reactions![0] is { Count: 2, Me: false }, "Own removal updates both count and highlighted state");
        Call(controller, "HandleReactionDispatch", "MESSAGE_REACTION_REMOVE_EMOJI", J("{\"channel_id\":\"700\",\"message_id\":\"800\",\"emoji\":{\"name\":\"wave\",\"id\":\"900\"}}"));
        Check(((MessageWindowCache)Field(controller, "cache")!).Get("700", "800")!.Reactions!.Count == 1, "Removing one emoji preserves other reactions");
        var ordinary = ((MessageWindowCache)Field(controller, "cache")!).Get("700", "800")!;
        ((MessageWindowCache)Field(controller, "cache")!).Upsert(ordinary with { Reactions = new[] { premiumState[0] } });
        await (Task)Call(controller, "ToggleReactionAsync", new UiAction("react-toggle", "800", "😀", ChannelId: "700"), session, 0, true)!;
        Check(requests.Any(r => r.Method == HttpMethod.Delete && r.Path.EndsWith("/reactions/" + Uri.EscapeDataString("😀") + "/1/@me", StringComparison.Ordinal)), "Removing an owned super reaction uses the original client's typed endpoint");
        ((MessageWindowCache)Field(controller, "cache")!).Upsert(ordinary);
        var withoutAdd = GuildAccess.From(Guild(all & ~react), member, "100"); Set(controller, "selectedGuildAccess", withoutAdd);
        await (Task)Call(controller, "ToggleReactionAsync", new UiAction("react-toggle", "800", "😀", ChannelId: "700"), session, 0, true)!;
        Check(requests.Any(r => r.Method == HttpMethod.Put), "Existing reaction can be joined even without ADD_REACTIONS");
        int before = requests.Count;
        await Reject(() => (Task)Call(controller, "ToggleReactionAsync", new UiAction("react-toggle", "800", "🔥", ChannelId: "700"), session, 0, true)!, "New reaction is rejected without ADD_REACTIONS");
        Check(requests.Count == before, "Denied new reaction makes no HTTP request");
        Set(controller, "selectedGuildAccess", GuildAccess.From(Guild(view | read), member, "100"));
        await Reject(() => (Task)Call(controller, "ActAsync", new UiAction("send", Text: "Do not send", ChannelId: "700"))!, "Direct send action is rejected in a read-only channel");
        Check(requests.Count == before, "Read-only send is rejected before contacting Discord");
        string file = Path.Combine(directory, "synthetic-attachment.txt"); File.WriteAllText(file, "synthetic bytes");
        string guid = new string('a', 32); string prepared = JsonSerializer.Serialize(new PreparedSend(guid, "799", new[] { file }));
        Set(controller, "selectedGuildAccess", GuildAccess.From(Guild(view | read | send), member, "100"));
        await Reject(() => (Task)Call(controller, "ActAsync", new UiAction("send-prepared", Text: "Caption", Extra: prepared, ChannelId: "700"))!, "Prepared attachments are rejected when ATTACH_FILES is absent");
        Check(requests.Count == before, "Denied prepared upload does not read or transmit content");
        Set(controller, "selectedGuildAccess", access);
        await (Task)Call(controller, "ActAsync", new UiAction("send-prepared", Text: "Synthetic caption", Extra: prepared, ChannelId: "700"))!;
        var multipart = requests.Last(r => r.Method == HttpMethod.Post);
        Check(multipart.Body.Contains("Synthetic caption") && multipart.Body.Contains("synthetic bytes") && multipart.Body.Contains("files[0]") && multipart.Body.Contains("799"), "Prepared multipart carries caption, explicit attachment and reply in one confirmed request");
        Check(multipart.Body.Contains(new string('a', 24)) && !multipart.Body.Contains(new string('a', 25)), "Prepared nonce stays within Discord's 25-character limit");
        var forwarding = ((AppSnapshot)Field(controller, "state")!) with { Guilds = new[] { new NavItem("500", "Origin", "guild"), new NavItem("501", "First", "guild"), new NavItem("502", "Second", "guild") } }; Set(controller, "state", forwarding);
        var firstTargets = await controller.GetForwardChannelsAsync("501"); var secondTargets = await controller.GetForwardChannelsAsync("502");
        Check(firstTargets.Any(c => c.Id == "903" && c.Access is { CanView: true, CanSend: false }), "Forward picker keeps visible read-only destinations with disabled-send permission");
        int posts = requests.Count(r => r.Method == HttpMethod.Post);
        await (Task)Call(controller, "ForwardManyAsync", new UiAction("forward-multiple", "800", "[\"901\",\"902\"]", ChannelId: "700"), session, 0)!;
        Check(requests.Count(r => r.Method == HttpMethod.Post) == posts + 2 && requests.TakeLast(2).Select(r => r.Path).Order().SequenceEqual(new[] { "/api/v9/channels/901/messages", "/api/v9/channels/902/messages" }), "One explicit forward preserves destinations across guild switches and sends to both");
        posts = requests.Count(r => r.Method == HttpMethod.Post);
        await Reject(() => (Task)Call(controller, "ForwardManyAsync", new UiAction("forward-multiple", "800", "[\"901\",\"903\"]", ChannelId: "700"), session, 0)!, "Forward validates read-only destination before sending any destination");
        Check(requests.Count(r => r.Method == HttpMethod.Post) == posts, "Invalid multi-destination permission causes no partial sends");
        before = requests.Count;
        await Reject(() => (Task)Call(controller, "ActAsync", new UiAction("search", Text: "from:9999"))!, "Numeric search author outside visible participants is rejected by backend");
        Check(requests.Count == before, "Out-of-scope search filter does not contact Discord");
        var revocation = ((AppSnapshot)Field(controller, "state")!) with { View = "profile", ProfileDetails = new ProfileDetails(me, null, null, "", "online", "", null, Array.Empty<PresenceActivity>(), Array.Empty<Person>(), Array.Empty<NavItem>(), GuildId: "500", GuildName: "Origin", GuildRoles: access.Roles, Nickname: "Role-context alias") }; Set(controller, "state", revocation);
        await (Task<bool>)Call(controller, "HandleAccessDispatchAsync", "CHANNEL_UPDATE", J("{\"id\":\"700\",\"guild_id\":\"500\",\"permission_overwrites\":[{\"id\":\"500\",\"type\":0,\"allow\":\"0\",\"deny\":\"1024\"}]}"))!;
        var revoked = (AppSnapshot)Field(controller, "state")!;
        Check(!revoked.ChannelAccess.CanView && revoked.Messages.Count == 0 && revoked.GuildMembers.Count == 0 && revoked.ProfileDetails!.GuildRoles == null && revoked.ProfileDetails.GuildId == "", "Losing VIEW clears history, member list and contextual profile roles together");
        Set(controller, "state", revoked with { View = "chat" });
        await (Task)Call(controller, "HandleDispatchAsync", "MESSAGE_CREATE", J("{\"id\":\"999\",\"channel_id\":\"700\",\"guild_id\":\"500\",\"author\":{\"id\":\"111\",\"username\":\"Hidden sender\"},\"content\":\"Must not reappear\",\"mentions\":[{\"id\":\"100\"}]}"))!;
        Check(((MessageWindowCache)Field(controller, "cache")!).Read("700").Count == 0 && ((AppSnapshot)Field(controller, "state")!).Messages.Count == 0, "Late Gateway messages cannot restore content after channel access was revoked");
        Set(controller, "state", snapshot with { DirectMessages = new[] { new NavItem("710", "Private fixture", "dm", RecipientId: "111") } });
        await (Task)Call(controller, "SelectChannelAsync", "710", false, "")!;
        var privateView = (AppSnapshot)Field(controller, "state")!;
        Check(privateView.SelectedGuildId.Length == 0 && privateView.Channels.Count == 0 && privateView.GuildRoles.Count == 0 && privateView.GuildMembers.Count == 0, "Opening a private conversation clears previous server navigation, roles and member scope");
        Check(privateView.SearchPeople.Select(p => p.Id).Order().SequenceEqual(new[] { "100", "111" }), "Private search scope is limited to its recipient and current user");
        before = requests.Count;
        await Reject(() => (Task)Call(controller, "ActAsync", new UiAction("search", Text: "in:700"))!, "A private chat cannot search a formerly selected server channel by numeric id");
        Check(requests.Count == before, "Out-of-scope private channel search makes no HTTP request");
        Set(controller, "state", snapshot); Set(controller, "currentGuild", "500"); Set(controller, "currentChannel", "700"); Set(controller, "selectedGuildAccess", access); Call(controller, "RememberSelectedChannelMetadata", JsonSerializer.SerializeToElement(new[] { channel }));
        ((MessageWindowCache)Field(controller, "cache")!).Replace("700", new[] { reactionMessage });
        Call(controller, "HandleReactionDispatch", "MESSAGE_REACTION_REMOVE_ALL", J("{\"channel_id\":\"700\",\"message_id\":\"800\"}"));
        Check(((MessageWindowCache)Field(controller, "cache")!).Get("700", "800")!.Reactions!.Count == 0, "Remove-all reaction event clears visible reactions");
        await controller.DisposeAsync(); window.Close();
    }
    sealed class FakeHttp(Func<HttpRequestMessage, Task<HttpResponseMessage>> reply) : HttpMessageHandler
    { protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) => reply(request); }
}
