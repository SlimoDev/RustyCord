using System.Collections;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using NotDiscord.Terminal;

internal static class FollowUpUiChecks
{
    [STAThread]
    static int Main(string[] args)
    {
        var failures = new List<string>(); int checks = 0;
        void Check(bool value, string label) { checks++; if (!value) failures.Add(label); }
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
        var window = new TerminalWindow { Width = 1180, Height = 820 };
        T Field<T>(string name) => (T)typeof(TerminalWindow).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        void Layout() { var root = (FrameworkElement)window.Content; root.Measure(new Size(1180, 820)); root.Arrange(new Rect(0, 0, 1180, 820)); root.UpdateLayout(); }
        var me = new Person("101", "Cuenta de prueba", Username: "synthetic_user");
        var guild = new NavItem("500", "Servidor de prueba", "guild", IconUrl: "https://cdn.discordapp.com/icons/500/synthetic.png");
        var source = new[] { new NavItem("200", "Fijado de prueba", "dm", IsPinned: true), new NavItem("201", "Mensaje nuevo", "dm", Unread: true, MentionCount: 2), new NavItem("202", "Llamada de prueba", "group", HasActiveCall: true, IncomingCallRinging: true), new NavItem("203", "Silenciado", "dm", Unread: true, IsMuted: true) };
        var snapshot = new AppSnapshot { SignedIn = true, CurrentUser = me, View = "chat", SelectedChannelId = "200", SelectedChannelKind = "dm", ChannelName = "Fijado de prueba", DirectMessages = source, Guilds = new[] { guild }, SelfPresence = "online" };
        window.Apply(snapshot); Layout();
        var rail = Field<ListBox>("attentionRail").ItemsSource.Cast<UiNavModel>().Select(m => m.Item).ToArray();
        Check(rail.Select(n => n.Id).SequenceEqual(new[] { "202", "201" }), "Attention rail contains calls first and unread private chats, excluding muted and guilds");
        var ordered = Field<ListBox>("channels").ItemsSource.Cast<UiNavModel>().Select(m => m.Item).ToArray();
        Check(ordered[0].Name == "LLAMADAS" && ordered[1].Id == "202" && ordered.Count(n => n.Id == "202") == 1, "Calls are first in the main navigation, before pinned chats and without duplicates");
        var call = new UiAttentionCell { DataContext = new UiNavModel(source[2], _ => {}, true) };
        Check(Descendants(call).OfType<TextBlock>().Any(t => t.Text == "☎"), "Incoming call rail icon includes a visible call indicator");
        var unread = new UiAttentionCell { DataContext = new UiNavModel(source[1], _ => {}, true) };
        Check(Descendants(unread).OfType<TextBlock>().Any(t => t.Text == "2"), "Unread private rail icon includes the notification count");
        var menu = (ContextMenu)typeof(TerminalWindow).GetMethod("BuildPresenceMenu", BindingFlags.NonPublic | BindingFlags.Instance)!.Invoke(window, null)!;
        var actions = new List<UiAction>(); window.ActionRequested += actions.Add;
        var self = menu.Items.OfType<MenuItem>().FirstOrDefault(m => m.Header?.ToString() == "Ver mi perfil"); self?.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Check(actions.Any(a => a.Kind == "profile-self"), "Own avatar menu provides a working own-profile action");
        Check(menu.Items.OfType<MenuItem>().Count() == 5, "Own profile option retains all four status choices");
        window.Apply(snapshot with { View = "pins" });
        Check(Field<Button>("pinned").ToolTip?.ToString() == "Volver al chat", "Pinned button advertises returning to the same chat when active");
        var profile = new ProfileDetails(me, DateTimeOffset.Parse("2020-01-01T00:00:00Z"), null, "Perfil ficticio", "online", "", null, Array.Empty<PresenceActivity>(), Array.Empty<Person>(), new[] { guild });
        window.Apply(snapshot with { View = "profile", ProfileUser = me, ProfileDetails = profile }); Layout();
        Check(!Descendants(Field<Border>("profilePane")).OfType<Button>().Any(b => b.Content?.ToString() == "Enviar mensaje"), "Own profile does not offer an invalid message-to-self action");
        var tabs = Descendants(Field<Border>("profilePane")).OfType<TabControl>().First(); tabs.SelectedIndex = 2; Layout();
        var mutualModels = ((ListBox)((TabItem)tabs.Items[2]).Content).ItemsSource.Cast<UiNavModel>().ToArray();
        Check(mutualModels.Length == 1 && mutualModels[0].Item.IconUrl == guild.IconUrl, "Mutual guild list keeps the supplied server image instead of flattening to text");
        var mutual = new UiMutualGuildCell { DataContext = mutualModels[0] };
        Check(Descendants(mutual).OfType<UiMediaImage>().Any(), "Mutual guild cell renders the actual server image lazily");
        var chrome = (Grid)window.Content;
        Check(window.WindowStyle == WindowStyle.None && window.Icon != null && chrome.RowDefinitions[0].Height.Value == 32, "Main window has a Discord icon and a custom themed title bar");
        Check(ReferenceEquals(chrome.Background, ((DockPanel)chrome.Children[0]).Background) && ReferenceEquals(chrome.Background, window.Background), "Caption uses the same base brush as the main window");
        typeof(TerminalWindow).GetMethod("OnDesktopNotice", BindingFlags.NonPublic | BindingFlags.Instance)!.Invoke(window, new object[] { new DesktopNotice("201", "Ficticio", "No mostrar") });
        Check(Field<object?>("desktopNotifications") == null && !window.IsLoaded, "Headless fixture never creates Windows notifications or starts media downloads");
        var safeGuild = guild with { IconUrl = null };
        window.Apply(snapshot with { Demo = true, Connection = "DEMO SIN CONEXIÓN", Status = "Todos los datos de esta captura son ficticios", Guilds = new[] { safeGuild }, DirectMessages = source }); Layout();
        if (args.Length > 0) window.SavePreview(args[0]);
        window.Close(); app.Shutdown();
        Console.WriteLine(JsonSerializer.Serialize(new { success = failures.Count == 0, checks, failures, syntheticDataOnly = true, realNotificationsShown = false, realNetworkUsed = false })); return failures.Count == 0 ? 0 : 1;
    }
    static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    { for (int i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++) { var child = VisualTreeHelper.GetChild(root, i); yield return child; foreach (var nested in Descendants(child)) yield return nested; } }
}
