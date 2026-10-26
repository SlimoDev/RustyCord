using System.Net;
using System.Net.Http;
using System.Reflection;
using System.Text.Json;
using NotDiscord.Terminal;

var errors = new List<string>();
var results = new List<object>();
void Check(bool condition, string name, object? evidence = null)
{
    results.Add(new { test = name, passed = condition, evidence });
    if (!condition) errors.Add(name);
}

// Entirely local HttpMessageHandler: no network or real credentials.
string? observedHost = null;
Check(Convert.ToHexString(Convert.FromBase64String(DiscordUserSettings.VisibilityDelta(false))) == "5A021A00", "Visibility false writes only observed status/showCurrentGame protobuf field");
Check(DiscordUserSettings.Read(DiscordUserSettings.VisibilityDelta(true), false).ShowCurrentGame == true && DiscordUserSettings.Read(DiscordUserSettings.VisibilityDelta(false), false).ShowCurrentGame == false, "Visibility delta roundtrip true and false");
Check(DiscordUserSettings.Read("", true).ShowCurrentGame == true && DiscordUserSettings.Read("", false).ShowCurrentGame == null, "Full defaults and partial missing fields remain distinct");
var bothSettings = DiscordUserSettings.Read(Convert.ToBase64String(Convert.FromHexString("5A0C0A060A0469646C651A020801")), true);
Check(bothSettings.Status == "idle" && bothSettings.ShowCurrentGame == true, "Observed nested status and BoolValue decode independently");
foreach (string invalidProto in new[] { "invalid!", Convert.ToBase64String(new byte[] { 0x5a, 20, 1 }), Convert.ToBase64String(new byte[] { 0 }) })
{
    try { DiscordUserSettings.Read(invalidProto, true); errors.Add("Malformed protobuf accepted"); } catch (ArgumentException) { }
}
bool simulatedVisibility = true; int writes = 0; string? writtenSettings = null;
await using (var visibilitySession = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new FakeHandler(request =>
{
    if (request.Method == HttpMethod.Patch)
    {
        ++writes;
        using var body = JsonDocument.Parse(request.Content!.ReadAsStringAsync().GetAwaiter().GetResult());
        writtenSettings = body.RootElement.GetProperty("settings").GetString();
        simulatedVisibility = DiscordUserSettings.Read(writtenSettings!, false).ShowCurrentGame!.Value;
        Check(body.RootElement.EnumerateObject().Count() == 1, "Visibility write does not include unrelated account settings");
    }
    return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(new { settings = DiscordUserSettings.VisibilityDelta(simulatedVisibility) })) };
})))
{
    await visibilitySession.RefreshActivityVisibilityAsync();
    Check(visibilitySession.ShowCurrentGame == true && writes == 0, "Initial visibility is read without any account mutation");
    await visibilitySession.SetActivityVisibilityAsync(false);
    Check(visibilitySession.ShowCurrentGame == false && writes == 1 && writtenSettings == DiscordUserSettings.VisibilityDelta(false), "Explicit visibility toggle verifies remote readback");
    var protoEvent = typeof(DiscordSession).GetMethod("RememberProtoEvent", BindingFlags.NonPublic | BindingFlags.Instance)!;
    protoEvent.Invoke(visibilitySession, new object[] { JsonSerializer.SerializeToElement(new { settings = new { type = 1, proto = DiscordUserSettings.VisibilityDelta(true) }, partial = true }) });
    Check(visibilitySession.ShowCurrentGame == true, "Original nested settings.proto Gateway event is applied");
    protoEvent.Invoke(visibilitySession, new object[] { JsonSerializer.SerializeToElement(new { settings = new { type = 1, proto = "" }, partial = true }) });
    Check(visibilitySession.ShowCurrentGame == true, "Unrelated partial Gateway settings do not change visibility");
}
await using (var mismatchSession = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new FakeHandler(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(new { settings = DiscordUserSettings.VisibilityDelta(true) })) })))
{
    try { await mismatchSession.SetActivityVisibilityAsync(false); errors.Add("Unverified settings write reported success"); }
    catch (ServiceException error) { Check(error.Message.Contains("no confirmó") && mismatchSession.ShowCurrentGame == true, "Readback mismatch reports failure and retains server state"); }
}
var readyMetadata = typeof(DiscordSession).GetMethod("ReadyMetadata", BindingFlags.Static | BindingFlags.NonPublic)!;
var metadata = (JsonElement)readyMetadata.Invoke(null, new object[] { JsonSerializer.SerializeToElement(new { merged_presences = new { friends = new[] { new { user_id = "101", status = "online" } }, guilds = new[] { new[] { new { user_id = "102", status = "dnd" } } } }, read_state = new { entries = new[] { new { id = "123", last_message_id = "8", mention_count = 2 } } } }) })!;
Check(metadata.GetProperty("presences").GetArrayLength() == 2 && metadata.GetProperty("read_states")[0].GetProperty("mention_count").GetInt32() == 2, "READY merged friend/guild presences and unread metadata are projected");
var emptyMergedMetadata = (JsonElement)readyMetadata.Invoke(null, new object[] { JsonSerializer.SerializeToElement(new { merged_presences = (object?)null }) })!;
Check(emptyMergedMetadata.GetProperty("presences").GetArrayLength() == 0, "Null optional merged-presence metadata remains an empty projection");
foreach (string requestedStatus in new[] { "online", "idle", "dnd", "invisible" })
{
    await using var presenceSession = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new FakeHandler(_ =>
        new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(new { status = requestedStatus })) }));
    await presenceSession.InitializePresenceAsync();
    Check(presenceSession.SelfPresence == requestedStatus, "Initial presence preserves " + requestedStatus);
    var payload = typeof(DiscordSession).GetMethod("PresencePayload", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, new object?[] { presenceSession.SelfPresence, "Synthetic game" });
    var json = JsonSerializer.SerializeToElement(payload).GetProperty("d");
    Check(json.GetProperty("status").GetString() == requestedStatus && json.GetProperty("activities")[0].GetProperty("name").GetString() == "Synthetic game", "Activity payload preserves " + requestedStatus);
}
await using (var unknownSession = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new FakeHandler(_ =>
    new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{\"status\":\"unexpected\"}") })))
{
    await unknownSession.InitializePresenceAsync();
    Check(unknownSession.SelfPresence.Length == 0, "Unknown presence never becomes online by default");
    try { await unknownSession.SetActivityAsync("Synthetic game"); errors.Add("Activity published with unknown presence"); }
    catch (ServiceException error) { Check(error.Message.Contains("estado de presencia"), "Unknown status blocks activity before any Gateway write"); }
}
await using (var session = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new FakeHandler(request =>
{
    observedHost = request.RequestUri!.Host;
    return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{}") };
})))
{
    try { await session.GetAsync("https:/\\test.invalid/path"); } catch (ArgumentException) { }
    Check(observedHost is null or "discord.com", "REST rejects mixed-slash external URI", new { observedHost });
}

