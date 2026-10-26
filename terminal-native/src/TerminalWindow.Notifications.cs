using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private readonly ListBox attentionRail = new();
    private readonly Border attentionDivider = new();
    private IReadOnlyList<NavItem> attentionItems = Array.Empty<NavItem>();
    private DesktopNotifications? desktopNotifications;
    private Controller? notificationController;
    private string notificationAccount = "";
    private readonly System.Windows.Threading.DispatcherTimer visibleReadTimer = new() { Interval = TimeSpan.FromMilliseconds(800) };

    private void BuildAttentionRail(DockPanel dock)
    {
        var section = new StackPanel();
        Virtual(attentionRail); attentionRail.ItemTemplate = CellTemplate<UiAttentionCell>(); attentionRail.MaxHeight = 240;
        attentionRail.PreviewKeyDown += (_, e) =>
        {
            if (e.Key != System.Windows.Input.Key.Enter || attentionRail.SelectedItem is not UiNavModel item) return;
            Emit("channel", item.Item.Id); e.Handled = true;
        };
        section.Children.Add(attentionRail);
        attentionDivider.Height = 1; attentionDivider.Background = line; attentionDivider.Margin = new Thickness(13, 8, 13, 8); section.Children.Add(attentionDivider);
        DockPanel.SetDock(section, Dock.Top); dock.Children.Add(section);
        messages.AddHandler(ScrollViewer.ScrollChangedEvent, new ScrollChangedEventHandler((_, _) => QueueVisibleRead()));
        Activated += (_, _) =>
        {
            QueueVisibleRead();
        };
        visibleReadTimer.Tick += (_, _) => { visibleReadTimer.Stop(); if (CanReadVisibleChannel()) Emit("mark-read", state.SelectedChannelId); };
        Closed += (_, _) =>
        {
            if (notificationController != null) notificationController.DesktopNoticeRequested -= OnDesktopNotice;
            visibleReadTimer.Stop(); desktopNotifications?.Dispose(); desktopNotifications = null;
        };
    }
    private void WireDesktopNotifications(Controller controller)
    {
        if (ReferenceEquals(notificationController, controller)) return;
        if (notificationController != null) notificationController.DesktopNoticeRequested -= OnDesktopNotice;
        notificationController = controller; controller.DesktopNoticeRequested += OnDesktopNotice;
    }
    private void OnDesktopNotice(DesktopNotice notice)
    {
        if (!Dispatcher.CheckAccess()) { Dispatcher.BeginInvoke(() => OnDesktopNotice(notice)); return; }
        if (!IsLoaded || !preferences.WindowsNotifications || !state.SignedIn || state.Demo || state.SelfPresence == "dnd") return;
        if (!notice.IsCall && IsActive && IsVisible && WindowState != WindowState.Minimized && state.View == "chat" && state.SelectedChannelId == notice.ChannelId && IsNearBottom(messages)) return;
        desktopNotifications ??= new DesktopNotifications(this, channel =>
        {
            Show(); if (WindowState == WindowState.Minimized) WindowState = WindowState.Normal;
            Activate(); Emit("notification-open", channel);
        });
        desktopNotifications.Show(notice.ChannelId, notice.Title, notice.Body, notice.IsCall, notice.AvatarUrl);
    }
    private void ApplyAttentionRail()
    {
        string accountId = state.CurrentUser?.Id ?? "";
        if (notificationAccount != accountId || !preferences.WindowsNotifications)
        { desktopNotifications?.Dispose(); desktopNotifications = null; notificationAccount = accountId; }
        var next = state.DirectMessages.Where(n => n.HasActiveCall || n.Unread && !n.IsMuted)
            .OrderByDescending(n => n.HasActiveCall).ThenByDescending(n => n.IncomingCallRinging).Take(50).ToArray();
        foreach (var previous in attentionItems)
        {
            var current = next.FirstOrDefault(n => n.Id == previous.Id);
            if (previous.IncomingCallRinging && current?.IncomingCallRinging != true || current?.Unread != true && current?.IncomingCallRinging != true) desktopNotifications?.DismissChannel(previous.Id);
        }
        if (!attentionItems.SequenceEqual(next))
        {
            attentionItems = next;
            attentionRail.ItemsSource = next.Select(n => new UiNavModel(n, handleAction, true, IsFriend(n.RecipientId), preferences.Compact, state.VoiceAvailable)).ToArray();
        }
        attentionRail.Visibility = attentionDivider.Visibility = next.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
        attentionRail.SelectedItem = (attentionRail.ItemsSource as UiNavModel[])?.FirstOrDefault(n => n.Item.Id == state.SelectedChannelId);
        QueueVisibleRead();
    }
    private bool CanReadVisibleChannel() => state.SignedIn && !state.Demo && IsActive && IsVisible && WindowState != WindowState.Minimized && state.View == "chat" && state.SelectedChannelId.Length > 0 && state.Messages.Count > 0 && IsNearBottom(messages) && state.DirectMessages.Concat(state.Channels).Any(n => n.Id == state.SelectedChannelId && n.Unread && NavigationOrder.MessageOrder(state.Messages[^1].Id) >= NavigationOrder.MessageOrder(n.LastMessageId));
    private void QueueVisibleRead() { if (CanReadVisibleChannel()) { if (!visibleReadTimer.IsEnabled) visibleReadTimer.Start(); } else visibleReadTimer.Stop(); }
}

