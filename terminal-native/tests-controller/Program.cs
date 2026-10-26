using System.Reflection;
using System.Net;
using System.Net.Http;
using System.IO;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Windows.Threading;
using NotDiscord.Terminal;

var errors = new List<string>();
int checks = 0;
void Check(bool result, string name) { ++checks; if (!result) errors.Add(name); }
var pendingType = typeof(Controller).GetNestedType("PendingChannelLoad", BindingFlags.NonPublic)!;
var pending = Activator.CreateInstance(pendingType, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance, null, new object[] { "123", 1 }, null)!;
var deltas = (List<(string Type, JsonElement Data)>)pendingType.GetProperty("Deltas")!.GetValue(pending)!;
ChatMessage Message(string id, string content) => new(id, "123", new Person("synthetic", "Synthetic"), content, DateTimeOffset.UnixEpoch.AddSeconds(int.Parse(id)), Array.Empty<ChatAttachment>());
JsonElement Data(object value) => JsonSerializer.SerializeToElement(value);
deltas.Add(("MESSAGE_CREATE", Data(new { id = "3", channel_id = "123", content = "synthetic-created", timestamp = "1970-01-01T00:00:03Z", author = new { id = "synthetic", username = "Synthetic" } })));
deltas.Add(("MESSAGE_UPDATE", Data(new { id = "1", channel_id = "123", content = "synthetic-edited" })));
deltas.Add(("MESSAGE_DELETE", Data(new { id = "2", channel_id = "123" })));
var merge = typeof(Controller).GetMethod("MergeLoadedMessages", BindingFlags.NonPublic | BindingFlags.Static)!;
var merged = ((IEnumerable<ChatMessage>)merge.Invoke(null, new object[] { new[] { Message("1", "synthetic-original"), Message("2", "synthetic-deleted") }, pending })!).ToArray();
Check(merged.Select(x => x.Id).SequenceEqual(new[] { "1", "3" }), "Gateway create/delete preserved across REST snapshot");
Check(merged[0].Content == "synthetic-edited" && merged[0].Edited, "Gateway edit preserved across REST snapshot");
deltas.Add(("MESSAGE_UPDATE", Data(new { id = "1", channel_id = "123", attachments = new[] { new { id = "4", filename = "synthetic.png", url = "https://example.invalid/synthetic.png", width = (int?)null, height = (int?)null, size = (long?)null } } })));
merged = ((IEnumerable<ChatMessage>)merge.Invoke(null, new object[] { new[] { Message("1", "synthetic-original"), Message("2", "synthetic-deleted") }, pending })!).ToArray();
Check(merged[0].Content == "synthetic-edited" && merged[0].Attachments.Count == 1, "Partial update preserves prior content");
Check(merged[0].Attachments[0].Width == 0 && merged[0].Attachments[0].Size == 0, "Nullable attachment metadata accepted");
Check(Controller.Int(Data(new { width = (int?)null }), "width") == 0, "Nullable numeric JSON accepted");
Check(Controller.Int(Data(new { width = "invalid" }), "width") == 0, "Nonnumeric JSON accepted safely");
var group = Controller.ParseChannel(Data(new { id = "300", type = 3, name = "Synthetic group", icon = "group-hash", recipients = new[] { new { id = "101", username = "A", avatar = "avatar-a" }, new { id = "102", username = "B", avatar = "avatar-b" } } }));
Check(group.IconUrl == "https://cdn.discordapp.com/channel-icons/300/group-hash.png?size=64" && group.RecipientAvatars?.Count == 2, "Group icon and bounded participant fallback are preserved");
var callMessage = Controller.ParseMessage(Data(new { id = "4", channel_id = "123", type = 3, content = "", timestamp = "2026-10-18T10:00:00Z", call = new { participants = new[] { "101", "102" }, ended_timestamp = "2026-10-18T10:01:35Z" } }));
Check(callMessage.MessageType == 3 && callMessage.Content.Contains("1:35") && callMessage.CallParticipantIds?.Count == 2 && !callMessage.Edited, "Call log uses real start/end duration and participants");
var update = typeof(Controller).GetMethod("ApplyMessageUpdate", BindingFlags.NonPublic | BindingFlags.Static)!;
var withFile = Message("5", "https://tenor.com/view/synthetic-gif-123") with { Attachments = new[] { new ChatAttachment("file-1", "fixture.png", "https://cdn.discordapp.com/attachments/1/fixture.png", "image/png") } };
var embedUpdate = Data(new { embeds = new[] { new { type = "gifv", url = "https://tenor.com/view/synthetic-gif-123", thumbnail = new { url = "https://media.tenor.com/synthetic/thumbnail.png", proxy_url = "https://media.discordapp.net/external/synthetic.png", width = 128, height = 128 } } } });
var updatedEmbed = (ChatMessage)update.Invoke(null, new object[] { withFile, embedUpdate })!;
Check(updatedEmbed.Attachments.Count == 2 && updatedEmbed.Attachments[0].Id == "file-1" && updatedEmbed.Attachments[1].Url == withFile.Content && updatedEmbed.Attachments[1].Animated, "Late GIF embed preserves file and exact share-page source");
Check(!updatedEmbed.Edited && updatedEmbed.Content == withFile.Content, "Embed-only update never invents text edit");
var gifAttachment = Controller.ParseMessage(Data(new { id = "6", channel_id = "123", attachments = new[] { new { id = "7", filename = "clip.gif", url = "https://cdn.discordapp.com/attachments/1/clip.gif", proxy_url = "https://media.discordapp.net/attachments/1/clip.gif?format=webp", content_type = "image/gif" } } }));
Check(gifAttachment.Attachments[0].Animated && gifAttachment.Attachments[0].Url == "https://cdn.discordapp.com/attachments/1/clip.gif", "Original GIF preferred over a converted WebP proxy");
var convertedOnly = Controller.ParseMessage(Data(new { id = "6", channel_id = "123", attachments = new[] { new { id = "7", filename = "clip.gif", url = "https://cdn.discordapp.com/attachments/1/clip.gif?format=webp", content_type = "image/gif" } } }));
Check(!convertedOnly.Attachments[0].Animated && convertedOnly.Attachments[0].ContentType != "image/gif", "Converted image not falsely classified as GIF bytes");
var callUpdate = (ChatMessage)update.Invoke(null, new object[] { callMessage with { CallEndedAt = null }, Data(new { call = new { ended_timestamp = "2026-10-18T10:03:00Z" } }) })!;
Check(callUpdate.Content.Contains("3:00") && !callUpdate.Edited && callUpdate.CallParticipantIds?.Count == 2, "Call completion updates log without a false edit");
var searchContext = new SearchContext("100", new[] { new Person("100", "Yo"), new Person("101", "Ana"), new Person("102", "Persona duplicada"), new Person("103", "Persona duplicada") },
    new[] { new NavItem("200", "general", "text"), new NavItem("201", "arte y fotos", "text") });
