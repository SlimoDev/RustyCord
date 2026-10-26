using System.Net;
using System.IO;
using System.Net.Http;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Threading;
using NotDiscord.Terminal;

internal static class MessageReferencesTests
{
    static int checks; static readonly List<string> failures = new();
    static JsonElement J(string text) => JsonDocument.Parse(text).RootElement.Clone();
    static void Check(bool value, string label) { checks++; if (!value) failures.Add(label); }
    static object? Field(object target, string name) => target.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(target);
    static void Set(object target, string name, object? value) => target.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
    static object? Call(object target, string name, params object[] arguments) => target.GetType().GetMethod(name, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(target, arguments);
    static ChatMessage Update(ChatMessage old, string json) => (ChatMessage)typeof(Controller).GetMethod("ApplyMessageUpdate", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, new object[] { old, J(json) })!;
    static async Task Rejected(Func<Task> action, string label) { bool rejected = false; try { await action(); } catch (ServiceException) { rejected = true; } Check(rejected, label); }
    [STAThread] static int Main()
    {
        try
        {
            var dispatcher = Dispatcher.CurrentDispatcher; SynchronizationContext.SetSynchronizationContext(new DispatcherSynchronizationContext(dispatcher));
            var task = Run(); if (!task.IsCompleted) { var frame = new DispatcherFrame(); _ = task.ContinueWith(_ => dispatcher.BeginInvoke(() => frame.Continue = false)); Dispatcher.PushFrame(frame); }
            task.GetAwaiter().GetResult(); Application.Current?.Shutdown();
        }
        catch (Exception error) { failures.Add(error.ToString()); }
        Console.WriteLine(JsonSerializer.Serialize(new { success = failures.Count == 0, checks, failures, syntheticDataOnly = true, realNetworkUsed = false, realSends = 0 }));
        return failures.Count == 0 ? 0 : 1;
    }

    static async Task Run()
    {
        var parsed = Controller.ParseMessage(J("""
          {"id":"800","channel_id":"700","timestamp":"2026-10-20T00:00:00Z","author":{"id":"100","username":"self"},
          "content":"Hola <@101> y <@!102>","mentions":[{"id":"101","username":"alice","global_name":"Alice"},{"id":"102","username":"bob","global_name":"Bob","member":{"nick":"Alias del servidor"}}],
          "message_reference":{"message_id":"799","channel_id":"700"},"referenced_message":{"id":"799","channel_id":"700","author":{"id":"101","username":"alice","global_name":"Alice"},"content":"Original <@102>","mentions":[{"id":"102","username":"bob"}],"referenced_message":{"id":"798","content":"Should not be loaded recursively"}}}
          """));
        Check(parsed.Mentions is { Count: 2 } && parsed.Mentions[0].Name == "Alice", "Message mentions retain transport display names");
        Check(parsed.Mentions![1].Name == "Alias del servidor", "Guild mention nickname takes precedence over global display name");
        Check(parsed.Content == "Hola <@101> y <@!102>", "Transport content remains exact for edit and notification handling");
        Check(parsed.ReplyTo == "799" && parsed.ReplyPreview is { MessageId: "799", ChannelId: "700", Author.Name: "Alice", Content: "Original <@102>", Unavailable: false }, "Reply preview retains original author and text without recursive messages");
        Check(parsed.ReplyPreview!.Mentions is { Count: 1 } && parsed.ReplyPreview.Mentions[0].Id == "102", "Reply preview retains its own mention resolution scope");
        var duplicate = Controller.ParseMessageMentions(J("{\"mentions\":[{\"id\":\"1\",\"username\":\"one\"},{\"id\":\"1\",\"username\":\"other\"},{\"id\":\"bad-id\",\"username\":\"invalid\"}]}"));
        Check(duplicate.Count == 1 && duplicate[0].Username == "one", "Malformed and duplicate mention ids cannot expand rendering scope");
        var many = JsonSerializer.SerializeToElement(new { mentions = Enumerable.Range(1, 150).Select(i => new { id = i.ToString(), username = new string('x', 400) }).ToArray() });
        var bounded = Controller.ParseMessageMentions(many); Check(bounded.Count == 100 && bounded.All(p => p.Name.Length <= 128), "Mention metadata has bounded count and text size");
        string head = "\"id\":\"800\",\"channel_id\":\"700\",\"message_reference\":{\"message_id\":\"799\",\"channel_id\":\"700\"}";
        Check(Controller.ParseReplyPreview(J("{" + head + "}")) is { Unavailable: true, Deleted: false }, "Missing referenced message is unavailable rather than deleted");
        Check(Controller.ParseReplyPreview(J("{" + head + ",\"referenced_message\":null}")) is { Deleted: true, Unavailable: false }, "Explicit null referenced message indicates deletion");
        Check(Controller.ParseReplyPreview(J("{" + head + ",\"referenced_message\":{\"id\":\"777\",\"content\":\"wrong\"}}")) is { Unavailable: true, Author: null, Content.Length: 0 }, "Mismatched reference id cannot display an unrelated body");
        Check(Controller.ParseReplyPreview(J("{\"channel_id\":\"700\",\"message_reference\":{\"message_id\":\"799\",\"channel_id\":\"999\"},\"referenced_message\":{\"id\":\"799\",\"content\":\"other channel\"}}")) is { Unavailable: true, Author: null }, "Cross-channel reference never inherits the visible channel identity");
        Check(Controller.ParseReplyPreview(J("{" + head + ",\"referenced_message\":{\"id\":\"799\",\"attachments\":[{\"id\":\"1\"}]}}"))?.Content == "Archivo adjunto", "Attachment-only originals have a useful preview label");
        Check(Controller.ParseReplyPreview(J("{\"channel_id\":\"700\",\"message_reference\":{\"message_id\":\"799\",\"type\":1}}")) == null, "Forward snapshots are not parsed as reply references");
        var edited = Update(parsed, "{\"id\":\"800\",\"channel_id\":\"700\",\"content\":\"edited\"}");
        Check(edited.Mentions == parsed.Mentions && edited.ReplyPreview == parsed.ReplyPreview, "Partial content update preserves mentions and reference metadata");
        Check(Update(parsed, "{\"id\":\"800\",\"channel_id\":\"700\",\"mentions\":[]}").Mentions is { Count: 0 }, "Explicit empty mentions update clears stale resolution data");
        Check(Update(parsed, "{\"id\":\"800\",\"channel_id\":\"700\",\"referenced_message\":null}").ReplyPreview is { Deleted: true }, "Partial reference-only deletion updates a preview");
        Check(Update(parsed, "{\"id\":\"800\",\"message_reference\":{\"message_id\":\"799\"}}").ReplyPreview == parsed.ReplyPreview, "Reference-only partial payload does not erase the known preview");
        Check(Update(parsed, "{\"id\":\"800\",\"channel_id\":\"700\",\"message_reference\":{\"message_id\":\"798\"}}").ReplyPreview is { MessageId: "798", Unavailable: true, Author: null }, "Changed reference id never retains the old body");
        Check(Update(parsed, "{\"id\":\"800\",\"channel_id\":\"700\",\"message_reference\":{\"message_id\":\"799\",\"channel_id\":\"999\"}}").ReplyPreview is { ChannelId: "999", Unavailable: true, Author: null }, "Partial cross-channel reference cannot reuse the old same-channel preview");

        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown }; var window = new TerminalWindow();
        string folder = Path.Combine(AppContext.BaseDirectory, "synthetic-references-data"); Directory.CreateDirectory(folder);
        var paths = new List<string>(); string responseId = "799"; Action? duringRequest = null;
        string? postedFriendUsername = null; int friendPosts = 0; Person? outgoingFriend = null;
        var session = new DiscordSession("SYNTHETIC-NOT-A-REAL-CREDENTIAL", new FakeHttp(request =>
        {
            paths.Add(request.RequestUri!.AbsolutePath + request.RequestUri.Query);
            if (request.RequestUri.AbsolutePath.EndsWith("/users/@me/relationships"))
            {
                if (request.Method == HttpMethod.Post)
                {
                    friendPosts++; var body = J(request.Content!.ReadAsStringAsync().GetAwaiter().GetResult()); postedFriendUsername = Controller.Str(body, "username");
                    return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{}") };
                }
                string bodyJson = outgoingFriend == null ? "[]" : JsonSerializer.Serialize(new[] { new { type = 4, user = new { id = outgoingFriend.Id, username = outgoingFriend.Username, global_name = outgoingFriend.Name } } });
                return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(bodyJson) };
            }
            duringRequest?.Invoke();
            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(new[] { new { id = responseId, channel_id = "700", timestamp = "2026-10-20T00:00:00Z", content = "Fetched original", author = new { id = "101", username = "alice" } } })) };
        }));
        await using var controller = new Controller(window, folder);
        Set(controller, "session", session); Set(controller, "currentChannel", "700");
        var state = new AppSnapshot { SignedIn = true, CurrentUser = new Person("100", "Self"), View = "chat", SelectedChannelId = "700", SelectedChannelKind = "dm", ChannelAccess = ChannelAccess.Private, DirectMessages = new[] { new NavItem("700", "Synthetic DM", "dm") }, Messages = new[] { parsed } };
        Set(controller, "state", state); var cache = (MessageWindowCache)Field(controller, "cache")!;
        var original = new ChatMessage("799", "700", new Person("101", "Alice"), "Original from cache", DateTimeOffset.Parse("2026-10-19T00:00:00Z"), Array.Empty<ChatAttachment>());
        var unavailable = parsed with { ReplyPreview = parsed.ReplyPreview! with { Author = null, Content = "", Unavailable = true } };
        cache.Replace("700", new[] { original, unavailable });
        var resolved = (IReadOnlyList<ChatMessage>)Call(controller, "ReadContextMessages", "700")!;
        Check(resolved.Single(m => m.Id == "800").ReplyPreview is { Content: "Original from cache", Unavailable: false }, "Cached originals resolve missing transport previews without HTTP");
        cache.Upsert(original with { Content = "Edited original" }); resolved = (IReadOnlyList<ChatMessage>)Call(controller, "ReadContextMessages", "700")!;
        Check(resolved.Single(m => m.Id == "800").ReplyPreview!.Content == "Edited original", "Observed original edits update linked preview text");
        Call(controller, "MarkReplyReferencesDeleted", "700", "799"); resolved = (IReadOnlyList<ChatMessage>)Call(controller, "ReadContextMessages", "700")!;
        Check(resolved.Single(m => m.Id == "800").ReplyPreview is { Deleted: true }, "Deleted reference remains deleted even while the source is still cached");
        var small = new MessageWindowCache(); small.Upsert(original); long normalBytes = small.EstimatedTextBytes; small.Upsert(original with { Mentions = bounded, ReplyPreview = parsed.ReplyPreview });
        Check(small.EstimatedTextBytes > normalBytes + 1000, "Mention and reply preview metadata count toward the bounded cache budget");

        cache.Replace("700", new[] { original, parsed }); Set(controller, "state", state);
        Task Jump(string id, string channel = "700") => (Task)Call(controller, "JumpToMessageAsync", new UiAction("jump-message", id, ChannelId: channel), session, 0)!;
        await Jump("799"); var focused = (AppSnapshot)Field(controller, "state")!;
        Check(paths.Count == 0 && focused.FocusMessageId == "799" && focused.FocusMessageVersion == 1, "Cached reply jump focuses the exact source with no redundant HTTP");
        await Jump("799"); Check(((AppSnapshot)Field(controller, "state")!).FocusMessageVersion == 2, "Repeated jumps to the same source have distinct focus revisions");
        await Rejected(() => Jump("799", "999"), "A stale action from another channel cannot navigate this chat");
        await Rejected(() => Jump("777"), "Unknown source ids cannot trigger speculative history lookup");
        cache.Replace("700", new[] { parsed }); Set(controller, "state", state);
        await Jump("799"); focused = (AppSnapshot)Field(controller, "state")!;
        Check(paths.Count == 1 && paths[0].Contains("around=799") && focused.FocusMessageId == "799" && focused.Messages.Any(m => m.Id == "799"), "Out-of-window known reply loads surrounding history then targets the original");
        cache.Replace("700", new[] { parsed }); Set(controller, "state", state);
        duringRequest = () => Call(controller, "RememberLoadingDelta", "MESSAGE_CREATE", J("{\"id\":\"801\",\"channel_id\":\"700\",\"timestamp\":\"2026-10-20T01:00:00Z\",\"content\":\"New event during history lookup\"}"));
        await Jump("799");
        Check(cache.Get("700", "801") != null, "Gateway messages arriving during a reply lookup survive page replacement");
        cache.Replace("700", new[] { parsed }); Set(controller, "state", state);
        duringRequest = () => Call(controller, "RememberLoadingDelta", "MESSAGE_DELETE", J("{\"id\":\"799\",\"channel_id\":\"700\"}"));
        await Rejected(() => Jump("799"), "Original deleted during lookup cannot be resurrected by the delayed page");
        Check(cache.Get("700", "800") != null && cache.Get("700", "799") == null, "A deletion race leaves the user's current conversation intact");
        duringRequest = null;
        cache.Replace("700", new[] { parsed }); Set(controller, "state", state); responseId = "777";
        await Rejected(() => Jump("799"), "Missing original in around result reports unavailable instead of jumping arbitrarily");
        Check(cache.Get("700", "800") != null && cache.Get("700", "777") == null, "Failed source lookup preserves the current cached conversation");
        Set(controller, "state", state with { DirectMessages = Array.Empty<NavItem>(), ChannelAccess = ChannelAccess.Private with { CanReadHistory = false } }); int before = paths.Count;
        await Rejected(() => Jump("799"), "History access is checked again when a reply is clicked");
        Check(paths.Count == before, "Denied reply navigation issues no HTTP request");
        var olderRetained = original with { Id = "650", Timestamp = DateTimeOffset.Parse("2020-01-01T00:00:00Z"), Content = "Older loaded page" };
        responseId = "799"; duringRequest = null; Set(controller, "state", state);
        cache.Replace("700", new[] { olderRetained, original with { Content = "Stale original" }, parsed });
        await (Task)Call(controller, "SelectChannelAsync", "700", false, "")!;
        Check(cache.Get("700", "650") != null && ((AppSnapshot)Field(controller, "state")!).Messages.Any(m => m.Id == "650"), "Revisiting a chat preserves the older loaded page for scroll restoration");
        Check(cache.Get("700", "799")?.Content == "Fetched original", "Latest page refresh overrides stale cached duplicates");
        Set(controller, "state", state); duringRequest = () => Call(controller, "RememberLoadingDelta", "MESSAGE_DELETE", J("{\"id\":\"650\",\"channel_id\":\"700\"}"));
        await (Task)Call(controller, "SelectChannelAsync", "700", false, "")!;
        Check(cache.Get("700", "650") == null, "Latest refresh merge applies concurrent deletes to retained older pages");
        Set(controller, "state", state); responseId = "650";
        cache.Replace("700", Enumerable.Range(1000, 200).Select(i => original with { Id = i.ToString(), Timestamp = DateTimeOffset.Parse("2026-10-20T00:00:00Z").AddMinutes(i), Content = "Recent cached item" }));
        bool initialRestoreWasEmpty = false;
        duringRequest = () => initialRestoreWasEmpty = ((AppSnapshot)Field(controller, "state")!).Messages.Count == 0;
        int restoreStart = paths.Count;
        await (Task)Call(controller, "SelectChannelAsync", "700", false, "650")!;
        Check(paths.Count == restoreStart + 1 && paths[^1].Contains("around=650"), "An evicted scroll anchor restores with one scoped around query rather than latest plus another query");
        Check(initialRestoreWasEmpty, "Missing cached restore anchor leaves the loading snapshot empty until its page arrives");
        Check(cache.Get("700", "650") != null && cache.Count <= 200, "A restored old anchor is retained without increasing the existing cache budget");
        Set(controller, "state", state); responseId = "799"; duringRequest = null;
        cache.Replace("700", new[] { olderRetained }); restoreStart = paths.Count;
        await (Task)Call(controller, "SelectChannelAsync", "700", false, "650")!;
        Check(paths.Count == restoreStart + 2 && paths[restoreStart].Contains("around=650") && !paths[^1].Contains("around="), "Deleted scroll anchor falls back to latest exactly once");
        Check(cache.Get("700", "650") == null && cache.Get("700", "799") != null, "Missing restore anchor is not resurrected from an obsolete cached page");
        cache.Replace("700", Array.Empty<ChatMessage>()); Set(controller, "state", state with { View = "pins", Messages = Array.Empty<ChatMessage>(), PinnedMessages = new[] { parsed } });
        await Jump("799");
        Check(((AppSnapshot)Field(controller, "state")!).FocusMessageId == "799", "A reply preview in pinned messages can navigate to its original outside the current chat page");
        duringRequest = null;
        var friend = new Person("450", "Friend fixture", Username: "trusted.username");
        var people = (Dictionary<string, Person>)Field(controller, "knownPeople")!; people[friend.Id] = friend;
        Set(controller, "state", state); outgoingFriend = friend;
        await (Task)Call(controller, "AddFriendAsync", new UiAction("add-friend", friend.Id, "stale.wrong.name"), session, 0)!;
        Check(friendPosts == 1 && postedFriendUsername == friend.Username, "Profile friend request uses the known person's username rather than stale UI text");
        Check(((AppSnapshot)Field(controller, "state")!).Relationships.Any(r => r.User.Id == friend.Id && r.Type == 4), "Friend request refreshes outgoing relationship state without a Gateway event");
        await Rejected(() => (Task)Call(controller, "AddFriendAsync", new UiAction("add-friend", friend.Id, friend.Username), session, 0)!, "Already-pending friend request is rejected before another POST");
        Check(friendPosts == 1, "A duplicate outgoing request emits no second POST");
        Set(controller, "state", state);
        await Rejected(() => (Task)Call(controller, "AddFriendAsync", new UiAction("add-friend", "100", "self"), session, 0)!, "Self friend requests are rejected at execution");
        people["451"] = new Person("451", "Bot fixture", Username: "bot", IsBot: true);
        await Rejected(() => (Task)Call(controller, "AddFriendAsync", new UiAction("add-friend", "451", "bot"), session, 0)!, "Bot friend requests are rejected at execution");
        Set(controller, "state", state with { Relationships = new[] { new Relationship(friend, 2) } });
        await Rejected(() => (Task)Call(controller, "AddFriendAsync", new UiAction("add-friend", friend.Id, friend.Username), session, 0)!, "Blocked person cannot receive a request through a stale profile button");
        Check(friendPosts == 1, "Denied social actions issue no account mutation requests even in the fake transport");
        Set(controller, "state", state); responseId = "799"; int latestStart = paths.Count;
        await (Task)Call(controller, "ActAsync", new UiAction("latest-messages", Extra: "650", ChannelId: "700"))!;
        Check(paths.Count == latestStart + 1 && !paths[^1].Contains("around="), "Return-to-present action always requests the latest page without a saved anchor");
        await Rejected(() => (Task)Call(controller, "ActAsync", new UiAction("latest-messages", ChannelId: "999"))!, "Return-to-present rejects stale actions from another chat");
        Set(controller, "state", state with { DirectMessages = Array.Empty<NavItem>(), ChannelAccess = ChannelAccess.Private with { CanReadHistory = false } }); latestStart = paths.Count;
        await Rejected(() => (Task)Call(controller, "ActAsync", new UiAction("latest-messages", ChannelId: "700"))!, "Return-to-present rechecks history permission");
        Check(paths.Count == latestStart, "Denied return-to-present performs no request");
        window.Close(); app.Shutdown();
    }
    private sealed class FakeHttp(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
    { protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) => Task.FromResult(respond(request)); }
}
