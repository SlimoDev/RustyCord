using System.IO;
using System.Net;
using System.Net.Http;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Threading;
using NotDiscord.Terminal;

internal static class CallEditTests
{
    static readonly List<string> failures = new(); static int checks;
    static void Check(bool value, string label) { checks++; if (!value) failures.Add(label); }
    static object? Field(object target, string name) => target.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(target);
    static void Set(object target, string name, object? value) => target.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
    static object? Call(object target, string name, params object[] arguments) => target.GetType().GetMethod(name, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(target, arguments);
    [STAThread] static int Main()
    {
        var dispatcher = Dispatcher.CurrentDispatcher; SynchronizationContext.SetSynchronizationContext(new DispatcherSynchronizationContext(dispatcher));
        try
        {
            var task = Run(); if (!task.IsCompleted) { var frame = new DispatcherFrame(); _ = task.ContinueWith(_ => dispatcher.BeginInvoke(() => frame.Continue = false)); Dispatcher.PushFrame(frame); } task.GetAwaiter().GetResult();
        }
        catch (Exception error) { failures.Add(error.ToString()); }
        Application.Current?.Shutdown(); Console.WriteLine(JsonSerializer.Serialize(new { success = failures.Count == 0, checks, failures, syntheticDataOnly = true, realNetworkUsed = false, realCalls = 0, realEdits = 0 })); return failures.Count == 0 ? 0 : 1;
    }
    static async Task Run()
    {
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown }; var window = new TerminalWindow { Width = 1360, Height = 860 };
        var me = new Person("100", "Cuenta ficticia");
        var original = new ChatMessage("800", "700", me, "Texto original", DateTimeOffset.UnixEpoch, Array.Empty<ChatAttachment>());
        var forwarded = original with { Forwarded = true };
        var actions = new List<UiAction>();
        UiMessageCell Cell(ChatMessage message) => new() { DataContext = new UiMessageModel(message, actions.Add, true, false, true, Access: ChannelAccess.Private) };
        var forwardCell = Cell(forwarded);
        Check(!Descendants(forwardCell).OfType<Button>().Any(b => b.ToolTip?.ToString() == "Editar") && !forwardCell.ContextMenu.Items.OfType<MenuItem>().Any(m => m.Header?.ToString() == "Editar mensaje"), "Own forwarded messages have no edit button or context-menu edit action");
        Check(Descendants(forwardCell).OfType<Button>().Any(b => b.ToolTip?.ToString() == "Eliminar") && forwardCell.ContextMenu.Items.OfType<MenuItem>().Any(m => m.Header?.ToString() == "Eliminar mensaje"), "Removing Edit preserves Delete for one's forwarded message");
        var originalCell = Cell(original);
        Check(Descendants(originalCell).OfType<Button>().Any(b => b.ToolTip?.ToString() == "Editar") && originalCell.ContextMenu.Items.OfType<MenuItem>().Any(m => m.Header?.ToString() == "Editar mensaje"), "Original own messages retain both edit entry points");
        var snapshot = new AppSnapshot { SignedIn = true, CurrentUser = me, View = "chat", SelectedChannelId = "700", SelectedChannelKind = "dm", ChannelName = "Chat ficticio", ChannelAccess = ChannelAccess.Private, DirectMessages = new[] { new NavItem("700", "Chat ficticio", "dm") }, Messages = new[] { forwarded }, VoiceAvailable = true };
        window.Apply(snapshot); window.ActionRequested += actions.Add;
        Call(window, "Handle", new UiAction("edit-local", "800", "Texto reenviado", ChannelId: "700"));
        Check(actions.Count == 0 && ((TextBlock)Field(window, "status")!).Text.Contains("reenviados"), "A stale forwarded Edit action is rejected before opening an editor");
        var phone = (Button)Field(window, "call")!; var camera = (Button)Field(window, "videoCall")!;
        Check(phone.Content is Image { Source: DrawingImage } phoneImage && camera.Content is Image { Source: DrawingImage } cameraImage && phoneImage.Width == cameraImage.Width && phoneImage.Height == cameraImage.Height, "Voice and camera use matching vector-image controls rather than a font glyph");
        Check(phone.Width == phone.Height && phone.Width == camera.Width && phone.Height == camera.Height && phone.Padding == camera.Padding, "Voice and camera have equal square frames and padding at normal width");
        window.Width = 1000; Call(window, "ApplyHeaderLayout");
        Check(phone.Width == phone.Height && camera.Width == camera.Height && phone.Width == camera.Width, "Header controls remain square at the compact width");
        Check(phone.Content is Image compactIcon && compactIcon.Width + phone.Padding.Left + phone.Padding.Right + 2 <= phone.Width, "The compact phone icon fits its frame without being clipped");
        window.Apply(snapshot with { VoiceConnected = true, VoiceEncryptionReady = false, CallStatusLabel = "Conectado · esperando participantes" });
        Check(((Button)Field(window, "callMute")!).IsEnabled && ((Button)Field(window, "callDeafen")!).IsEnabled && ((Button)Field(window, "callDisconnect")!).IsEnabled && !((Button)Field(window, "callScreen")!).IsEnabled && !((Button)Field(window, "camera")!).IsEnabled, "Solo call keeps audio and leave controls usable while new capture still waits for encryption");
        window.Apply(snapshot with { VoiceConnected = true, VoiceEncryptionReady = false, ScreenSharing = true, CameraEnabled = true });
        Check(((Button)Field(window, "callScreen")!).IsEnabled && ((Button)Field(window, "camera")!).IsEnabled && ((Button)Field(window, "share")!).IsEnabled, "Already active screen or camera capture retains its stop controls if encryption readiness changes");
        window.Apply(snapshot with { VoiceConnected = true, VoiceEncryptionReady = false, ScreenSharing = true, ScreenPaused = true, CallStatusLabel = "Conectado · preparando conexión cifrada" });
        Check(((Button)Field(window, "callScreen")!).Content?.ToString() == "Cancelar pantalla en pausa" && ((TextBlock)Field(window, "callStatus")!).Text.Contains("Pantalla en pausa") && ((Button)Field(window, "callScreen")!).IsEnabled, "An encryption pause is shown truthfully and can be cancelled from its normal screen control");
        window.Apply(snapshot);
        int requests = 0; HttpMethod? lastMethod = null;
        var session = new DiscordSession("SYNTHETIC-NOT-A-CREDENTIAL", new FakeHttp(request => { requests++; lastMethod = request.Method; return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{}") }; }));
        string folder = Path.Combine(AppContext.BaseDirectory, "synthetic-call-edit"); Directory.CreateDirectory(folder);
        await using (var controller = new Controller(window, folder))
        {
            Set(controller, "session", session); Set(controller, "currentChannel", "700"); Set(controller, "state", snapshot);
            var cache = (MessageWindowCache)Field(controller, "cache")!; cache.Upsert(forwarded);
            bool rejected = false;
            try { await (Task)Call(controller, "ActAsync", new UiAction("edit", "800", "Cambio", ChannelId: "700"))!; } catch (ServiceException) { rejected = true; }
            Check(rejected && requests == 0, "Backend refuses editing forwarded snapshots before issuing an HTTP mutation");
            cache.Upsert(original);
            await (Task)Call(controller, "ActAsync", new UiAction("edit", "800", "Cambio permitido", ChannelId: "700"))!;
            Check(requests == 1 && lastMethod == HttpMethod.Patch, "The original-message edit path still reaches the mocked PATCH endpoint");
        }
        Check(!window.IsLoaded, "No live UI, account, call or real edit was activated by the fixture"); window.Close();
    }
    static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        var seen = new HashSet<DependencyObject> { root }; var queue = new Queue<DependencyObject>(); queue.Enqueue(root);
        while (queue.Count > 0)
        {
            var current = queue.Dequeue(); var children = LogicalTreeHelper.GetChildren(current).OfType<DependencyObject>().ToList();
            if (current is Visual || current is System.Windows.Media.Media3D.Visual3D) for (int i = 0; i < VisualTreeHelper.GetChildrenCount(current); i++) children.Add(VisualTreeHelper.GetChild(current, i));
            foreach (var child in children) if (seen.Add(child)) { yield return child; queue.Enqueue(child); }
        }
    }
    sealed class FakeHttp(Func<HttpRequestMessage, HttpResponseMessage> response) : HttpMessageHandler
    { protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) => Task.FromResult(response(request)); }
}