var query = SearchQuery.Parse("hola from:Ana in:general has:image before:2026-10-18 after:2026-10-01 pinned:true", searchContext);
Check(query.Content == "hola" && query.Filters.Contains(new("author_id", "101")) && query.Filters.Contains(new("channel_id", "200")) && query.Filters.Contains(new("has", "image")), "Search resolves known names and tags");
Check(query.QueryString.Contains("pinned=true") && query.QueryString.Contains("max_id=") && query.QueryString.Contains("min_id="), "Search date filters converted to bounds");
Check(SearchQuery.Parse("from:me in:\"arte y fotos\" texto", searchContext).Filters.Contains(new("channel_id", "201")), "Quoted channel and own-user filter");
Check(SearchQuery.Parse("from:<@101> in:<#200> a&b=1", searchContext).QueryString.EndsWith("content=a%26b%3D1"), "Mentions and query escaping");
Check(SearchQuery.Parse("https://example.invalid/a..b", searchContext).Content == "https://example.invalid/a..b", "URL remains ordinary search content");
var mentionSearch = SearchQuery.Parse("mentions:Ana during:2026-10 has:forward", searchContext);
Check(mentionSearch.Filters.Contains(new("mentions", "101")) && mentionSearch.Filters.Contains(new("has", "snapshot")) && mentionSearch.Filters.Any(f => f.Key == "min_id") && mentionSearch.Filters.Any(f => f.Key == "max_id"), "Mentions, month range and forwarded-message filters are translated");
Check(SearchQuery.Parse("from: Ana", searchContext).Filters.Contains(new("author_id", "101")), "Whitespace after a search tag is accepted");
var authorSearch = SearchQuery.Parse("texto author:bot", searchContext);
Check(authorSearch.AuthorType == "bot" && !authorSearch.QueryString.Contains("author_type") && authorSearch.Matches(Message("1", "text") with { Author = new Person("100", "Bot", IsBot: true) }) && !authorSearch.Matches(Message("1", "text")), "Author type is an explicit local page filter, never an unverified server parameter");
Check(SearchQuery.Parse("author:webhook", searchContext).Matches(Message("1", "text") with { WebhookId = "123" }), "Webhook filter distinguishes bot-user messages");
var richPresence = Controller.ParsePresenceDetails(Data(new { status = "online", activities = new object[] {
    new { type = 4, name = "Custom Status", state = "Synthetic custom", emoji = new { id = "700", name = "wave", animated = true } },
    new { type = 2, name = "Spotify", details = "Synthetic song", state = "Synthetic artist", assets = new { large_image = "spotify:synthetic" }, timestamps = new { start = 1700000000000L, end = 1700000240000L } },
    new { type = 0, name = "Synthetic game", details = "Synthetic map", application_id = "800" } } }));