await using (var session = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new FakeHandler(_ =>
    new HttpResponseMessage(HttpStatusCode.Unauthorized) { Content = new StringContent("{\"private\":\"SYNTHETIC-PRIVATE-BODY\"}") })))
{
    try { await session.GetAsync("users/@me"); errors.Add("Unauthorized response accepted"); }
    catch (ServiceException error) { Check(error.Status == 401 && !error.Message.Contains("SYNTHETIC-PRIVATE-BODY"), "Private HTTP body not exposed in error"); }
}

var getBody = new GeneratedContent(24 * 1024 * 1024);
await using (var session = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new FakeHandler(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = getBody })))
{
    try { await session.GetAsync("channels/123/messages"); errors.Add("Oversized GET response accepted"); } catch (ServiceException) { }
    Check(getBody.Emitted <= 16 * 1024 * 1024 + 32768, "GET enforces streamed response bound", new { bytesRead = getBody.Emitted });
}

string data = args.Length > 0 ? Path.GetFullPath(args[0]) : Path.Combine(AppContext.BaseDirectory, "synthetic-data");
Directory.CreateDirectory(data);
string syntheticFile = Path.Combine(data, "synthetic-upload.txt");
await File.WriteAllTextAsync(syntheticFile, "synthetic fixture only");
var uploadBody = new GeneratedContent(24 * 1024 * 1024);
await using (var session = new DiscordSession("SYNTHETIC-TEST-CREDENTIAL-NOT-REAL", new FakeHandler(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = uploadBody })))
{
    try { await session.UploadAsync("123", syntheticFile, "synthetic"); errors.Add("Oversized upload response accepted"); } catch (ServiceException) { }
    Check(uploadBody.Emitted <= 1024 * 1024 + 32768, "Upload enforces streamed response bound", new { bytesRead = uploadBody.Emitted });
}

