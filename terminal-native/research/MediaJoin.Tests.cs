using System.Net;
using System.Net.Http;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Windows.Threading;
using NotDiscord.Terminal;

internal static class MediaJoinTests
{
    private static int checks;
    private static void Check(bool ok, string name) { if (!ok) throw new Exception(name); checks++; }
    private static void Set(Controller target, string field, object value) => typeof(Controller).GetField(field, BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
    private static object? Get(Controller target, string field) => typeof(Controller).GetField(field, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(target);
    [STAThread]
    public static int Main()
    {
        Run().GetAwaiter().GetResult();
        Console.WriteLine(JsonSerializer.Serialize(new { success = true, checks, realCalls = 0, mediaProcessesStarted = 0, realNetworkRequests = 0 })); return 0;
    }
    private static async Task Run()
    {
        var target = (Controller)RuntimeHelpers.GetUninitializedObject(typeof(Controller));
        var calls = new Dictionary<string, IncomingCall>(); var requests = new List<HttpMethod>(); bool deleteWhilePreflight = false;
        await using var session = new DiscordSession("SYNTHETIC-NOT-A-REAL-CREDENTIAL", new FakeHttp(request =>
        {
            requests.Add(request.Method);
            if (deleteWhilePreflight) calls.Clear();
            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(request.Method == HttpMethod.Get ? "{\"ringable\":true}" : "{}") };
        }));
        Set(target, "session", session); Set(target, "sessionWork", new CancellationTokenSource()); Set(target, "activeCalls", calls);
        Set(target, "state", new AppSnapshot { SignedIn = true, CurrentUser = new Person("100", "Synthetic self"), DirectMessages = [new("200", "Synthetic DM", "dm")] });
        Set(target, "voiceChannel", ""); Set(target, "voiceGuild", ""); Set(target, "pendingDmRingChannel", "");
        Set(target, "redraw", new DispatcherTimer());
        var start = typeof(Controller).GetMethod("StartDmVoiceAsync", BindingFlags.Instance | BindingFlags.NonPublic)!;
        await (Task)start.Invoke(target, ["200", false, false])!;
        Check(requests.Count == 0 && Get(target, "media") == null, "joining a terminated call does not perform even preflight or start media");
        Check(((AppSnapshot)Get(target, "state")!).Status.Contains("ya terminó"), "terminated join gives usable feedback");
        calls["200"] = new("200", "Synthetic DM", "dm", null, null, DateTimeOffset.UnixEpoch, [], true, true);
        deleteWhilePreflight = true;
        await (Task)start.Invoke(target, ["200", false, false])!;
        Check(requests.SequenceEqual([HttpMethod.Get]) && Get(target, "media") == null && (string)Get(target, "voiceChannel")! == "", "CALL_DELETE during preflight cannot create a replacement call");
        Check(Get(target, "pendingDmRingChannel") as string == "" && !(bool)Get(target, "voiceRequestPending")!, "cancelled join never arms ring and releases preparation gate");
        deleteWhilePreflight = false; requests.Clear(); Set(target, "voiceChannel", "200");
        var dispatch = typeof(Controller).GetMethod("HandleMediaDispatchAsync", BindingFlags.Instance | BindingFlags.NonPublic)!;
        var packet = JsonSerializer.SerializeToElement(new { channel_id = "200" });
        await (Task<bool>)dispatch.Invoke(target, ["CALL_CREATE", packet])!;
        Check(requests.Count == 0, "joining existing call does not ring on CALL_CREATE");
        Set(target, "pendingDmRingChannel", "200");
        await (Task<bool>)dispatch.Invoke(target, ["CALL_CREATE", packet])!;
        await (Task<bool>)dispatch.Invoke(target, ["CALL_CREATE", packet])!;
        Check(requests.SequenceEqual([HttpMethod.Post]) && Get(target, "pendingDmRingChannel") as string == "", "explicit normal call still rings once and repeated CALL_CREATE does not ring again");
    }
    private sealed class FakeHttp(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken token) => Task.FromResult(respond(request));
    }
}