Check(richPresence.CustomStatus == "Synthetic custom" && richPresence.CustomStatusEmoji?.ImageUrl?.Contains("700.gif") == true && richPresence.Activities.Count == 2, "Custom text and server emoji remain separate from all real activities");
Check(richPresence.Activities[0].Details == "Synthetic song" && richPresence.Activities[0].State == "Synthetic artist" && richPresence.Activities[0].LargeImageUrl == "https://i.scdn.co/image/synthetic" && richPresence.Activities[0].StartedAt != null, "Spotify track, artist, art and timestamps survive parsing");
Check(Controller.ParsePresenceDetails(Data(new { activities = new[] { new { type = 0, name = "Synthetic", timestamps = new { start = "invalid" } } } })).Activities[0].StartedAt == null, "Malformed activity timestamps do not terminate the Gateway reducer");
var readyProjection = typeof(DiscordSession).GetMethod("ReadyMetadata", BindingFlags.Static | BindingFlags.NonPublic)!;
var normalizedPresence = (JsonElement)readyProjection.Invoke(null, new object[] { Data(new { merged_presences = new { friends = new[] { new { user_id = "101", status = "online", activities = new object[] {
    new { type = 4, name = "Custom Status", state = "Synthetic startup status", emoji = new { name = "😀" } },
    new { type = 0, name = "Synthetic startup game" } } } } } }) })!;
