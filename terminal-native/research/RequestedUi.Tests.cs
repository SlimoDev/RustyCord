using System.Collections;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using NotDiscord.Terminal;

internal static class RequestedUiTests
{
    [STAThread] static int Main(string[] args)
    {
        var failures = new List<string>(); int checks = 0;
        void Check(bool value, string name) { checks++; if (!value) failures.Add(name); }
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
        var window = new TerminalWindow { Width = 1450, Height = 900 };
        T Field<T>(string name) => (T)typeof(TerminalWindow).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        object? Method(string name, params object[] parameters) => typeof(TerminalWindow).GetMethod(name, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, parameters);
        void Layout() { var root = (FrameworkElement)window.Content; root.Measure(new Size(1450, 900)); root.Arrange(new Rect(0, 0, 1450, 900)); root.UpdateLayout(); }
        var me = new Person("100", "Cuenta ficticia"); var alice = new Person("101", "Alice"); var outsider = new Person("999", "Fuera del canal");
        var role = new GuildRole("510", "Moderadores", 10, 0xDF63E9, true);
        var guild = new NavItem("500", "Servidor ficticio", "guild");
        var allowed = ChannelAccess.Private;
        var readOnly = allowed with { CanSend = false, CanAttach = false, CanReact = false, Reason = "Sólo lectura" };
        var member = new ChannelMember(alice, "online", "Estado ficticio", new StatusEmoji("😀"), new[] { new PresenceActivity(0, "Juego de prueba") }, Roles: new[] { role }, Nickname: "Alice del servidor", RoleColor: role.Color, HoistRoleId: role.Id);
        var message = new ChatMessage("2001", "200", alice, "Mensaje ficticio con reacciones", DateTimeOffset.Parse("2026-10-20T12:00:00Z"), Array.Empty<ChatAttachment>(), Reactions: new[] { new MessageReaction(new StatusEmoji("😀"), 3, true), new MessageReaction(new StatusEmoji("custom", "123"), 2, false) });
        var snapshot = new AppSnapshot { SignedIn = true, CurrentUser = me, View = "chat", SelectedGuildId = "500", SelectedChannelId = "200", SelectedChannelKind = "text", ChannelName = "general", ChannelAccess = readOnly,
            Guilds = new[] { guild }, Channels = new[] { new NavItem("200", "general", "text", Access: readOnly) }, Messages = new[] { message }, GuildMembers = new[] { member }, GuildRoles = new[] { role }, GuildMemberGroups = new[] { new GuildMemberGroup(role.Id, role.Name, 1, role.Color) }, SearchPeople = new[] { me, alice }, Relationships = new[] { new Relationship(outsider, 1) } };
        window.Apply(snapshot); Layout();
        Check(!Field<TextBox>("composer").IsEnabled && !Field<Button>("send").IsEnabled && !Field<Button>("attachmentButton").IsEnabled, "Read-only channel disables writing, sending and attaching in the actual compositor");
        Check(Field<TextBlock>("composerPermissionHint").Text.Contains("permiso"), "Disabled compositor explains its effective permissions");
        Check(Field<Border>("groupPane").Visibility == Visibility.Visible && Field<ListBox>("groupList").Items.Count == 2, "Guild sidebar renders a role heading and visible channel member");
        var memberCell = new UiMemberCell { DataContext = new UiMemberModel(member, _ => {}) };
        Check(Descendants(memberCell).OfType<TextBlock>().Any(t => t.Text.Contains("Alice del servidor")), "Server member uses the contextual nickname");
        Check(Descendants(memberCell).OfType<TextBlock>().Any(t => t.Foreground is SolidColorBrush brush && brush.Color.R == 0xDF && brush.Color.B == 0xE9), "Visible role color reaches the server member name");
        var row = new UiMessageCell { DataContext = new UiMessageModel(message, _ => {}, false, false, true, Access: readOnly) };
        var buttons = Descendants(row).OfType<Button>().ToArray();
        Check(buttons.Any(b => b.ToolTip?.ToString()?.StartsWith("Quitar mi reacción") == true && b.IsEnabled), "Existing own reaction remains removable without ADD_REACTIONS");
        Check(buttons.Any(b => b.ToolTip?.ToString()?.StartsWith("Añadir mi reacción") == true && b.IsEnabled), "Existing emoji reaction remains joinable without ADD_REACTIONS");
        Check(Descendants(row).OfType<UiMediaImage>().Any() && Descendants(row).OfType<TextBlock>().Any(t => t.Text == "3"), "Reactions show custom emoji image controls and real counters");
        Check(row.ContextMenu.Items.OfType<MenuItem>().Any(i => i.Header?.ToString()?.Contains("Añadir reacción") == true && !i.IsEnabled), "New-emoji reaction action is disabled in a read-only reaction channel");
        var context = (SearchContext)Method("CurrentSearchContext")!;
        Check(context.People.All(p => p.Id != outsider.Id) && context.People.Any(p => p.Id == alice.Id), "Search suggestions never mix the global friends list into the active channel");
        window.Apply(snapshot with { ChannelAccess = allowed, CanPinMessages = true });
        var updatedRows = Field<System.Collections.ObjectModel.ObservableCollection<UiMessageModel>>("messageModels");
        Check(updatedRows.Count == 1 && updatedRows[0].Access == allowed && updatedRows[0].CanPin, "Permission-only updates refresh message actions even when the message collection is unchanged");
        window.Apply(snapshot with { SelectedGuildId = "", SelectedChannelKind = "dm", ChannelAccess = allowed });
        Check(((SearchContext)Method("CurrentSearchContext")!).Channels.Count == 0, "Private chat search does not inherit channels from a previous server");
        var iconTarget = new NavItem("301", "Destino con avatar", "dm", IconUrl: "https://cdn.discordapp.com/avatars/301/synthetic.png", Access: allowed);
        var targetCell = new UiForwardTargetCell { DataContext = new UiForwardTargetModel(iconTarget, false, "", (_, _) => true) };
        Check(Descendants(targetCell).OfType<UiMediaImage>().Any(), "Forward targets retain their avatar image controls");
        var selected = new List<string>();
        bool Toggle(NavItem item, bool next) { if (next) selected.Add(item.Id); else selected.Remove(item.Id); return true; }
        var first = new UiForwardTargetCell { DataContext = new UiForwardTargetModel(iconTarget, false, "", Toggle) }; var second = new UiForwardTargetCell { DataContext = new UiForwardTargetModel(iconTarget with { Id = "302" }, false, "", Toggle) };
        Descendants(first).OfType<CheckBox>().First().IsChecked = true; Descendants(second).OfType<CheckBox>().First().IsChecked = true;
        Check(selected.SequenceEqual(new[] { "301", "302" }), "Forward selector allows multiple destinations without replacing the first choice");
        var disabled = new UiForwardTargetCell { DataContext = new UiForwardTargetModel(iconTarget, false, "Sin permiso de enviar", Toggle) };
        Check(!Descendants(disabled).OfType<CheckBox>().First().IsEnabled && Descendants(disabled).OfType<TextBlock>().Any(t => t.Text.Contains("Sin permiso")), "Visible but non-sendable forwarding destination is disabled with a reason");
        var scope = new UiForwardScopeCell { DataContext = new NavItem("", "Conversaciones", "section") };
        Check(Descendants(scope).OfType<TextBlock>().Any(t => t.Text == "Conversaciones") && !Descendants(scope).OfType<TextBlock>().Any(t => t.Text.StartsWith("NavItem")), "Forward scope has a real label rather than the record debug representation");
        var nav = Field<ListBox>("channels"); var previous = nav.SelectedItem;
        var right = new MouseButtonEventArgs(Mouse.PrimaryDevice, 0, MouseButton.Right) { RoutedEvent = UIElement.PreviewMouseRightButtonDownEvent }; nav.RaiseEvent(right);
        Check(right.Handled && ReferenceEquals(previous, nav.SelectedItem), "Right-button preview prevents ListBox selection and navigation");
        Check(Descendants((FrameworkElement)window.Content).OfType<Button>().Any(b => b.ToolTip?.ToString() == "Inicio · mensajes y amigos" && b.Padding == new Thickness(0) && b.Content is Image logo && logo.Width <= b.Width - 2), "Discord home icon fits inside the sidebar button without cropping");
        Check(!window.IsLoaded, "No account, network images or Windows notification is activated by the fixture");
        if (args.Length > 0) { window.Apply(snapshot with { Demo = true, ChannelAccess = allowed, Status = "Datos ficticios · revisión de permisos y roles" }); Layout(); window.SavePreview(args[0]); }
        window.Close(); app.Shutdown();
        Console.WriteLine(JsonSerializer.Serialize(new { success = failures.Count == 0, checks, failures, syntheticDataOnly = true, realNetworkUsed = false, realNotificationsShown = false })); return failures.Count == 0 ? 0 : 1;
    }
    static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        var seen = new HashSet<DependencyObject>(); var queue = new Queue<DependencyObject>(); queue.Enqueue(root); seen.Add(root);
        while (queue.Count > 0)
        {
            var current = queue.Dequeue();
            var children = LogicalTreeHelper.GetChildren(current).OfType<DependencyObject>().ToList();
            if (current is Visual || current is System.Windows.Media.Media3D.Visual3D)
                for (int i = 0; i < VisualTreeHelper.GetChildrenCount(current); i++) children.Add(VisualTreeHelper.GetChild(current, i));
            foreach (var child in children) if (seen.Add(child)) { yield return child; queue.Enqueue(child); }
        }
    }
}
