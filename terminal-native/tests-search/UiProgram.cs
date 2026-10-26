using System.IO;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;
using NotDiscord.Terminal;

internal static class UiProgram
{
    [STAThread]
    private static int Main()
    {
        var failures = new List<string>(); int checks = 0;
        void Check(bool value, string name) { checks++; if (!value) failures.Add(name); }
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
        var window = new TerminalWindow { Width = 1100, Height = 760, Left = 80, Top = 80, ShowInTaskbar = false, Title = "NotDiscord · prueba sintética del buscador" };
        var alice = new Person("101", "Ana", Username: "ana_example");
        window.Apply(new AppSnapshot { SignedIn = true, View = "chat", SelectedChannelId = "200", ChannelName = "Datos ficticios · sin conexión", CurrentUser = new("100", "Demo"),
            Relationships = new[] { new Relationship(alice, 1) }, Channels = new[] { new NavItem("200", "general", "text") }, SelectedChannelKind = "text" });
        var search = (TextBox)typeof(TerminalWindow).GetField("search", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        var popup = (Popup)typeof(TerminalWindow).GetField("searchFiltersPopup", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        window.Show(); window.Activate(); search.Focus(); Pump();
        Check(search.IsKeyboardFocusWithin && popup.IsOpen, "Focus opens filters immediately without activation roundtrip");
        Check(ReferenceEquals(popup.PlacementTarget, search), "Popup always has a concrete placement target");
        Check(popup.Child is FrameworkElement { ActualWidth: > 0, ActualHeight: > 0 } && PresentationSource.FromVisual(popup.Child) != null, "First open contains measured child attached to a presentation source");
        var initial = Descendants(popup.Child).OfType<TextBlock>().Select(t => t.Text).ToArray();
        Check(new[] { "from:", "has:", "mentions:", "in:", "before:", "after:", "during:", "pinned:" }.All(key => initial.Any(t => t.Contains(key))), "First panel exposes all eight Discord filter families");
        Button? Pick(string title) => Descendants(popup.Child).OfType<Button>().FirstOrDefault(b => Descendants(b).OfType<TextBlock>().Any(t => t.Text == title));
        Pick("Contiene un tipo de contenido")?.RaiseEvent(new RoutedEventArgs(Button.ClickEvent)); Pump();
        Check(search.Text == "has:" && popup.IsOpen, "Filter click inserts token and shows its value suggestions");
        Pick("Imagen")?.RaiseEvent(new RoutedEventArgs(Button.ClickEvent)); Pump();
        Check(search.Text == "has:image " && popup.IsOpen, "Content suggestion fills active token and keeps combinations available");
        Pick("De una persona")?.RaiseEvent(new RoutedEventArgs(Button.ClickEvent)); Pump();
        Pick("Ana")?.RaiseEvent(new RoutedEventArgs(Button.ClickEvent)); Pump();
        Check(search.Text == "has:image from:101 ", "Person suggestion combines with an existing has filter");
        search.RaiseEvent(new KeyEventArgs(Keyboard.PrimaryDevice, PresentationSource.FromVisual(search), Environment.TickCount, Key.Escape) { RoutedEvent = Keyboard.PreviewKeyDownEvent }); Pump();
        Check(!popup.IsOpen, "Escape closes suggestions");
        search.RaiseEvent(new MouseButtonEventArgs(Mouse.PrimaryDevice, Environment.TickCount, MouseButton.Left) { RoutedEvent = UIElement.PreviewMouseLeftButtonDownEvent }); Pump();
        Check(popup.IsOpen && ((FrameworkElement)popup.Child).ActualHeight > 0, "Click reopens an already-focused field without alt-tab");
        var other = new Window { Width = 120, Height = 80, Left = 30, Top = 30, ShowInTaskbar = false, Content = "Fixture de foco" };
        other.Show(); other.Activate(); Pump();
        Check(!popup.IsOpen, "Owner deactivation closes popup");
        other.Close(); window.Activate(); search.Focus(); search.RaiseEvent(new MouseButtonEventArgs(Mouse.PrimaryDevice, Environment.TickCount, MouseButton.Left) { RoutedEvent = UIElement.PreviewMouseLeftButtonDownEvent }); Pump();
        Check(popup.IsOpen, "After activation one click still opens the populated popup");
        popup.IsOpen = false; window.Close(); Pump(); app.Shutdown();
        string evidence = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../evidence")); Directory.CreateDirectory(evidence);
        string report = JsonSerializer.Serialize(new { success = failures.Count == 0, checks, failures, syntheticDataOnly = true, accountAccessed = false, networkUsed = false, realSearchSubmitted = false }, new JsonSerializerOptions { WriteIndented = true });
        File.WriteAllText(Path.Combine(evidence, "search-ui-checks.json"), report); Console.WriteLine(report);
        return failures.Count == 0 ? 0 : 1;
    }
    private static void Pump()
    {
        var frame = new DispatcherFrame(); Dispatcher.CurrentDispatcher.BeginInvoke(DispatcherPriority.ContextIdle, new Action(() => frame.Continue = false)); Dispatcher.PushFrame(frame);
    }
    private static IEnumerable<DependencyObject> Descendants(DependencyObject? root)
    {
        if (root == null) yield break;
        yield return root;
        for (int i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++) foreach (var child in Descendants(VisualTreeHelper.GetChild(root, i))) yield return child;
    }
}
