using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private Controller? attachedController;
    private string attachedDataDirectory = "";
    private ProfileDetails? renderedProfileDetails;
    private IReadOnlyList<Relationship>? renderedProfileRelationships, renderedMemberRelationships;
    private bool renderedProfileBusy, renderedMemberCanAct;
    private string renderedMemberSelfId = "";
    private readonly ColumnDefinition groupColumn = new() { Width = new GridLength(0) };
    private readonly Border groupPane = new();
    private readonly ListBox groupList = new();
    private readonly TextBlock groupTitle = new();
    private IReadOnlyList<ChannelMember>? renderedMembers;
    private ListBox? groupDialogList;
    private bool groupMembersOpen = true;
    private readonly Button moreMembers = new();

    public void AttachController(Controller controller, string dataDirectory)
    { attachedController = controller; attachedDataDirectory = dataDirectory; WireDesktopNotifications(controller); }

    private void ApplyHeaderLayout()
    {
        double available = Math.Max(220, (ActualWidth > 0 ? ActualWidth : Width) / preferences.Zoom - 72 - 292 - 34);
        bool narrow = available < 480;
        var buttons = new[] { call, videoCall, pinned, addGroup, membersToggle };
        foreach (var button in buttons) { button.Width = button.Height = narrow ? 26 : 36; button.Padding = new Thickness(narrow ? 2 : 5); if (button.Content is Image image) image.Width = image.Height = narrow ? 18 : 22; }
        int visible = buttons.Count(b => b.Visibility == Visibility.Visible);
        search.Width = Math.Clamp(available - visible * (narrow ? 29 : 39) - 80, 100, 220);
    }

    private void BuildGroupSidebar(Grid center)
    {
        var dock = new DockPanel();
        groupTitle.Margin = new Thickness(12, 16, 10, 12); groupTitle.Foreground = Muted; groupTitle.FontSize = 13;
        DockPanel.SetDock(groupTitle, Dock.Top); dock.Children.Add(groupTitle);
        Virtual(groupList); groupList.ItemTemplate = CellTemplate<UiMemberCell>(); dock.Children.Add(groupList);
        moreMembers.Content = "Cargar más miembros"; moreMembers.Margin = new Thickness(10); moreMembers.Click += (_, _) => Emit("members-more"); DockPanel.SetDock(moreMembers, Dock.Bottom); dock.Children.Insert(1, moreMembers);
        groupPane.Child = dock; groupPane.Background = UiTheme.Panel; groupPane.BorderBrush = line; groupPane.BorderThickness = new Thickness(1, 0, 0, 0);
        groupPane.Visibility = Visibility.Collapsed; Grid.SetColumn(groupPane, 1); center.Children.Add(groupPane);
        SizeChanged += (_, _) => { ApplyHeaderLayout(); ApplyGroupWidth(); };
    }

    private double ChatWidth => (ActualWidth > 0 ? ActualWidth : Width) / preferences.Zoom - 364;
    private void ApplyGroupWidth()
    {
        bool show = groupMembersOpen && (state.SelectedChannelKind == "group" || state.SelectedGuildId.Length > 0 && state.ChannelAccess.CanViewMembers) && state.View == "chat" && ChatWidth >= 560;
        groupPane.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
        groupColumn.Width = new GridLength(show ? 222 : 0);
    }
    private void ApplySocialState(bool changedChannel)
    {
        if (changedChannel) groupMembersOpen = true;
        var source = state.SelectedChannelKind == "group" ? state.GroupMembers : state.ChannelAccess.CanViewMembers ? state.GuildMembers : Array.Empty<ChannelMember>();
        bool memberCanAct = state.SignedIn && !state.Demo && !state.Busy;
        if (!ReferenceEquals(renderedMembers, source) || !ReferenceEquals(renderedMemberRelationships, state.Relationships) || renderedMemberCanAct != memberCanAct || renderedMemberSelfId != (state.CurrentUser?.Id ?? ""))
        {
            groupTitle.Text = $"MIEMBROS — {source.Count}" + (state.SelectedGuildId.Length > 0 && state.GuildMembersPartial ? " · lista parcial" : "");
            var rows = BuildMemberRows(source); groupList.ItemsSource = rows; renderedMembers = source;
            renderedMemberRelationships = state.Relationships; renderedMemberCanAct = memberCanAct; renderedMemberSelfId = state.CurrentUser?.Id ?? "";
            if (groupDialogList != null) groupDialogList.ItemsSource = rows;
        }
        moreMembers.Visibility = state.SelectedGuildId.Length > 0 && state.ChannelAccess.CanViewMembers && state.GuildMembersHasMore ? Visibility.Visible : Visibility.Collapsed;
        moreMembers.IsEnabled = !state.Busy;
        ApplyGroupWidth();
    }
    private void ToggleGroupMembers()
    {
        if (state.SelectedChannelKind != "group" && !(state.SelectedGuildId.Length > 0 && state.ChannelAccess.CanViewMembers)) return;
        if (state.SelectedChannelKind == "group") Emit("group-members");
        if (ChatWidth < 560)
        {
            var dialog = Dialog("Miembros del canal", 360, 560); var list = new ListBox(); Virtual(list); list.ItemTemplate = CellTemplate<UiMemberCell>();
            list.ItemsSource = BuildMemberRows(state.SelectedChannelKind == "group" ? state.GroupMembers : state.GuildMembers); groupDialogList = list; dialog.Closed += (_, _) => { if (ReferenceEquals(groupDialogList, list)) groupDialogList = null; }; dialog.Content = list; dialog.ShowDialog(); return;
        }
        groupMembersOpen = !groupMembersOpen; ApplyGroupWidth();
    }
    private void ShowGroupAdd()
    {
        if (state.SelectedChannelKind != "group" || !state.SignedIn || state.Demo) return;
        string channel = state.SelectedChannelId;
        var current = state.GroupMembers.Select(m => m.User.Id).ToHashSet();
        var choices = (state.GroupRecipientCandidates.Count > 0 ? state.GroupRecipientCandidates : state.Relationships.Where(r => r.Type == 1).Select(r => r.User)).Where(p => !current.Contains(p.Id) && p.Id != state.CurrentUser?.Id).ToArray();
        ChoosePerson("Añadir una persona al grupo", choices, p => Handle(new("group-add", p.Id, ChannelId: channel)));
    }
    private void ChoosePerson(string title, IReadOnlyList<Person> choices, Action<Person> selected)
    {
        var dialog = Dialog(title, 430, 540); var layout = new DockPanel { Margin = new Thickness(14) };
        var query = new TextBox { ToolTip = "Buscar por nombre o usuario", Margin = new Thickness(0, 0, 0, 10) }; DockPanel.SetDock(query, Dock.Top); layout.Children.Add(query);
        var list = new ListBox { DisplayMemberPath = nameof(Person.Name), ItemsSource = choices }; Virtual(list);
        void Pick() { if (list.SelectedItem is Person person) { dialog.Close(); selected(person); } }
        var button = Button("Añadir", Pick); DockPanel.SetDock(button, Dock.Bottom); layout.Children.Add(button); list.MouseDoubleClick += (_, _) => Pick();
        query.TextChanged += (_, _) => list.ItemsSource = choices.Where(p => p.Name.Contains(query.Text, StringComparison.OrdinalIgnoreCase) || p.Username.Contains(query.Text, StringComparison.OrdinalIgnoreCase)).Take(200).ToArray();
        layout.Children.Add(list); dialog.Content = layout; dialog.ShowDialog();
    }

    private async void ShowForwardLegacy(UiAction source)
    {
        if (attachedController == null) return;
        var original = source;
        var dialog = Dialog("Reenviar mensaje", 470, 570);
        var layout = new DockPanel { Margin = new Thickness(16) };
        var description = Text("Elige el destino y confirma el reenvío.", 14, Muted); description.Margin = new Thickness(0, 0, 0, 12); DockPanel.SetDock(description, Dock.Top); layout.Children.Add(description);
        var scopes = new ComboBox { DisplayMemberPath = nameof(NavItem.Name), ItemsSource = new[] { new NavItem("", "Conversaciones", "section") }.Concat(state.Guilds).ToArray(), SelectedIndex = 0, Margin = new Thickness(0, 0, 0, 10) }; DockPanel.SetDock(scopes, Dock.Top); layout.Children.Add(scopes);
        var query = new TextBox { ToolTip = "Buscar destino", Margin = new Thickness(0, 0, 0, 10) }; DockPanel.SetDock(query, Dock.Top); layout.Children.Add(query);
        var list = new ListBox { DisplayMemberPath = nameof(NavItem.Name) }; Virtual(list);
        IReadOnlyList<NavItem> targets = state.DirectMessages;
        void Filter() => list.ItemsSource = targets.Where(n => n.Name.Contains(query.Text, StringComparison.OrdinalIgnoreCase)).Take(500).ToArray();
        Filter(); query.TextChanged += (_, _) => Filter();
        using var lifetime = new CancellationTokenSource(); int version = 0;
        dialog.Closed += (_, _) => lifetime.Cancel();
        scopes.SelectionChanged += async (_, _) =>
        {
            int attempt = ++version; if (scopes.SelectedItem is not NavItem scope) return;
            try
            {
                targets = Array.Empty<NavItem>(); Filter(); description.Text = "Cargando destinos…";
                var fetched = scope.Id.Length == 0 ? state.DirectMessages : await attachedController.GetForwardChannelsAsync(scope.Id, lifetime.Token);
                if (lifetime.IsCancellationRequested || version != attempt) return;
                targets = fetched; Filter(); description.Text = "Elige el destino y confirma el reenvío.";
            }
            catch (OperationCanceledException) { }
            catch (Exception) { if (!lifetime.IsCancellationRequested) description.Text = "No se pudieron cargar esos canales."; }
        };
        var forward = Button("Reenviar", () =>
        {
            if (list.SelectedItem is not NavItem target) return;
            if (!Confirm("Reenviar mensaje", "¿Reenviar este mensaje a «" + target.Name + "»?")) return;
            dialog.Close(); Handle(original with { Kind = "forward", Text = target.Id });
        }); DockPanel.SetDock(forward, Dock.Bottom); layout.Children.Add(forward); layout.Children.Add(list); dialog.Content = layout;
        await Dispatcher.InvokeAsync(() => dialog.ShowDialog());
    }

    private UIElement BuildProfileView(Person person, ProfileDetails? details, string fallback)
    {
        var content = new StackPanel { MaxWidth = 780, HorizontalAlignment = HorizontalAlignment.Stretch };
        var back = Button("← Volver al chat", ReturnToPreviousChat); back.HorizontalAlignment = HorizontalAlignment.Left; back.Margin = new Thickness(0, 0, 0, 18); content.Children.Add(back);
        var identity = new DockPanel(); var avatar = UiPresenceViews.AvatarWithPresence(person, details?.Presence ?? "", 88); avatar.Margin = new Thickness(0, 0, 20, 12); DockPanel.SetDock(avatar, Dock.Left); identity.Children.Add(avatar);
        var names = new StackPanel(); names.Children.Add(new TextBlock { Text = person.DisplayName.Length > 0 ? person.DisplayName : person.Name, FontSize = 27, FontWeight = FontWeights.SemiBold, Foreground = Ink });
        if (person.Username.Length > 0) names.Children.Add(Text("@" + person.Username, 15, Muted));
        names.Children.Add(Text(details == null ? fallback : UiPresenceViews.PresenceLabel(details.Presence), 14, Muted));
        if (details != null) names.Children.Add(UiPresenceViews.StatusLine(details.CustomStatus, details.CustomStatusEmoji, 16));
        identity.Children.Add(names); content.Children.Add(identity);
        if (fallback.StartsWith("Cargando", StringComparison.Ordinal) || fallback.StartsWith("Discord no", StringComparison.Ordinal)) content.Children.Add(Text(fallback, 13, Muted));
        if (details == null) content.Children.Add(Text("Cargando perfil…", 14, Muted));
        else
        {
            if (details.GuildId.Length > 0)
            {
                var context = new StackPanel { Margin = new Thickness(0, 12, 0, 6) }; context.Children.Add(Text("EN " + details.GuildName.ToUpperInvariant(), 12, Muted));
                if (details.Nickname.Length > 0) context.Children.Add(Text("Apodo: " + details.Nickname, 15));
                var roles = new WrapPanel(); foreach (var role in details.GuildRoles ?? Array.Empty<GuildRole>()) roles.Children.Add(UiRoleViews.Badge(role));
                if (roles.Children.Count == 0) context.Children.Add(Text("Sin roles adicionales disponibles", 13, Muted)); else context.Children.Add(roles); content.Children.Add(context);
            }
            if (details.Bio.Length > 0) { var bio = UiPresenceViews.RichText(details.Bio, 16); bio.Margin = new Thickness(0, 12, 0, 14); content.Children.Add(bio); }
            var dates = new WrapPanel { Margin = new Thickness(0, 8, 0, 18) };
            dates.Children.Add(ProfileDate("CUENTA CREADA", details.CreatedAt));
            if (details.FriendsSince.HasValue || state.Relationships.Any(r => r.Type == 1 && r.User.Id == person.Id)) dates.Children.Add(ProfileDate("AMIGOS DESDE", details.FriendsSince));
            content.Children.Add(dates);
            var tabs = new TabControl { Background = UiTheme.Panel, Foreground = Ink, BorderBrush = line, Height = 330 };
            var activityPanel = new StackPanel { Margin = new Thickness(16) };
            foreach (var a in details.Activities.Where(a => a.Type != 4).Take(8))
            {
                var card = new Border { Background = UiTheme.Surface, CornerRadius = new CornerRadius(8), Padding = new Thickness(14), Margin = new Thickness(0, 0, 0, 10) };
                var row = new DockPanel();
                if (MediaImages.IsAllowed(a.LargeImageUrl)) { var image = new UiMediaImage(a.LargeImageUrl, 128) { Width = 64, Height = 64, Margin = new Thickness(0, 0, 14, 0) }; DockPanel.SetDock(image, Dock.Left); row.Children.Add(image); }
                var labels = new StackPanel(); labels.Children.Add(UiPresenceViews.RichText(UiPresenceViews.ActivityLabel(a), 16));
                if (a.StartedAt.HasValue) labels.Children.Add(Text("Desde " + a.StartedAt.Value.ToLocalTime().ToString("HH:mm"), 12, Muted));
                row.Children.Add(labels); card.Child = row; activityPanel.Children.Add(card);
            }
            if (activityPanel.Children.Count == 0) activityPanel.Children.Add(Text("Sin actividad disponible", 14, Muted));
            tabs.Items.Add(new TabItem { Header = "Actividad", Content = new ScrollViewer { Content = activityPanel, VerticalScrollBarVisibility = ScrollBarVisibility.Auto } });
            var friends = new ListBox(); Virtual(friends); friends.ItemTemplate = CellTemplate<UiMemberCell>();
            if (details.MutualFriendsAvailable) friends.ItemsSource = details.MutualFriends.Take(100).Select(friend => { var presence = state.UserPresences.GetValueOrDefault(friend.Id); return MemberModel(new ChannelMember(friend, presence?.Status ?? "", presence?.CustomStatus ?? "", presence?.CustomStatusEmoji, presence?.Activities)); }).ToArray();
            else friends.Items.Add(Text("Discord no proporcionó esta información", 14, Muted));
            tabs.Items.Add(new TabItem { Header = details.MutualFriendsAvailable ? $"Amigos en común ({details.MutualFriends.Count})" : "Amigos en común", Content = friends });
            var guilds = new ListBox(); Virtual(guilds); guilds.ItemTemplate = CellTemplate<UiMutualGuildCell>();
            if (details.MutualGuildsAvailable) guilds.ItemsSource = details.MutualGuilds.Take(100).Select(guild => new UiNavModel(guild, handleAction, false)).ToArray();
            else guilds.Items.Add(Text("Discord no proporcionó esta información", 14, Muted));
            tabs.Items.Add(new TabItem { Header = details.MutualGuildsAvailable ? $"Servidores en común ({details.MutualGuilds.Count})" : "Servidores en común", Content = guilds }); content.Children.Add(tabs);
        }
        if (person.Id != state.CurrentUser?.Id)
        {
            var actions = new WrapPanel { Margin = new Thickness(0, 16, 0, 6) };
            var sendMessage = Button("Enviar mensaje", () => Emit("open-dm", person.Id)); sendMessage.Margin = new Thickness(0, 0, 10, 0); sendMessage.IsEnabled = state.SignedIn && !state.Demo && !state.Busy; actions.Children.Add(sendMessage);
            if (!person.IsBot && relationshipTypes.GetValueOrDefault(person.Id) is var relationship && relationship != 1)
            {
                string label = relationship switch { 2 => "Usuario bloqueado", 3 => "Aceptar solicitud", 4 => "Solicitud enviada", _ => "Añadir amigo" };
                var add = Button(label, () => Handle(relationship == 3 ? new("accept-friend", person.Id) : new UiAction("add-friend", person.Id, person.Username)));
                add.IsEnabled = state.SignedIn && !state.Demo && !state.Busy && relationship is not (2 or 4) && (relationship == 3 || person.Username.Length > 0);
                add.ToolTip = person.Username.Length == 0 && relationship == 0 ? "El nombre de usuario aún no está disponible; espera a que cargue el perfil." : label; ToolTipService.SetShowOnDisabled(add, true); actions.Children.Add(add);
            }
            content.Children.Add(actions);
        }
        else
        { var ownStatus = Button("Cambiar mi estado", ShowPresenceMenu); ownStatus.HorizontalAlignment = HorizontalAlignment.Left; ownStatus.Margin = new Thickness(0, 16, 0, 6); content.Children.Add(ownStatus); }
        return new ScrollViewer { Content = content, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
    }
    private static UIElement ProfileDate(string title, DateTimeOffset? date)
    {
        var block = new StackPanel { Margin = new Thickness(0, 0, 32, 8) }; block.Children.Add(Text(title, 12, Muted)); block.Children.Add(Text(date?.ToLocalTime().ToString("d MMMM yyyy", CultureInfo.GetCultureInfo("es-ES")) ?? "No disponible", 15)); return block;
    }
}