try
{
    var normalizedDetails = Controller.ParsePresenceDetails(normalizedPresence.GetProperty("presences")[0]);
    Check(normalizedDetails.CustomStatus == "Synthetic startup status" && normalizedDetails.Activities.Count == 1 && normalizedDetails.Activities[0].StartedAt == null, "READY projection and rich presence parser interoperate with absent activity timestamps");
}
catch (InvalidOperationException)
{ errors.Add("READY projection introduces null timestamps that terminate the rich presence bootstrap reducer"); }
Check(Controller.ParsePresenceDetails(Data(new { activities = new[] { new { type = 0, name = "Synthetic", timestamps = (object?)null } } })).Activities[0].StartedAt == null, "Explicitly null activity timestamps are harmless");
Check(Controller.ParsePresenceDetails(Data(new { activities = new object?[] { null, new { type = 4, state = "Nullable fields", emoji = (object?)null, assets = (object?)null, timestamps = (object?)null } } })).CustomStatus == "Nullable fields", "Null optional activity structs and invalid array entries cannot abort the presence batch");
var personDetails = Controller.ParsePerson(Data(new { id = "175928847299117063", username = "synthetic_user", global_name = "Synthetic display", avatar = "avatar", bot = false }));
Check(personDetails.Name == "Synthetic display" && personDetails.DisplayName == "Synthetic display" && personDetails.Username == "synthetic_user" && personDetails.AvatarUrl?.Contains("/175928847299117063/avatar.png") == true, "Display name, username and real message avatar remain available independently");
var friendRelation = Controller.ParseRelationship(Data(new { type = 1, since = "2024-10-13T12:00:00Z", user = new { id = "175928847299117063", username = "synthetic_user" } }));
Check(friendRelation.Since?.Year == 2024 && Controller.ParseRelationship(Data(new { type = 3, since = "2024-10-13T12:00:00Z", user = new { id = "101", username = "pending" } })).Since == null, "Friendship date is not fabricated for pending or nonfriend relationships");
var profileFallback = new ProfileDetails(personDetails, Controller.AccountCreatedAt(personDetails.Id), friendRelation.Since, "", richPresence.Status, richPresence.CustomStatus, richPresence.CustomStatusEmoji, richPresence.Activities, Array.Empty<Person>(), Array.Empty<NavItem>());
var profileDetails = Controller.ParseProfileDetails(Data(new { user = new { id = "175928847299117063", username = "synthetic_user", global_name = "Synthetic display", avatar = "avatar" }, user_profile = new { bio = "Synthetic bio" }, mutual_friends = new[] { new { id = "101", username = "mutual" } }, mutual_guilds = new[] { new { id = "500" } } }), profileFallback, new[] { new NavItem("500", "Known synthetic guild", "guild", IconUrl: "https://cdn.discordapp.com/icons/500/hash.png") }, Array.Empty<Person>());
Check(profileDetails.CreatedAt?.Year == 2016 && profileDetails.FriendsSince?.Year == 2024 && profileDetails.Bio == "Synthetic bio" && profileDetails.MutualFriends.Count == 1 && profileDetails.MutualGuilds[0].Name == "Known synthetic guild", "Profile merges real friendship, snowflake creation, public bio and actual mutual lists");
var mutualGuildAlias = Controller.ParseProfileDetails(Data(new { mutual_guilds = new[] { new { guild_id = "500" } } }), profileFallback, new[] { new NavItem("500", "Known icon", "guild", IconUrl: "https://cdn.discordapp.com/icons/500/known.png?size=64") }, Array.Empty<Person>());
Check(mutualGuildAlias.MutualGuilds.Single().IconUrl == "https://cdn.discordapp.com/icons/500/known.png?size=64", "Mutual guild_id entries reuse known server icons without extra network requests");
Check(Controller.AccountCreatedAt("invalid") == null && !Controller.ParseProfileDetails(Data(new { }), profileFallback, Array.Empty<NavItem>(), Array.Empty<Person>()).MutualFriendsAvailable, "Unavailable profile fields are distinct from a confirmed empty list");
var forwardPayload = JsonSerializer.SerializeToElement(Controller.ForwardPayload(Message("1", "private synthetic body"), "500"));
Check(forwardPayload.GetProperty("message_reference").GetProperty("type").GetInt32() == 1 && !forwardPayload.TryGetProperty("content", out _) && forwardPayload.GetProperty("allowed_mentions").GetProperty("parse").GetArrayLength() == 0, "Forwarding uses the native source reference without copying contents or allowing unexpected pings");
var snapshotMessage = Controller.ParseMessage(Data(new { id = "8", channel_id = "123", type = 0, content = "", author = new { id = "101", username = "Forwarder" }, message_reference = new { type = 1, message_id = "1", channel_id = "200" }, message_snapshots = new[] { new { message = new { content = "Synthetic forwarded content", attachments = new[] { new { id = "300", filename = "fixture.png", url = "https://cdn.discordapp.com/attachments/1/fixture.png", content_type = "image/png" } } } } } }));
Check(snapshotMessage.Forwarded && snapshotMessage.ReplyTo == null && snapshotMessage.Content == "Synthetic forwarded content" && snapshotMessage.Attachments.Count == 1 && snapshotMessage.Author.Name == "Forwarder", "Received forward snapshot renders content and attachments under the forwarding author without being mislabeled as a reply");
foreach (string invalid in new[] { "from:", "from:Desconocido", "from:\"Persona duplicada\"", "before:2026-02-30", "after:2026-10-20 before:2026-10-01", "has:unknown", "pinned:yes", "pinned:true pinned:false" })
{
    try { SearchQuery.Parse(invalid, searchContext); errors.Add("Invalid search accepted: " + invalid); } catch (ArgumentException) { }
}
var sorted = NavigationOrder.DirectMessages(new[] {
    new NavItem("3", "Alfabético primero", "dm", LastMessageId: "100"),
    new NavItem("2", "Reciente", "dm", LastMessageId: "900"),
    new NavItem("1", "Fijado", "dm", IsPinned: true, LastMessageId: "10") }).ToArray();