public sealed class UiMutualGuildCell : Border
{
    public UiMutualGuildCell() { Padding = new Thickness(12, 8, 12, 8); DataContextChanged += (_, _) => Render(); }
    private void Render()
    {
        if (DataContext is not UiNavModel model) { Child = null; return; }
        var row = new DockPanel(); var icon = TerminalWindow.NavAvatar(model.Item, 40); icon.Margin = new Thickness(0, 0, 12, 0); DockPanel.SetDock(icon, Dock.Left); row.Children.Add(icon);
        var name = TerminalWindow.Text(model.Item.Name, 15); name.TextTrimming = TextTrimming.CharacterEllipsis; row.Children.Add(name); Child = row;
        Cursor = System.Windows.Input.Cursors.Hand; MouseLeftButtonUp -= Open; MouseLeftButtonUp += Open;
    }
    private void Open(object sender, System.Windows.Input.MouseButtonEventArgs e) { if (DataContext is UiNavModel model) model.Action(new("guild", model.Item.Id)); }
}

public sealed class UiAttentionCell : Border
{
    public UiAttentionCell()
    {
        Width = 52; Height = 54; Margin = new Thickness(2, 3, 2, 3); Cursor = System.Windows.Input.Cursors.Hand; DataContextChanged += (_, _) => Render();
        MouseLeftButtonUp += (_, e) => { if (DataContext is UiNavModel model) { model.Action(new("channel", model.Item.Id)); e.Handled = true; } };
    }
    private void Render()
    {
        if (DataContext is not UiNavModel model) { Child = null; return; }
        var item = model.Item;
        ToolTip = item.Name + (item.HasActiveCall ? item.IncomingCallRinging ? " · llamada entrante" : " · llamada activa" : " · mensajes sin leer");
        var grid = new Grid();
        var avatar = TerminalWindow.NavAvatar(item, 48); avatar.HorizontalAlignment = HorizontalAlignment.Center; avatar.VerticalAlignment = VerticalAlignment.Center; grid.Children.Add(avatar);
        if (item.HasActiveCall)
        {
            grid.Children.Add(new Border { Background = TerminalWindow.Brush("#248046"), BorderBrush = UiTheme.Rail, BorderThickness = new Thickness(2), CornerRadius = new CornerRadius(10), Width = 22, Height = 22, HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top,
                Child = new TextBlock { Text = "☎", Foreground = UiTheme.OnAccent, FontSize = 13, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center } });
        }
        if (item.Unread || item.MentionCount > 0)
        {
            var text = TerminalWindow.Text(item.MentionCount > 99 ? "99+" : item.MentionCount > 0 ? item.MentionCount.ToString() : "•", 12, UiTheme.OnAccent);
            text.HorizontalAlignment = HorizontalAlignment.Center; text.VerticalAlignment = VerticalAlignment.Center;
            grid.Children.Add(new Border { MinWidth = 21, Height = 21, Padding = new Thickness(3, 0, 3, 0), Background = TerminalWindow.Brush("#DA373C"), BorderBrush = UiTheme.Rail, BorderThickness = new Thickness(2), CornerRadius = new CornerRadius(11), HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Bottom, Child = text });
        }
        Child = grid;
        var menu = UiMenus.Create(); UiMenus.Add(menu, "Abrir conversación", () => model.Action(new("channel", item.Id)));
        if (item.HasActiveCall) UiMenus.Add(menu, "Unirse a la llamada", () => model.Action(new("join-call", item.Id)), model.CanCall);
        UiMenus.Add(menu, "Marcar como leído", () => model.Action(new("mark-read", item.Id))); ContextMenu = menu;
    }
}