public sealed record UiMemberModel(ChannelMember Member, Action<UiAction> Action, string SectionName = "", int SectionColor = 0, int RelationshipType = 0, bool IsSelf = false, bool CanAct = true);
public sealed class UiMemberCell : Border
{
    public UiMemberCell() { DataContextChanged += (_, _) => Render(); Padding = new Thickness(8); MouseLeftButtonUp += (_, _) => { if (DataContext is UiMemberModel model && model.SectionName.Length == 0 && model.Member.User.Id.Length > 0) model.Action(new("profile", model.Member.User.Id)); }; }
    private void Render()
    {
        ContextMenu = null;
        if (DataContext is not UiMemberModel model) { Child = null; return; }
        if (model.SectionName.Length > 0) { Child = TerminalWindow.Text(model.SectionName, 12, UiRoleViews.Color(model.SectionColor, true)); IsHitTestVisible = false; Padding = new Thickness(9, 17, 5, 6); return; }
        IsHitTestVisible = true; Padding = new Thickness(8);
        var person = model.Member; var row = new DockPanel();
        var avatar = UiPresenceViews.AvatarWithPresence(person.User, person.Presence, 36); avatar.Margin = new Thickness(0, 0, 10, 0); DockPanel.SetDock(avatar, Dock.Left); row.Children.Add(avatar);
        var labels = new StackPanel(); labels.Children.Add(new TextBlock { Text = (person.Nickname.Length > 0 ? person.Nickname : person.User.Name) + (person.IsOwner ? " ♛" : ""), FontSize = 14, Foreground = UiRoleViews.Color(person.RoleColor, true), TextTrimming = TextTrimming.CharacterEllipsis });
        labels.Children.Add(UiPresenceViews.StatusLine(person.CustomStatus, person.CustomStatusEmoji, 12));
        var activities = person.Activities ?? Array.Empty<PresenceActivity>();
        foreach (var a in activities.Where(a => a.Type != 4).Take(1)) labels.Children.Add(UiPresenceViews.CompactActivity(UiPresenceViews.ActivityLabel(a), 12, TerminalWindow.Muted));
        if (person.CustomStatus.Length == 0 && activities.Count == 0) labels.Children.Add(TerminalWindow.Text(UiPresenceViews.PresenceLabel(person.Presence), 12, TerminalWindow.Muted));
        row.Children.Add(labels); Child = row; Cursor = Cursors.Hand; ToolTip = UiPresenceViews.PresenceLabel(person.Presence);
        ContextMenu = UiMemberMenus.Build(model);
    }
}
