using System.Reflection;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using NotDiscord.Terminal;

internal static class MediaDevicesUiChecks
{
    private static int checks;
    private static void Check(bool ok, string name) { if (!ok) throw new Exception(name); checks++; }
    private static T Field<T>(object value, string name) => (T)value.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(value)!;
    [STAThread]
    public static int Main()
    {
        string root = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../"));
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown }; int result = 1;
        app.Startup += async (_, _) =>
        {
            var owner = new TerminalWindow { Left = -10000, Top = -10000, WindowStartupLocation = WindowStartupLocation.Manual, ShowActivated = false };
            try
            {
                owner.Show();
                var devices = new MediaDeviceInventory([new("demo-input", "Micrófono de demostración")], [new("demo-output", "Auriculares de demostración")], [new("demo-camera", "Cámara de demostración")]);
                var settings = new MediaPreferences { InputId = "demo-input", OutputId = "demo-output", CameraId = "demo-camera" };
                var events = new List<bool>(); var saves = new List<bool>();
                var gate = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                bool wait = false;
                async Task Apply(MediaPreferences _, bool persist) { saves.Add(persist); if (wait) await gate.Task; }
                Task Mic(bool active) { events.Add(active); return Task.CompletedTask; }
                var dialog = new MediaSettingsDialog(owner, settings, devices, Apply, Mic, _ => throw new Exception("camera test unexpectedly invoked"), true, false)
                { Left = -10000, Top = -10000, WindowStartupLocation = WindowStartupLocation.Manual, ShowActivated = false };
                dialog.Show(); await Task.Delay(60); dialog.UpdateLayout();
                Check(!Field<Button>(dialog, "cameraTest").IsEnabled, "camera test disabled outside a call");
                Check(Field<ComboBox>(dialog, "input").Items.Count == 2, "Windows default plus enumerated input");
                Check((string)((ComboBoxItem)Field<ComboBox>(dialog, "input").SelectedItem).Tag == "demo-input", "persisted input selection");
                Capture(dialog, Path.Combine(root, "media-settings-dark.png"));
                typeof(TerminalWindow).Assembly.GetType("NotDiscord.Terminal.UiTheme")!.GetMethod("Apply")!.Invoke(null, ["#EEF1F6", "#B8C3D4", "#5865F2"]);
                await Task.Delay(30); Capture(dialog, Path.Combine(root, "media-settings-light.png"));
                Field<Button>(dialog, "apply").RaiseEvent(new RoutedEventArgs(Button.ClickEvent)); await Task.Delay(20);
                Check(saves.SequenceEqual([true]), "Apply explicitly saves settings");
                wait = true;
                Field<Button>(dialog, "micTest").RaiseEvent(new RoutedEventArgs(Button.ClickEvent)); await Task.Delay(20);
                dialog.Close(); await Task.Delay(20);
                Check(dialog.IsVisible, "Close waits for a pending test setup");
                gate.SetResult(); await Task.Delay(100);
                Check(!dialog.IsVisible, "Close completes after stopping test");
                Check(events.TakeLast(2).SequenceEqual([true, false]), "local test stopped even when close races setup");
                var cancelled = new MediaSettingsDialog(owner, settings, devices, Apply, Mic, _ => Task.CompletedTask, false, false)
                { Left = -10000, Top = -10000, WindowStartupLocation = WindowStartupLocation.Manual, ShowActivated = false };
                cancelled.Show(); await Task.Delay(20); int saveCount = saves.Count;
                Check(!Field<Button>(cancelled, "micTest").IsEnabled, "mic test disabled within a call");
                Field<Slider>(cancelled, "inputVolume").Value = 10; cancelled.Close(); await Task.Delay(50);
                Check(saves.Count == saveCount, "closing discards unapplied settings");
                File.WriteAllText(Path.Combine(root, "media-settings-ui-checks.json"), JsonSerializer.Serialize(new { success = true, checks, syntheticOnly = true, devicesOpened = false, networkUsed = false }));
                result = 0;
            }
            catch (Exception e) { Console.WriteLine(e.ToString()); }
            finally { owner.Close(); app.Shutdown(); }
        };
        app.Run(); return result;
    }
    private static void Capture(MediaSettingsDialog dialog, string path)
    {
        var content = (FrameworkElement)dialog.Content; content.UpdateLayout();
        int width = (int)Math.Ceiling(content.ActualWidth), height = (int)Math.Ceiling(content.ActualHeight);
        var canvas = new DrawingVisual(); using (var drawing = canvas.RenderOpen()) { drawing.DrawRectangle(dialog.Background, null, new Rect(0, 0, width, height)); drawing.DrawRectangle(new VisualBrush(content), null, new Rect(0, 0, width, height)); }
        var bitmap = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32); bitmap.Render(canvas);
        var png = new PngBitmapEncoder(); png.Frames.Add(BitmapFrame.Create(bitmap)); using var stream = File.Create(path); png.Save(stream);
    }
}