var richReadyMethod = typeof(DiscordSession).GetMethod("ReadyMetadata", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Static)!;
var richReady = (JsonElement)richReadyMethod.Invoke(null, new object[] { JsonSerializer.SerializeToElement(new { merged_presences = new { friends = new[] { new { user_id = "101", status = "online", activities = new object[] {
    new { type = 4, name = "Custom Status", state = "Synthetic text", emoji = new { id = "700", name = "wave", animated = true } },
    new { type = 2, name = "Spotify", details = "Synthetic song", state = "Synthetic artist", assets = new { large_image = "spotify:synthetic" }, timestamps = new { start = 1700000000000L } } } } } } }) })!;
var readyActivities = richReady.GetProperty("presences")[0].GetProperty("activities");
Check(readyActivities.GetArrayLength() == 2 && readyActivities[0].GetProperty("emoji").GetProperty("id").GetString() == "700" && readyActivities[1].GetProperty("details").GetString() == "Synthetic song" && readyActivities[1].GetProperty("timestamps").GetProperty("start").GetInt64() == 1700000000000L, "READY preserves bounded custom emoji and complete activity metadata");
string report = JsonSerializer.Serialize(new { success = errors.Count == 0, errors, results, syntheticDataOnly = true, realNetworkUsed = false }, new JsonSerializerOptions { WriteIndented = true });
Console.WriteLine(report);
await File.WriteAllTextAsync(Path.Combine(data, "transport-tests.json"), report);
return errors.Count == 0 ? 0 : 1;

sealed class FakeHandler(Func<HttpRequestMessage, HttpResponseMessage> response) : HttpMessageHandler
{
    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) => Task.FromResult(response(request));
}

sealed class GeneratedContent(int length) : HttpContent
{
    public long Emitted;
    protected override bool TryComputeLength(out long size) { size = 0; return false; }
    protected override Task<Stream> CreateContentReadStreamAsync() => Task.FromResult<Stream>(new GeneratedStream(this, length));
    protected override async Task SerializeToStreamAsync(Stream stream, TransportContext? context)
    {
        byte[] buffer = new byte[32768]; Array.Fill(buffer, (byte)'x');
        for (int written = 0; written < length; written += buffer.Length)
        {
            int size = Math.Min(buffer.Length, length - written);
            await stream.WriteAsync(buffer.AsMemory(0, size)); Emitted += size;
        }
    }
    private sealed class GeneratedStream(GeneratedContent owner, int length) : Stream
    {
        private int position;
        public override bool CanRead => true;
        public override bool CanSeek => false;
        public override bool CanWrite => false;
        public override long Length => length;
        public override long Position { get => position; set => throw new NotSupportedException(); }
        public override int Read(byte[] buffer, int offset, int count)
        {
            int size = Math.Min(count, length - position);
            buffer.AsSpan(offset, size).Fill((byte)'x'); position += size; owner.Emitted += size; return size;
        }
        public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
        {
            cancellationToken.ThrowIfCancellationRequested();
            int size = Math.Min(buffer.Length, length - position);
            buffer.Span[..size].Fill((byte)'x'); position += size; owner.Emitted += size; return ValueTask.FromResult(size);
        }
        public override void Flush() { }
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    }
}
