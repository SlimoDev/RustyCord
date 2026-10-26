using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using NotDiscord.Terminal;

internal static class Program
{
    [STAThread]
    private static int Main(string[] args)
    {
        if (args.Length != 1) throw new ArgumentException("Project root required");
        var application = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
        int result = 1;
        application.Startup += async (_, _) =>
        {
            var failures = new List<string>(); int assertions = 0, stages = 0;
            void Check(bool condition, string name) { assertions++; if (!condition) failures.Add(name); }
            string directory = Path.Combine(args[0], "tests-media-recovery", "synthetic-data", Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(directory);
            TerminalWindow? window = null; Controller? controller = null; MediaBridge? bridge = null;
            try
            {
                window = new TerminalWindow(); // Never shown; no application login/initialization.
                controller = new Controller(window, directory);
                bridge = new MediaBridge(args[0]);
                bridge.Event += (type, _) => { if (type == "mediaStage") Interlocked.Increment(ref stages); };
                int ownedPid = bridge.ProcessId;
                var capabilities = await bridge.CommandAsync("capabilities");
                Check(capabilities.GetProperty("backend").GetString() == "electron-main-napi", "Verified pipe connects to compatible media host");
                var stats = await bridge.CommandAsync("stats");
                Check(stats.GetProperty("voice").ValueKind == JsonValueKind.Null && stats.GetProperty("share").ValueKind == JsonValueKind.Null, "Helper has no voice or capture session");
                Set(controller, "media", bridge);
                Set(controller, "voiceConnected", true);
                Set(controller, "voiceEncryptionReady", true);
                Set(controller, "voiceChannel", "100000000000000002");
                Set(controller, "voiceGuild", "100000000000000003");
                Set(controller, "selfMute", true);
                var started = DateTimeOffset.UtcNow.AddMinutes(-2);
                Set(controller, "state", Get<AppSnapshot>(controller, "state") with { CurrentUser = new Person("100000000000000001", "Cuenta sintética") });
                Set(controller, "callStartedAt", started); Set(controller, "callChannelName", "Canal de prueba"); Set(controller, "callStatusLabel", "Conexión cifrada");
                Set(controller, "cameraAvailable", true); Set(controller, "cameraEnabled", true); Set(controller, "sharing", true);
                using (var participant = JsonDocument.Parse("{\"user_id\":\"100000000000000004\",\"channel_id\":\"100000000000000002\",\"self_mute\":true,\"self_deaf\":true,\"self_video\":true,\"self_stream\":true}"))
                    Invoke(controller, "UpdateCallParticipant", participant.RootElement);
                var call = Get<AppSnapshot>(controller, "state");
                Check(call.CallStartedAt == started && call.CallChannelName == "Canal de prueba", "Call metadata uses the backend start time and channel");
                Check(call.CallParticipants.Count == 2 && call.CallParticipants.Count(p => p.IsSelf) == 1, "Call panel contains self and a real event participant");
                var remote = call.CallParticipants.Single(p => !p.IsSelf);
                Check(remote.SelfMute && remote.SelfDeaf && remote.SelfVideo && remote.ScreenSharing, "Remote call flags reflect gateway event data");
                Check(call.CameraAvailable && call.CameraEnabled && call.ScreenSharing, "Camera and sharing state are distinct flags");
                using (var departed = JsonDocument.Parse("{\"user_id\":\"100000000000000004\",\"channel_id\":null}"))
                    Invoke(controller, "UpdateCallParticipant", departed.RootElement);
                Check(Get<AppSnapshot>(controller, "state").CallParticipants.Count == 1, "Leaving participant is removed from the call panel");
                bool rejected = false;
                try
                {
                    var operation = (Task)typeof(Controller).GetMethod("SetSelfAudioAsync", BindingFlags.NonPublic | BindingFlags.Instance)!.Invoke(controller, new object?[] { false, false })!;
                    await operation.WaitAsync(TimeSpan.FromSeconds(12));
                }
                catch (ServiceException) { rejected = true; }
                Check(rejected, "Unmute without a helper voice session is rejected");
                Check(Get<MediaBridge?>(controller, "media") == null, "Rejected audio change detaches the helper");
                Check(Get<bool>(controller, "selfMute"), "Rejected audio change leaves mute enabled");
                Check(!Get<bool>(controller, "voiceConnected") && !Get<bool>(controller, "voiceConnecting") && Get<string>(controller, "voiceChannel").Length == 0, "Rejected audio change clears connection state");
                var snapshot = Get<AppSnapshot>(controller, "state");
                Check(snapshot.SelfMute && !snapshot.VoiceConnected && !snapshot.VoiceConnecting, "UI reports muted and disconnected after failure");
                Check(snapshot.CallStartedAt == null && snapshot.CallParticipants.Count == 0 && snapshot.CallChannelName.Length == 0, "Recovery clears stale call metadata and participants");
                Check(!snapshot.VoiceEncryptionReady && !snapshot.CameraEnabled && !snapshot.CameraPreparing && !snapshot.ScreenSharing && !snapshot.ScreenPaused && !snapshot.ScreenPreparing, "Recovery clears encrypted readiness and all video and screen indicators");
                using var preferences = JsonDocument.Parse(File.ReadAllText(Path.Combine(directory, "audio-settings.json")));
                Check(preferences.RootElement.GetProperty("mute").GetBoolean(), "Next launch keeps the safe mute preference");
                bool processGone;
                try { using var process = Process.GetProcessById(ownedPid); processGone = process.HasExited; }
                catch (ArgumentException) { processGone = true; }
                Check(processGone, "Owned media process exited after failed audio change");
                Check(Volatile.Read(ref stages) == 0, "No native device initialization stages occurred");
                bridge = null; // Controller already disposed it during recovery.
            }
            catch (Exception error) { failures.Add("Unexpected test failure: " + error.GetType().Name); }
            finally
            {
                try { if (controller != null) await controller.DisposeAsync(); }
                catch { failures.Add("Controller cleanup failed"); }
                try { if (bridge != null) await bridge.DisposeAsync(); }
                catch { failures.Add("Bridge cleanup failed"); }
                window?.Close();
            }
            result = failures.Count == 0 ? 0 : 1;
            var report = new { success = result == 0, assertions, failures, realNetworkUsed = false, microphoneActivated = false, cameraActivated = false, screenCaptureStarted = false, accountLoaded = false, windowShown = false, nativeInitializationStages = stages };
            string json = JsonSerializer.Serialize(report, new JsonSerializerOptions { WriteIndented = true });
            File.WriteAllText(Path.Combine(args[0], "tests-media-recovery", "result.json"), json); Console.WriteLine(json);
            application.Shutdown(result);
        };
        application.Run();
        return result;
    }
    private static void Set(Controller controller, string name, object? value) => typeof(Controller).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(controller, value);
    private static T Get<T>(Controller controller, string name) => (T)typeof(Controller).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(controller)!;
    private static void Invoke(Controller controller, string name, params object?[] args) => typeof(Controller).GetMethod(name, BindingFlags.NonPublic | BindingFlags.Instance)!.Invoke(controller, args);
}
