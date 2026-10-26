using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using System.Windows.Threading;
using NotDiscord.Terminal;

internal static class SoloVoiceReadinessTests
{
    private const BindingFlags Private = BindingFlags.Instance | BindingFlags.NonPublic;
    private static int checks;
    private static void Check(bool ok, string name) { if (!ok) throw new Exception(name); checks++; }
    private static void Set(object target, string field, object? value) => target.GetType().GetField(field, Private)!.SetValue(target, value);
    private static object? Get(object target, string field) => target.GetType().GetField(field, Private)!.GetValue(target);
    private static object? Invoke(object target, string method, params object?[] args) => target.GetType().GetMethod(method, Private, args.Select(a => a!.GetType()).ToArray())!.Invoke(target, args);
    private static Controller Target()
    {
        var target = (Controller)RuntimeHelpers.GetUninitializedObject(typeof(Controller));
        Set(target, "state", new AppSnapshot { CurrentUser = new Person("100", "Synthetic self") }); Set(target, "redraw", new DispatcherTimer());
        Set(target, "voiceChannel", "200"); Set(target, "voiceGuild", ""); Set(target, "voiceSession", "synthetic-session");
        Set(target, "callChannelName", "Synthetic call"); Set(target, "callStatusLabel", "");
        Set(target, "callParticipants", new Dictionary<string, CallParticipant>()); Set(target, "knownPeople", new Dictionary<string, Person>());
        foreach (string field in new[] { "streamKey", "rtcServerId", "streamChannel", "streamEndpoint", "streamCredential", "sourceId" }) Set(target, field, "");
        return target;
    }
    private static AppSnapshot Readiness(Controller target, object payload)
    {
        Invoke(target, "ApplyVoiceReadiness", JsonSerializer.SerializeToElement(payload)); Invoke(target, "RefreshMediaControls");
        ((DispatcherTimer)Get(target, "redraw")!).Stop();
        return (AppSnapshot)Get(target, "state")!;
    }
    [STAThread]
    public static int Main()
    {
        Run().GetAwaiter().GetResult();
        Console.WriteLine(JsonSerializer.Serialize(new { success = true, checks, realCalls = 0, nativeLoads = 0, mediaProcessesStarted = 0, realNetworkRequests = 0, microphoneOpened = false, cameraOpened = false, captureStarted = false })); return 0;
    }
    private static async Task Run()
    {
        var target = Target(); Set(target, "voiceConnecting", true); Set(target, "pendingVideoGeneration", 7);
        var state = Readiness(target, new { state = "waiting-peer", transportReady = true, encryptionReady = false, cameraAvailable = true });
        Check(state.VoiceConnected && !state.VoiceConnecting && !state.VoiceEncryptionReady, "sole call controls become usable without claiming E2EE");
        Check(!state.ScreenAvailable && !state.CameraAvailable && !state.CameraEnabled, "waiting peer cannot enable screen or camera send");
        Check(state.CallStatusLabel.Contains("esperando participantes") && state.CallStartedAt != null, "waiting call has honest status and one call clock");
        Check((int)Get(target, "pendingVideoGeneration")! == 7, "local readiness does not consume pending video activation");
        var started = state.CallStartedAt;
        state = Readiness(target, new { state = "negotiating", transportReady = true, encryptionReady = false, cameraAvailable = true });
        Check(state.VoiceConnected && !state.VoiceEncryptionReady && !state.ScreenAvailable && !state.CameraAvailable, "peer joining keeps local controls while send remains gated");
        Check(state.CallStartedAt == started, "peer joining preserves the call clock");
        state = Readiness(target, new { state = "connected", transportReady = true, encryptionReady = true, cameraAvailable = true });
        Check(state.VoiceConnected && state.VoiceEncryptionReady && state.ScreenAvailable && state.CameraAvailable, "real E2EE readiness enables remote media controls");
        Set(target, "selfMute", true); Set(target, "selfDeaf", true); Set(target, "sharing", true); Set(target, "sharePaused", true);
        state = Readiness(target, new { state = "waiting-peer", transportReady = true, encryptionReady = false, cameraAvailable = false });
        Check(state.SelfMute && state.SelfDeaf && state.ScreenSharing && state.ScreenPaused, "reset preserves explicit audio choices and paused screen Stop");
        Check(state.VoiceConnected && !state.ScreenAvailable && !state.CameraAvailable, "sole-member reset withdraws send availability");
        state = Readiness(target, new { state = "connected", transportReady = true, cameraAvailable = true });
        Check(!state.VoiceConnected && !state.VoiceEncryptionReady && !state.CameraAvailable && !state.ScreenAvailable, "a connected literal without encryption proof never authorizes media");
        state = Readiness(target, new { state = "negotiating", transportReady = false, encryptionReady = false, cameraAvailable = true });
        Check(!state.VoiceConnected && state.VoiceConnecting && !state.CameraAvailable, "pre-transport negotiation remains connecting");
        state = Readiness(target, new { state = "disconnected", transportReady = false, encryptionReady = false });
        Check(!state.VoiceConnected && !state.VoiceEncryptionReady && !state.ScreenAvailable, "disconnect cannot retain readiness");

        await ShareInterrupted(false); await ShareInterrupted(true);
        await ShareReplaced();
    }
    private static async Task ShareInterrupted(bool beforeStart)
    {
        var target = Target(); var bridge = new FakeBridge(command => { if (command == "startShare") Set(target, "voiceEncryptionReady", false); });
        PrepareShare(target, bridge.Value); Set(target, "voiceEncryptionReady", !beforeStart);
        await (Task)Invoke(target, "TryStartShareAsync")!;
        Check(bridge.Commands.SequenceEqual(beforeStart ? new[] { "stopShare" } : new[] { "startShare", "stopShare" }), beforeStart ? "loss before stream dispatch never starts native media" : "loss while native start is pending stops the same attempt");
        Check(!(bool)Get(target, "shareConnecting")! && !(bool)Get(target, "shareTransportStarting")! && !(bool)Get(target, "sharing")! && (string)Get(target, "sourceId")! == "", "interrupted share releases preparation and chosen source");
        ((DispatcherTimer)Get(target, "redraw")!).Stop(); bridge.Dispose();
    }
    private static async Task ShareReplaced()
    {
        var target = Target(); var replacement = new FakeBridge(_ => { });
        var bridge = new FakeBridge(command => { if (command == "startShare") { Set(target, "media", replacement.Value); Set(target, "shareGeneration", 2); } });
        PrepareShare(target, bridge.Value);
        await (Task)Invoke(target, "TryStartShareAsync")!;
        Check(bridge.Commands.SequenceEqual(new[] { "startShare" }) && replacement.Commands.Count == 0, "late completion never stops a replacement media attempt");
        Check((int)Get(target, "shareGeneration")! == 2, "late completion does not overwrite replacement generation");
        bridge.Dispose(); replacement.Dispose();
    }
    private static void PrepareShare(Controller target, MediaBridge bridge)
    {
        Set(target, "media", bridge); Set(target, "voiceEncryptionReady", true); Set(target, "voiceConnected", true);
        Set(target, "shareGeneration", 1); Set(target, "shareConnecting", true); Set(target, "rtcServerId", "300");
        Set(target, "streamEndpoint", "synthetic.invalid"); Set(target, "streamCredential", "SYNTHETIC-NOT-A-REAL-CREDENTIAL"); Set(target, "sourceId", "synthetic-source");
    }
    private sealed class FakeBridge : IDisposable
    {
        public MediaBridge Value { get; } = (MediaBridge)RuntimeHelpers.GetUninitializedObject(typeof(MediaBridge));
        public List<string> Commands { get; } = new();
        private readonly Process currentProcess = Process.GetCurrentProcess();
        private readonly CancellationTokenSource lifetime = new();
        private readonly StreamWriter output;
        private readonly SemaphoreSlim writer = new(1, 1);
        public FakeBridge(Action<string> duringCommand)
        {
            output = new StreamWriter(new SyntheticPipe(line =>
            {
                using var frame = JsonDocument.Parse(line); string command = frame.RootElement.GetProperty("command").GetString()!;
                Commands.Add(command); duringCommand(command);
                Invoke(Value, "ProcessFrame", JsonSerializer.Serialize(new { id = frame.RootElement.GetProperty("id").GetString(), ok = true, result = new { } }));
            }), new UTF8Encoding(false));
            Set(Value, "process", currentProcess); Set(Value, "connected", Task.CompletedTask); Set(Value, "waiting", new ConcurrentDictionary<string, TaskCompletionSource<JsonElement>>());
            Set(Value, "writer", writer); Set(Value, "lifetime", lifetime); Set(Value, "pipeOutput", output);
        }
        public void Dispose() { output.Dispose(); lifetime.Dispose(); writer.Dispose(); currentProcess.Dispose(); }
    }
    private sealed class SyntheticPipe(Action<string> receive) : MemoryStream
    {
        private readonly StringBuilder pending = new();
        private void Consume(ReadOnlySpan<byte> bytes)
        {
            pending.Append(Encoding.UTF8.GetString(bytes));
            while (pending.ToString().IndexOf('\n') is int end && end >= 0) { string line = pending.ToString(0, end); pending.Remove(0, end + 1); receive(line); }
        }
        public override void Write(byte[] buffer, int offset, int count) => Consume(buffer.AsSpan(offset, count));
        public override void Write(ReadOnlySpan<byte> buffer) => Consume(buffer);
        public override ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken cancellationToken = default) { Consume(buffer.Span); return ValueTask.CompletedTask; }
        public override Task WriteAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken) { Consume(buffer.AsSpan(offset, count)); return Task.CompletedTask; }
    }
}