Check(sorted.Select(x => x.Id).SequenceEqual(new[] { "1", "2", "3" }), "DM pinned first then most recent message, not alphabetic");
Check(NavigationOrder.DirectMessages(sorted.Append(new NavItem("4", "Incoming call", "dm", HasActiveCall: true, IncomingCallRinging: true))).First().Id == "4", "An incoming call has priority even above pinned conversations");
var guild = Data(new { id = "500", owner_id = "999", roles = new[] { new { id = "500", permissions = "1025" }, new { id = "501", permissions = "32" } } });
var member = Data(new { roles = new[] { "501" } });
var access = GuildAccess.From(guild, member, "100");
Check(GuildAccess.From(Data(new { }), Data(new { }), "").BasePermissions == 0, "Missing owner/user metadata never grants owner permissions");
Check(access.BasePermissions == 1057, "Guild permissions combine everyone and own roles");
var overwritten = Data(new { permission_overwrites = new[] { new { id = "500", type = 0, allow = "0", deny = "1" }, new { id = "501", type = 0, allow = "1", deny = "0" } } });
Check((access.ChannelPermissions(overwritten) & 1) != 0, "Role allow applied after everyone deny");
var denied = Data(new { permission_overwrites = new[] { new { id = "501", type = 0, allow = "1", deny = "0" }, new { id = "100", type = 1, allow = "0", deny = "1" } } });
Check((access.ChannelPermissions(denied) & 1) == 0, "Member deny overrides role allow");
foreach (object? invalidType in new object?[] { null, "0", 2 })
{
    var malformed = Data(new { permission_overwrites = new[] { new { id = "501", type = invalidType, allow = "8", deny = "0" } } });
    Check(access.ChannelPermissions(malformed) == access.BasePermissions, "Malformed overwrite type never grants role permission");
}
Check(access.ChannelPermissions(Data(new { permission_overwrites = new[] { new { id = "501", allow = "8", deny = "0" } } })) == access.BasePermissions, "Missing overwrite type never grants role permission");
Check(GuildAccess.From(guild, member, "999").ChannelPermissions(denied) == ulong.MaxValue, "Owner bypasses channel overwrites");
var adminGuild = Data(new { id = "500", owner_id = "999", roles = new[] { new { id = "500", permissions = "0" }, new { id = "501", permissions = "8" } } });
Check(GuildAccess.From(adminGuild, member, "100").ChannelPermissions(denied) == ulong.MaxValue, "Administrator bypasses channel overwrites");
// Exercise real unread reducers without starting the app, account, network or a visible window.
var unreadController = (Controller)RuntimeHelpers.GetUninitializedObject(typeof(Controller));
void SetField(string name, object value) => typeof(Controller).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(unreadController, value);
object? Invoke(string name, params object[] arguments) => typeof(Controller).GetMethod(name, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(unreadController, arguments);
AppSnapshot UnreadState() => (AppSnapshot)typeof(Controller).GetField("state", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(unreadController)!;
var readMarkers = new Dictionary<string, string>(); var unreadGuilds = new Dictionary<string, string>();
SetField("state", new AppSnapshot { CurrentUser = new Person("100", "Synthetic self"), SelectedChannelId = "200", DirectMessages = new[] { new NavItem("200", "Synthetic DM", "dm", LastMessageId: "10") } });
SetField("currentGuild", ""); SetField("currentChannel", "200"); SetField("readMessageIds", readMarkers); SetField("unreadGuilds", unreadGuilds);
SetField("latestObservedMessages", new Dictionary<string, string>()); SetField("mentionCounts", new Dictionary<string, int>());
SetField("mutedChannels", new Dictionary<string, bool>()); SetField("mutedGuilds", new Dictionary<string, bool>());
SetField("activeCalls", new Dictionary<string, IncomingCall>()); SetField("notifiedMessageIds", new HashSet<string>()); SetField("notifiedMessageOrder", new Queue<string>()); SetField("notifiedCalls", new HashSet<string>());
SetField("privateChannelLoads", new Dictionary<string, Task>()); SetField("privateChannelFailures", new Dictionary<string, DateTimeOffset>()); SetField("groupRecipients", new Dictionary<string, (Person[] People, string Owner)>());
var syntheticFeatures = new NativeFeatures(Path.Combine(AppContext.BaseDirectory, "synthetic-social-data")); SetField("features", syntheticFeatures); SetField("cache", new MessageWindowCache());
var knownPresences = new Dictionary<string, (string Status, string Activity)>(); SetField("presences", knownPresences); SetField("knownPeople", new Dictionary<string, Person>()); SetField("richPresences", new Dictionary<string, UserPresence>());
Invoke("ApplyPresence", Data(new { status = "dnd" }), false, "101");
Check(knownPresences.TryGetValue("101", out var knownPresence) && knownPresence.Status == "dnd", "Member-list presence without an embedded ID uses the enclosing member user ID");
Invoke("ApplyPresence", normalizedPresence.GetProperty("presences")[0], false, "");
Check(knownPresences["101"].Status == "online" && UnreadState().UserPresences["101"].CustomStatus == "Synthetic startup status", "Full READY projection reaches the controller presence store before incremental Gateway events");
var noUiRedraw = new DispatcherTimer(); SetField("redraw", noUiRedraw);
var notices = new List<DesktopNotice>(); unreadController.DesktopNoticeRequested += notices.Add;
Invoke("NoteIncomingMessage", Data(new { id = "11", channel_id = "200", author = new { id = "100", username = "Synthetic self" } }));
Check(!UnreadState().DirectMessages[0].Unread && UnreadState().UnreadMessageCount == 0, "Own message from another client never creates an unread badge");
Check(readMarkers["200"] == "11", "Own last-message metadata cannot recreate a new unread badge during sidebar decoration");
Invoke("NoteIncomingMessage", Data(new { id = "12", channel_id = "200", author = new { id = "101", username = "Synthetic peer" }, mentions = new[] { new { id = "100" } } }));
Check(UnreadState().DirectMessages[0].Unread && UnreadState().DirectMessages[0].MentionCount == 1 && UnreadState().NewMessageNotice.Length > 0, "Selected channel still records incoming unread and mention without assuming it was viewed");
Invoke("NoteIncomingMessage", Data(new { id = "12", channel_id = "200", author = new { id = "101", username = "Synthetic peer" }, mentions = new[] { new { id = "100" } } }));
Check(notices.Count == 1 && notices[0].ChannelId == "200" && UnreadState().DirectMessages[0].MentionCount == 1, "A replayed private message never duplicates its desktop candidate or mention count");
Invoke("RememberRead", "200", "12"); Invoke("RememberRead", "200", "10");
Check(readMarkers["200"] == "12", "Read marker remains monotonic after a stale acknowledgement");
Invoke("NoteIncomingMessage", Data(new { id = "40", channel_id = "500", guild_id = "600", author = new { id = "101", username = "Synthetic peer" } }));
Invoke("ClearUnread", "500", "35");
Check(unreadGuilds.ContainsKey("500"), "Stale ACK cannot clear newer messages in an unloaded server channel");
Invoke("ClearUnread", "500", "40");
Check(!unreadGuilds.ContainsKey("500"), "Current ACK clears the unloaded channel badge");
Check(notices.Count == 1, "Ordinary server messages without a mention do not offer desktop notifications");
Invoke("NoteIncomingMessage", Data(new { id = "41", channel_id = "500", guild_id = "600", author = new { id = "101", username = "Synthetic peer" }, mentions = new[] { new { id = "100" } } }));
Check(notices.Count == 2 && notices[^1].ChannelId == "500", "A server mention offers a desktop notice candidate");
var mutedTestChannels = new Dictionary<string, bool> { ["700"] = true }; SetField("mutedChannels", mutedTestChannels);
Invoke("NoteIncomingMessage", Data(new { id = "42", channel_id = "700", author = new { id = "101", username = "Synthetic peer" } }));
Check(notices.Count == 2, "Muted private conversations remain unread without offering a desktop notice");
int readRequests = 0;
await using (var incomingSession = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new SyntheticHandler(request =>
{
    ++readRequests;
    return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(new { id = "800", type = 1, last_message_id = "1", recipients = new[] { new { id = "101", username = "Synthetic new DM", avatar = "avatar" } } })) };
})))
{
    SetField("session", incomingSession); SetField("sessionWork", new CancellationTokenSource());
    Invoke("NoteIncomingMessage", Data(new { id = "1000", channel_id = "800", author = new { id = "101", username = "Synthetic new DM" } }));
    await (Task)Invoke("EnsureIncomingPrivateChannelAsync", "800", incomingSession, 0)!;
    var hydrated = UnreadState().DirectMessages.Single(n => n.Id == "800");
    Check(hydrated.Unread && hydrated.LastMessageId == "1000" && hydrated.Name == "Synthetic new DM" && readRequests == 1, "New private conversations are hydrated on demand without losing the arriving unread or last-message ID");
    await (Task)Invoke("EnsureIncomingPrivateChannelAsync", "800", incomingSession, 0)!;
    Check(readRequests == 1, "Known private conversations cause no repeated metadata fetches");
    var callData = Data(new { channel_id = "200", message_id = "1001", ringing = new[] { "100" }, voice_states = new[] { new { user_id = "101" } } });
    int beforeCallNotices = notices.Count;
    Invoke("ObserveIncomingCallDispatch", "CALL_CREATE", callData, incomingSession, 0);
    Invoke("ObserveIncomingCallDispatch", "CALL_UPDATE", callData, incomingSession, 0);
    Check(UnreadState().IncomingCalls.Single().Ringing && UnreadState().DirectMessages.First().Id == "200" && notices.Count == beforeCallNotices + 1 && notices[^1].IsCall, "Incoming calls are prioritized with one notification across repeated create/update events");
    Invoke("ClearUnread", "200", "1001");
    Check(!UnreadState().IncomingCalls.Single().Unread && UnreadState().IncomingCalls.Single().Ringing && UnreadState().DirectMessages.Single(n => n.Id == "200").HasActiveCall, "Reading the current call conversation clears only its unread count while preserving active ringing");
    Invoke("DismissIncomingCall", "200");
    Invoke("ObserveIncomingCallDispatch", "CALL_UPDATE", callData, incomingSession, 0);
    Check(!UnreadState().IncomingCalls.Single().Unread && notices.Count == beforeCallNotices + 1, "Dismissing a call stays local and does not cause notification spam on updates");
    Invoke("ObserveIncomingCallDispatch", "CALL_DELETE", Data(new { channel_id = "200" }), incomingSession, 0);
    Check(UnreadState().IncomingCalls.Count == 0 && !UnreadState().DirectMessages.Any(n => n.HasActiveCall), "Call deletion removes the incoming indicator and call-priority state");
    SetField("channelLoad", new CancellationTokenSource()); SetField("state", UnreadState() with { View = "pins", SelectedChannelId = "200" });
    bool toggled = await (Task<bool>)Invoke("TryHandleSocialActionAsync", new UiAction("pins", ChannelId: "200"), incomingSession, 0)!;
    Check(toggled && UnreadState().View == "chat" && UnreadState().SelectedChannelId == "200" && readRequests == 1, "Pressing pins again restores the same cached chat without an extra API request");
    SetField("state", UnreadState() with { SelfPresence = "invisible" });
    var ownProfile = (ProfileDetails)Invoke("KnownProfile", UnreadState().CurrentUser!)!;
    Check(ownProfile.User.Id == "100" && ownProfile.Presence == "invisible", "The own profile uses the current account and preserves its selected presence");
}
SetField("typingPeople", new Dictionary<string, DateTimeOffset>()); SetField("currentGuild", "900");
SetField("state", UnreadState() with { View = "chat", Guilds = new[] { new NavItem("600", "Synthetic notified guild", "guild") }, Channels = Array.Empty<NavItem>(), SelectedGuildId = "900" });
await using (var navigationSession = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new SyntheticHandler(request =>
{
    string path = request.RequestUri!.AbsolutePath;
    object body = path switch
    {
        "/api/v9/channels/850" => new { id = "850", type = 0, guild_id = "600", name = "notified-channel" },
        "/api/v9/guilds/600/channels" => new[] { new { id = "850", type = 0, name = "notified-channel" } },
        "/api/v9/guilds/600" => new { id = "600", owner_id = "999", roles = new[] { new { id = "600", permissions = "3072" } } },
        "/api/v9/guilds/600/members/100" => new { roles = Array.Empty<string>() },
        "/api/v9/guilds/600/emojis" => Array.Empty<object>(),
        "/api/v9/channels/850/messages" => new[] { new { id = "1002", channel_id = "850", type = 0, content = "Synthetic notification target", timestamp = "2026-10-19T12:00:00Z", author = new { id = "101", username = "Synthetic peer" } } },
        _ => throw new InvalidOperationException("Unexpected synthetic notification route")
    };
    return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(body)) };
})))
{
    SetField("session", navigationSession);
    await (Task)Invoke("OpenNotificationChannelAsync", "850", navigationSession, 0)!;
    Check(UnreadState().SelectedGuildId == "600" && UnreadState().SelectedChannelId == "850" && UnreadState().Messages.Single().Content == "Synthetic notification target", "Opening a notification hydrates and selects its actual guild before the channel");
}
int raceRequests = 0;
await using (var raceSession = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new SyntheticHandler(request =>
{
    ++raceRequests;
    int navigation = (int)typeof(Controller).GetField("navigationVersion", BindingFlags.NonPublic | BindingFlags.Instance)!.GetValue(unreadController)!;
    SetField("navigationVersion", navigation + 1); SetField("currentChannel", "999"); SetField("state", UnreadState() with { SelectedChannelId = "999", ChannelName = "User-chosen channel" });
    return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{\"id\":\"860\",\"type\":0,\"guild_id\":\"600\",\"name\":\"Obsolete target\"}") };
})))
{
    SetField("session", raceSession);
    await (Task)Invoke("OpenNotificationChannelAsync", "860", raceSession, 0)!;
    Check(UnreadState().SelectedChannelId == "999" && raceRequests == 1, "A delayed notification lookup cannot overwrite a newer user navigation");
}
await syntheticFeatures.DisposeAsync();
noUiRedraw.Stop();
Console.WriteLine(JsonSerializer.Serialize(new { success = errors.Count == 0, checks, errors, syntheticDataOnly = true, realNetworkUsed = false }, new JsonSerializerOptions { WriteIndented = true }));
return errors.Count == 0 ? 0 : 1;

sealed class SyntheticHandler(Func<HttpRequestMessage, HttpResponseMessage> response) : HttpMessageHandler
{
    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) => Task.FromResult(response(request));
}
