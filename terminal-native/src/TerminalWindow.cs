using System.Collections.ObjectModel;
using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Markup;
using Microsoft.Win32;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow : Window
{
    public event Action<UiAction>? ActionRequested;
    private AppSnapshot state = new();
    private readonly Grid shell = new();
    private readonly ListBox servers = new(), channels = new(), messages = new(), people = new();
    private readonly TextBox composer = new(), search = new();
    private readonly TextBlock connection = new(), status = new(), heading = new(), sideHeading = new(), account = new(), reply = new(), typing = new(), workspaceTitle = new();
    private readonly Border accountAvatar = new(), accountPresenceDot = new(), headerAvatar = new(), welcome = new(), peoplePane = new(), profilePane = new();
    private readonly Button send = new(), voice = new(), share = new(), older = new(), login = new(), mute = new(), deafen = new(), activity = new(), call = new(), addFriend = new();
    private readonly Button present = new();
    private readonly StackPanel composerPane = new();
    private Button? attachmentButton;
    private readonly List<Button> composerExtras = new();
    private readonly TextBlock composerPermissionHint = new() { TextWrapping = TextWrapping.Wrap, Foreground = UiTheme.Muted, FontSize = 12, Margin = new Thickness(6, 3, 6, 3) };
    private readonly UniformGrid footerVoiceControls = new() { Columns = 2, Margin = new Thickness(0, 6, 0, 0), Visibility = Visibility.Collapsed };
    private readonly Button videoCall = new(), pinned = new(), addGroup = new(), membersToggle = new();
    private readonly Border composerFrame = new();
    private readonly DockPanel replyRow = new();
    private readonly Border callPanel = new();
    private readonly TextBlock callTitle = new(), callStatus = new(), callDuration = new(), callMediaNote = new(), newMessageNotice = new();
    private readonly WrapPanel callParticipants = new();
    private readonly Button callMute = new(), callDeafen = new(), callScreen = new(), camera = new(), callDisconnect = new();
    private readonly System.Windows.Threading.DispatcherTimer callClock = new() { Interval = TimeSpan.FromSeconds(1) };
    private IReadOnlyList<CallParticipant>? renderedCallParticipants;
    private readonly ObservableCollection<UiMessageModel> messageModels = new();
    private readonly List<Button> signedInButtons = new();
    private readonly Action<UiAction> handleAction;
    private Person? renderedAccount;
    private IReadOnlyList<ChatMessage>? renderedMessages;
    private string? renderedMessageUserId;
    private bool renderedMessagesWritable, renderedMessagesDemo;
    private ChannelAccess? renderedMessageAccess;
    private bool renderedMessageCanPin;
    private string renderedMessageView = "";
    private IReadOnlyList<Relationship>? renderedRelationships;
    private string? renderedPeopleView;
    private bool renderedPeopleDemo;
    private long renderedPeoplePresenceVersion = -1;
    private int renderedWelcomeKind = -1;
    private Person? renderedProfile;
    private string? renderedProfileSubtitle;
    private IReadOnlyList<NavItem>? groupedDmSource, groupedDmItems;
    private int styleVersion, renderedStyleVersion = -1;
    private IReadOnlyList<Relationship>? relationshipIndexSource;
    private HashSet<string> friendIds = new();
    private Dictionary<string, int> relationshipTypes = new(StringComparer.Ordinal);
    private NavItem? renderedHeaderItem;
    private bool applying;
    private string replyId = "";
    private UiPreferences preferences;
    private readonly string preferencePath;
    private readonly SolidColorBrush bg = UiTheme.Background, panel = UiTheme.Panel, line = UiTheme.Line;
    internal static readonly SolidColorBrush Ink = UiTheme.Ink, Muted = UiTheme.Muted, Accent = UiTheme.Accent;
    private static readonly SolidColorBrush PresenceOnline = Brush("#23A55A"), PresenceIdle = Brush("#F0B232"), PresenceDnd = Brush("#F23F43");

    public TerminalWindow()
    {
        handleAction = Handle;
        Title = "NotDiscord Terminal — prototipo funcional";
        Width = 1360; Height = 860; MinWidth = 1000; MinHeight = 650;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        FontFamily = new FontFamily("Segoe UI"); FontSize = 15;
        Foreground = Ink;
        preferencePath = FindPreferences();
        preferences = ReadPreferences();
        BuildStyles();
        Build();
        UpdateTheme();
        ApplyZoom();
        PreviewKeyDown += ZoomKey;
        callClock.Tick += (_, _) => UpdateCallDuration();
        StateChanged += (_, _) => SyncCallClock(); IsVisibleChanged += (_, _) => SyncCallClock(); Closed += (_, _) => callClock.Stop();
        Apply(new());
    }

    private static string FindPreferences()
    {
        var folder = new DirectoryInfo(AppContext.BaseDirectory);
        while (folder != null && !folder.Name.Equals("terminal-native", StringComparison.OrdinalIgnoreCase)) folder = folder.Parent;
        return Path.Combine(folder?.FullName ?? AppContext.BaseDirectory, "data", "ui-settings.json");
    }
    private UiPreferences ReadPreferences()
    {
        try { return JsonSerializer.Deserialize<UiPreferences>(File.ReadAllText(preferencePath)) ?? new(); }
        catch { return new(); }
    }
    internal static SolidColorBrush Brush(string color)
    {
        var b = new SolidColorBrush((Color)ColorConverter.ConvertFromString(color)); b.Freeze(); return b;
    }
    private void BuildStyles()
    {
        Resources["ThemePanel"] = UiTheme.Panel; Resources["ThemeRail"] = UiTheme.Rail; Resources["ThemeSelected"] = UiTheme.Selected;
        var tooltip = new Style(typeof(ToolTip)); tooltip.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Menu)); tooltip.Setters.Add(new Setter(Control.ForegroundProperty, Ink)); tooltip.Setters.Add(new Setter(Control.BorderBrushProperty, line)); Resources[typeof(ToolTip)] = tooltip;
        var buttonStyle = new Style(typeof(Button));
        buttonStyle.Setters.Add(new Setter(Control.ForegroundProperty, Ink));
        buttonStyle.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Surface));
        buttonStyle.Setters.Add(new Setter(Control.BorderBrushProperty, Brushes.Transparent));
        buttonStyle.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(10, 6, 10, 6)));
        buttonStyle.Setters.Add(new Setter(Control.CursorProperty, Cursors.Hand));
        buttonStyle.Setters.Add(new Setter(Control.FontFamilyProperty, FontFamily));
        buttonStyle.Setters.Add(new Setter(Control.FontSizeProperty, 14d));
        buttonStyle.Setters.Add(new Setter(Control.HorizontalContentAlignmentProperty, HorizontalAlignment.Center));
        var template = new ControlTemplate(typeof(Button));
        var border = new FrameworkElementFactory(typeof(Border)); border.Name = "ButtonBorder";
        border.SetBinding(Border.BackgroundProperty, new System.Windows.Data.Binding("Background") { RelativeSource = new(RelativeSourceMode.TemplatedParent) });
        border.SetBinding(Border.BorderBrushProperty, new System.Windows.Data.Binding("BorderBrush") { RelativeSource = new(RelativeSourceMode.TemplatedParent) });
        border.SetValue(Border.BorderThicknessProperty, new Thickness(1)); border.SetValue(Border.CornerRadiusProperty, new CornerRadius(3));
        var presenter = new FrameworkElementFactory(typeof(ContentPresenter));
        presenter.SetBinding(FrameworkElement.MarginProperty, new System.Windows.Data.Binding("Padding") { RelativeSource = new(RelativeSourceMode.TemplatedParent) });
        presenter.SetBinding(FrameworkElement.HorizontalAlignmentProperty, new Binding("HorizontalContentAlignment") { RelativeSource = new(RelativeSourceMode.TemplatedParent) }); presenter.SetValue(FrameworkElement.VerticalAlignmentProperty, VerticalAlignment.Center);
        border.AppendChild(presenter); template.VisualTree = border;
        var hover = new Trigger { Property = IsMouseOverProperty, Value = true }; hover.Setters.Add(new Setter(Border.BackgroundProperty, UiTheme.Hover, "ButtonBorder")); template.Triggers.Add(hover);
        var disabled = new Trigger { Property = IsEnabledProperty, Value = false }; disabled.Setters.Add(new Setter(OpacityProperty, .38)); template.Triggers.Add(disabled);
        buttonStyle.Setters.Add(new Setter(Control.TemplateProperty, template)); Resources[typeof(Button)] = buttonStyle;
        var textStyle = new Style(typeof(TextBox));
        textStyle.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Input));
        textStyle.Setters.Add(new Setter(Control.ForegroundProperty, Ink));
        textStyle.Setters.Add(new Setter(TextBoxBase.CaretBrushProperty, Accent));
        textStyle.Setters.Add(new Setter(Control.BorderBrushProperty, Brushes.Transparent));
        textStyle.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(8)));
        textStyle.Setters.Add(new Setter(Control.FontFamilyProperty, FontFamily)); Resources[typeof(TextBox)] = textStyle;
        BuildChoiceStyles();
        var itemStyle = new Style(typeof(ListBoxItem));
        itemStyle.Setters.Add(new Setter(Control.HorizontalContentAlignmentProperty, HorizontalAlignment.Stretch));
        itemStyle.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(0)));
        itemStyle.Setters.Add(new Setter(Control.MarginProperty, new Thickness(4, 1, 4, 1)));
        itemStyle.Setters.Add(new Setter(Control.ForegroundProperty, Ink));
        var itemTemplate = new ControlTemplate(typeof(ListBoxItem));
        var ib = new FrameworkElementFactory(typeof(Border)); ib.Name = "RowBorder"; ib.SetValue(Border.BackgroundProperty, Brushes.Transparent); ib.SetValue(Border.CornerRadiusProperty, new CornerRadius(5));
        var ip = new FrameworkElementFactory(typeof(ContentPresenter)); ib.AppendChild(ip); itemTemplate.VisualTree = ib;
        var selected = new Trigger { Property = ListBoxItem.IsSelectedProperty, Value = true }; selected.Setters.Add(new Setter(Border.BackgroundProperty, UiTheme.Selected, "RowBorder")); itemTemplate.Triggers.Add(selected);
        var over = new Trigger { Property = IsMouseOverProperty, Value = true }; over.Setters.Add(new Setter(Border.BackgroundProperty, UiTheme.Surface, "RowBorder")); itemTemplate.Triggers.Add(over);
        itemStyle.Setters.Add(new Setter(Control.TemplateProperty, itemTemplate)); Resources[typeof(ListBoxItem)] = itemStyle;
        Resources[typeof(ScrollBar)] = (Style)XamlReader.Parse("""
        <Style xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" TargetType="ScrollBar">
          <Setter Property="Width" Value="10"/><Setter Property="Background" Value="{DynamicResource ThemePanel}"/>
          <Setter Property="Template"><Setter.Value><ControlTemplate TargetType="ScrollBar">
            <Grid Background="{TemplateBinding Background}"><Track x:Name="PART_Track" Orientation="{TemplateBinding Orientation}" Minimum="{TemplateBinding Minimum}" Maximum="{TemplateBinding Maximum}" Value="{TemplateBinding Value}" ViewportSize="{TemplateBinding ViewportSize}" IsDirectionReversed="True">
              <Track.DecreaseRepeatButton><RepeatButton x:Name="Decrease" Command="ScrollBar.PageUpCommand" Opacity="0" Focusable="False"/></Track.DecreaseRepeatButton>
              <Track.Thumb><Thumb><Thumb.Template><ControlTemplate TargetType="Thumb"><Border Background="{DynamicResource ThemeSelected}" CornerRadius="4" Margin="2"/></ControlTemplate></Thumb.Template></Thumb></Track.Thumb>
              <Track.IncreaseRepeatButton><RepeatButton x:Name="Increase" Command="ScrollBar.PageDownCommand" Opacity="0" Focusable="False"/></Track.IncreaseRepeatButton>
            </Track></Grid>
            <ControlTemplate.Triggers><Trigger Property="Orientation" Value="Horizontal"><Setter TargetName="PART_Track" Property="IsDirectionReversed" Value="False"/><Setter TargetName="Decrease" Property="Command" Value="{x:Static ScrollBar.PageLeftCommand}"/><Setter TargetName="Increase" Property="Command" Value="{x:Static ScrollBar.PageRightCommand}"/></Trigger></ControlTemplate.Triggers>
          </ControlTemplate></Setter.Value></Setter>
          <Style.Triggers><Trigger Property="Orientation" Value="Horizontal"><Setter Property="Width" Value="Auto"/><Setter Property="Height" Value="10"/></Trigger></Style.Triggers>
        </Style>
        """);
    }
    private void Build()
    {
        shell.RowDefinitions.Add(new() { Height = new GridLength(54) });
        shell.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        shell.RowDefinitions.Add(new() { Height = new GridLength(26) });
        shell.ColumnDefinitions.Add(new() { Width = new GridLength(72) });
        shell.ColumnDefinitions.Add(new() { Width = new GridLength(292) });
        shell.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        Content = BuildWindowChrome(shell);
        var brand = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(15, 0, 12, 0), VerticalAlignment = VerticalAlignment.Center };
        brand.Children.Add(DiscordLogo(27)); workspaceTitle.Text = "NotDiscord"; workspaceTitle.FontSize = 16; workspaceTitle.Margin = new Thickness(10, 0, 0, 0); workspaceTitle.VerticalAlignment = VerticalAlignment.Center; workspaceTitle.TextTrimming = TextTrimming.CharacterEllipsis; workspaceTitle.MaxWidth = 245; brand.Children.Add(workspaceTitle);
        var brandBorder = Frame(brand); Place(brandBorder, 0, 0, 2);
        var top = new Grid { Margin = new Thickness(18, 0, 14, 0) };
        top.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) }); top.ColumnDefinitions.Add(new() { Width = GridLength.Auto }); top.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        heading.FontSize = 17; heading.FontWeight = FontWeights.SemiBold; heading.TextTrimming = TextTrimming.CharacterEllipsis; heading.VerticalAlignment = VerticalAlignment.Center;
        var headerIdentity = new DockPanel(); headerAvatar.Width = 32; headerAvatar.Height = 32; headerAvatar.Margin = new Thickness(0, 0, 10, 0); DockPanel.SetDock(headerAvatar, Dock.Left); headerIdentity.Children.Add(headerAvatar); headerIdentity.Children.Add(heading); top.Children.Add(headerIdentity);
        var headerActions = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Center };
        UiIcons.Set(call, UiIcons.Phone); call.ToolTip = "Llamada de voz"; call.Click += (_, _) => Emit("call-dm", state.SelectedChannelId);
        UiIcons.Set(videoCall, UiIcons.Video); videoCall.ToolTip = "Videollamada · elegir cámara"; videoCall.Click += (_, _) => Emit("call-dm-video", state.SelectedChannelId);
        pinned.Content = new Image { Source = UiIcons.Pin, Width = 22, Height = 22 }; pinned.ToolTip = "Mensajes fijados"; pinned.Click += (_, _) => Emit("pins");
        UiIcons.Set(addGroup, UiIcons.AddPerson); addGroup.ToolTip = "Añadir personas al grupo"; addGroup.Click += (_, _) => ShowGroupAdd();
        UiIcons.Set(membersToggle, UiIcons.Members); membersToggle.ToolTip = "Mostrar u ocultar miembros"; membersToggle.Click += (_, _) => ToggleGroupMembers();
        foreach (var b in new[] { call, videoCall, pinned, addGroup, membersToggle }) { b.Width = 36; b.Height = 36; b.Padding = new Thickness(5); b.Margin = new Thickness(3, 0, 0, 0); headerActions.Children.Add(b); }
        Grid.SetColumn(headerActions, 1); top.Children.Add(headerActions);
        search.Width = 220; search.Height = 32; search.FontSize = 14; search.Padding = new Thickness(10, 4, 10, 4); search.Margin = new Thickness(10, 0, 10, 0); search.ToolTip = "Buscar mensajes · from: · has: · in: · before: · after:";
        search.KeyDown += (_, e) => { if (e.Key == Key.Enter && !string.IsNullOrWhiteSpace(search.Text)) { Emit("search", text: search.Text); e.Handled = true; } };
        var searchBox = new Grid(); searchBox.Children.Add(search);
        var searchHint = Text("Buscar mensajes  ⌕", 14, Muted); searchHint.Margin = new Thickness(20, 0, 20, 0); searchHint.IsHitTestVisible = false; searchBox.Children.Add(searchHint);
        search.TextChanged += (_, _) => searchHint.Visibility = search.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        WireSearchFilters();
        Grid.SetColumn(searchBox, 2); top.Children.Add(searchBox);
        Place(Frame(top), 0, 2);

        var serverDock = new DockPanel { Margin = new Thickness(0, 0, 0, 82) };
        var home = Button("", () => Emit("guild", "")); home.Content = DiscordLogo(32); home.ToolTip = "Inicio · mensajes y amigos"; home.Margin = new Thickness(12, 12, 12, 10); home.Width = home.Height = 48; home.Padding = new Thickness(0); home.HorizontalAlignment = HorizontalAlignment.Center; home.Background = UiTheme.Selected; DockPanel.SetDock(home, Dock.Top); serverDock.Children.Add(home);
        BuildAttentionRail(serverDock);
        Virtual(servers); servers.ItemTemplate = CellTemplate<UiServerCell>(); servers.SelectionChanged += (_, _) => { if (!applying && servers.SelectedItem is UiNavModel item) Emit("guild", item.Item.Id); };
        serverDock.Children.Add(servers); Place(Frame(serverDock), 1, 0);

        var navigation = new DockPanel();
        var navTop = new StackPanel { Margin = new Thickness(9, 10, 9, 5) };
        var friends = Button("☺  Amigos", () => Emit("view", "friends")); friends.HorizontalContentAlignment = HorizontalAlignment.Left;
        var requests = Button("↗  Solicitudes", () => Emit("view", "requests")); requests.Margin = new Thickness(0, 3, 0, 0);
        friends.MinHeight = 40; friends.FontSize = 16; requests.MinHeight = 40; requests.FontSize = 16;
        navTop.Children.Add(friends); navTop.Children.Add(requests); signedInButtons.AddRange(new[] { friends, requests });
        addFriend.Content = "+ Añadir amigo"; addFriend.Click += (_, _) => { var name = Prompt("Añadir amigo", "Nombre de usuario de Discord (sin @)"); if (!string.IsNullOrWhiteSpace(name)) Emit("add-friend", text: name.Trim()); };
        addFriend.Margin = new Thickness(0, 3, 0, 0); addFriend.FontSize = 13; navTop.Children.Add(addFriend); signedInButtons.Add(addFriend);
        sideHeading.Margin = new Thickness(5, 16, 0, 6); sideHeading.FontSize = 12; sideHeading.FontWeight = FontWeights.SemiBold; sideHeading.Foreground = Muted; navTop.Children.Add(sideHeading);
        DockPanel.SetDock(navTop, Dock.Top); navigation.Children.Add(navTop);
        var navBottom = new StackPanel { Margin = new Thickness(8, 5, 8, 82) };
        var invitation = Button("↳ Unirse con invitación", InviteDialog); invitation.ToolTip = "Aceptar un enlace o código de invitación"; signedInButtons.Add(invitation); navBottom.Children.Add(invitation);
        invitation.FontSize = 12; invitation.Padding = new Thickness(5, 4, 5, 4);
        footerVoiceControls.Children.Add(voice); footerVoiceControls.Children.Add(share); navBottom.Children.Add(footerVoiceControls);
        var accountRow = new Grid { Margin = new Thickness(8, 7, 5, 7) }; accountRow.ColumnDefinitions.Add(new() { Width = new GridLength(46) }); accountRow.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) }); accountRow.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        accountAvatar.Width = 40; accountAvatar.Height = 40; accountAvatar.Background = UiTheme.AccentSurface; accountAvatar.Clip = new EllipseGeometry(new Point(20, 20), 20, 20); accountRow.Children.Add(accountAvatar);
        account.FontSize = 15; account.FontWeight = FontWeights.SemiBold; account.TextTrimming = TextTrimming.CharacterEllipsis; account.VerticalAlignment = VerticalAlignment.Center; Grid.SetColumn(account, 1); accountRow.Children.Add(account);
        accountPresenceDot.Width = 13; accountPresenceDot.Height = 13; accountPresenceDot.CornerRadius = new CornerRadius(7); accountPresenceDot.BorderBrush = panel; accountPresenceDot.BorderThickness = new Thickness(2); accountPresenceDot.HorizontalAlignment = HorizontalAlignment.Right; accountPresenceDot.VerticalAlignment = VerticalAlignment.Bottom; accountPresenceDot.Margin = new Thickness(0, 0, 3, 0); accountPresenceDot.IsHitTestVisible = false; accountRow.Children.Add(accountPresenceDot);
        foreach (var identity in new FrameworkElement[] { accountAvatar, account })
        {
            identity.Cursor = Cursors.Hand; identity.Focusable = true;
            identity.MouseLeftButtonUp += (_, e) => { ShowPresenceMenu(); e.Handled = true; };
            identity.MouseRightButtonUp += (_, e) => { ShowPresenceMenu(); e.Handled = true; };
            identity.KeyDown += (_, e) => { if (e.Key is Key.Enter or Key.Space) { ShowPresenceMenu(); e.Handled = true; } };
        }
        var accountSettings = Button("⚙", PreferencesDialog); accountSettings.ToolTip = "Ajustes de apariencia y cuenta"; accountSettings.FontSize = 21;
        var personalControls = new StackPanel { Orientation = Orientation.Horizontal, Height = 42, Margin = new Thickness(5, 0, 0, 0) };
        mute.Click += (_, _) => Emit("toggle-mute"); deafen.Click += (_, _) => Emit("toggle-deafen");
        activity.Click += (_, _) => Emit("toggle-game-activity");
        foreach (var b in new[] { mute, deafen, activity, accountSettings })
        {
            b.Width = 36; b.Height = 40; b.Padding = new Thickness(5); b.Margin = new Thickness(1, 0, 1, 0); personalControls.Children.Add(b);
            if (b == mute || b == deafen)
            { var section = b == mute ? "input" : "output"; var devices = Button("⌄", () => Emit("media-settings", text: section)); devices.Width = 15; devices.Height = 40; devices.Padding = new Thickness(0); devices.ToolTip = b == mute ? "Dispositivo de entrada" : "Dispositivo de salida"; personalControls.Children.Add(devices); signedInButtons.Add(devices); }
        }
        Grid.SetColumn(personalControls, 2); accountRow.Children.Add(personalControls);
        var accountBox = new StackPanel(); accountBox.Children.Add(accountRow);
        login.Content = "Iniciar sesión"; login.Click += (_, _) => { if (state.SignedIn) { if (Confirm("Cerrar sesión", "¿Cerrar la sesión de este prototipo?")) Emit("logout"); } else Emit("login"); }; accountBox.Children.Add(login);
        var accountBar = new Border { Child = accountBox, Background = UiTheme.Panel, BorderBrush = line, BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(8), Margin = new Thickness(7), VerticalAlignment = VerticalAlignment.Bottom }; Panel.SetZIndex(accountBar, 5); Place(accountBar, 1, 0, 2);
        DockPanel.SetDock(navBottom, Dock.Bottom); navigation.Children.Add(navBottom);
        Virtual(channels); channels.ItemTemplate = CellTemplate<UiChannelCell>();
        channels.SelectionChanged += (_, _) => { if (!applying && channels.SelectedItem is UiNavModel item && item.Item.Kind is not ("category" or "section")) Emit("channel", item.Item.Id); };
        WireNavigationContextMenus(channels); WireNavigationContextMenus(servers); WireNavigationContextMenus(attentionRail);
        navigation.Children.Add(channels); Place(Frame(navigation), 1, 1);

        var chat = new Grid(); chat.RowDefinitions.Add(new() { Height = GridLength.Auto }); chat.RowDefinitions.Add(new() { Height = GridLength.Auto }); chat.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) }); chat.RowDefinitions.Add(new() { Height = GridLength.Auto });
        BuildCallPanel(); chat.Children.Add(callPanel);
        var tools = new DockPanel { Margin = new Thickness(18, 4, 18, 2) };
        var toolButtons = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right };
        voice.Content = "◉ Voz"; voice.ToolTip = "La conexión de voz se habilita cuando el backend está disponible"; voice.Click += (_, _) => Emit("voice", state.SelectedChannelId);
        share.Content = "▣ Pantalla"; share.Margin = new Thickness(7, 0, 0, 0); share.ToolTip = "Compartir pantalla requiere una sesión de voz compatible"; share.Click += (_, _) => Emit("share");
        DockPanel.SetDock(toolButtons, Dock.Right); tools.Children.Add(toolButtons);
        var more = Button("···", () => { }); more.Margin = new Thickness(7, 0, 0, 0); more.ToolTip = "Historial local e invitaciones";
        var extraMenu = UiMenus.Create();
        UiMenus.Add(extraMenu, "Ver registro local de mensajes", () => Emit("view", "history")); var history = (MenuItem)extraMenu.Items[^1];
        UiMenus.Add(extraMenu, "Volver al chat", () => { if (state.SelectedChannelId.Length > 0) Emit("channel", state.SelectedChannelId); }); var returnChat = (MenuItem)extraMenu.Items[^1];
        UiMenus.Add(extraMenu, "Crear invitación al canal (24 h)", () => { if (Confirm("Crear invitación", "¿Crear una invitación a este canal válida durante 24 horas y copiarla?")) Emit("create-invite"); }); var invite = (MenuItem)extraMenu.Items[^1];
        more.ContextMenu = extraMenu;
        more.Click += (_, _) => { extraMenu.PlacementTarget = more; extraMenu.Placement = PlacementMode.Bottom; history.IsEnabled = state.SignedIn && !state.Demo && state.SelectedChannelId.Length > 0; returnChat.IsEnabled = history.IsEnabled; invite.IsEnabled = history.IsEnabled && state.SelectedGuildId.Length > 0; extraMenu.IsOpen = true; };
        toolButtons.Children.Add(more);
        older.Content = "↑ Mensajes anteriores"; older.HorizontalAlignment = HorizontalAlignment.Left; older.Click += (_, _) => Emit("older"); DockPanel.SetDock(older, Dock.Left); tools.Children.Add(older);
        present.Content = "Ir al presente"; present.Margin = new Thickness(7, 0, 0, 0); present.Click += (_, _) => GoToPresent(); DockPanel.SetDock(present, Dock.Left); tools.Children.Add(present);
        newMessageNotice.Foreground = Accent; newMessageNotice.FontSize = 13; newMessageNotice.Margin = new Thickness(12, 0, 0, 0); newMessageNotice.VerticalAlignment = VerticalAlignment.Center; newMessageNotice.TextTrimming = TextTrimming.CharacterEllipsis; tools.Children.Add(newMessageNotice); Grid.SetRow(tools, 1); chat.Children.Add(tools);
        var center = new Grid(); center.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) }); center.ColumnDefinitions.Add(groupColumn); Grid.SetRow(center, 2); chat.Children.Add(center);
        BuildGroupSidebar(center);
        Virtual(messages); messages.SelectionMode = SelectionMode.Single; messages.ItemTemplate = CellTemplate<UiMessageCell>(); messages.ItemsSource = messageModels; messages.Padding = new Thickness(10, 4, 8, 12); center.Children.Add(messages);
        welcome.Padding = new Thickness(40); welcome.HorizontalAlignment = HorizontalAlignment.Stretch; welcome.VerticalAlignment = VerticalAlignment.Stretch; center.Children.Add(welcome);
        Virtual(people); people.ItemTemplate = CellTemplate<UiPersonCell>(); peoplePane.Padding = new Thickness(18); peoplePane.Child = people; center.Children.Add(peoplePane);
        profilePane.Padding = new Thickness(28); center.Children.Add(profilePane);

        composerPane.Margin = new Thickness(7, 2, 7, 2);
        var cancelReply = Button("×", () => { replyId = ""; reply.Text = ""; replyRow.Visibility = Visibility.Collapsed; }); cancelReply.Margin = new Thickness(8, 0, 0, 4); DockPanel.SetDock(cancelReply, Dock.Right); replyRow.Children.Add(cancelReply); reply.Foreground = Accent; reply.TextTrimming = TextTrimming.CharacterEllipsis; reply.VerticalAlignment = VerticalAlignment.Center; replyRow.Children.Add(reply);
        replyRow.Visibility = Visibility.Collapsed;
        reply.Cursor = Cursors.Hand; reply.ToolTip = "Ver mensaje original";
        reply.MouseLeftButtonUp += (_, e) => { if (replyId.Length > 0) Handle(new("jump-message", replyId, ChannelId: state.SelectedChannelId)); e.Handled = true; };
        composerPane.Children.Add(replyRow);
        composer.AcceptsReturn = true; composer.TextWrapping = TextWrapping.Wrap; composer.MinHeight = 44; composer.MaxHeight = 160; composer.FontSize = 16; composer.Padding = new Thickness(8, 10, 8, 9); composer.Background = Brushes.Transparent; composer.BorderThickness = new Thickness(0); composer.VerticalScrollBarVisibility = ScrollBarVisibility.Auto; composer.ToolTip = "Escribe un mensaje · Enter envía · Shift+Enter crea una línea";
        composer.PreviewKeyDown += (_, e) => { if (HandleComposerMentionKey(e)) return; if (e.Key == Key.Enter && (Keyboard.Modifiers & ModifierKeys.Shift) == 0) { SendMessage(); e.Handled = true; } };
        var composerLine = new Grid(); composerLine.ColumnDefinitions.Add(new() { Width = GridLength.Auto }); composerLine.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) }); composerLine.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        var attach = Button("+", AttachFile); attach.FontSize = 24; attach.ToolTip = "Adjuntar archivo"; attach.Width = 38; attach.Padding = new Thickness(4); attach.VerticalAlignment = VerticalAlignment.Bottom; attach.Height = 44; composerLine.Children.Add(attach); signedInButtons.Add(attach);
        attachmentButton = attach; ToolTipService.SetShowOnDisabled(attach, true);
        var composeBox = new Grid(); composeBox.Children.Add(composer); var composeHint = Text("Escribe un mensaje…", 16, Muted); composeHint.Margin = new Thickness(8, 0, 0, 0); composeHint.IsHitTestVisible = false; composeBox.Children.Add(composeHint); composer.TextChanged += (_, _) => composeHint.Visibility = composer.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed; Grid.SetColumn(composeBox, 1); composerLine.Children.Add(composeBox);
        var actions = new StackPanel { Orientation = Orientation.Horizontal, VerticalAlignment = VerticalAlignment.Bottom, Height = 44 };
        var gif = Button("GIF", GifDialog); gif.ToolTip = "GIF: guardados, populares y búsqueda"; var emoji = Button("☺", EmojiDialog); emoji.FontSize = 25; emoji.ToolTip = "Emojis de Windows y de tus servidores";
        composerExtras.AddRange(new[] { gif, emoji });
        send.Content = "➤"; send.ToolTip = "Enviar mensaje"; send.FontSize = 21; send.Click += (_, _) => SendMessage();
        foreach (var b in new[] { gif, emoji, send }) { b.Margin = new Thickness(1, 0, 1, 0); b.Padding = new Thickness(7, 4, 7, 4); actions.Children.Add(b); if (b != send) signedInButtons.Add(b); }
        Grid.SetColumn(actions, 2); composerLine.Children.Add(actions); composerPane.Children.Add(composerLine);
        typing.FontSize = 12; typing.Foreground = Muted; typing.Margin = new Thickness(45, 0, 4, 3); composerPane.Children.Add(typing);
        composerFrame.Background = UiTheme.Surface; composerFrame.CornerRadius = new CornerRadius(8); composerFrame.Margin = new Thickness(18, 6, 18, 14); composerFrame.Child = composerPane; Grid.SetRow(composerFrame, 3); chat.Children.Add(composerFrame); Place(chat, 1, 2);

        var footer = new Grid { Margin = new Thickness(14, 0, 14, 0) }; footer.ColumnDefinitions.Add(new() { Width = GridLength.Auto }); footer.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        connection.VerticalAlignment = VerticalAlignment.Center; connection.Foreground = Accent; connection.FontSize = 11; footer.Children.Add(connection);
        status.VerticalAlignment = VerticalAlignment.Center; status.Foreground = Muted; status.FontSize = 11; status.Margin = new Thickness(18, 0, 0, 0); status.TextTrimming = TextTrimming.CharacterEllipsis; Grid.SetColumn(status, 1); footer.Children.Add(status); Place(Frame(footer), 2, 0, 3);
        WireComposerDrafts();
        WireChatNavigation();
        composerPane.Children.Add(composerPermissionHint);
    }

    private void BuildCallPanel()
    {
        callPanel.Background = UiTheme.Panel; callPanel.BorderBrush = line; callPanel.BorderThickness = new Thickness(0, 0, 0, 1); callPanel.Padding = new Thickness(20, 14, 20, 12); callPanel.Visibility = Visibility.Collapsed;
        var layout = new StackPanel(); var header = new DockPanel();
        callDuration.FontSize = 13; callDuration.Foreground = Muted; callDuration.VerticalAlignment = VerticalAlignment.Center; DockPanel.SetDock(callDuration, Dock.Right); header.Children.Add(callDuration);
        callTitle.FontSize = 18; callTitle.FontWeight = FontWeights.SemiBold; callTitle.TextTrimming = TextTrimming.CharacterEllipsis; header.Children.Add(callTitle); layout.Children.Add(header);
        callStatus.Foreground = Accent; callStatus.FontSize = 13; callStatus.Margin = new Thickness(0, 4, 0, 9); callStatus.TextWrapping = TextWrapping.Wrap; layout.Children.Add(callStatus);
        var peopleScroll = new ScrollViewer { Content = callParticipants, MaxHeight = 126, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled }; layout.Children.Add(peopleScroll);
        var controls = new WrapPanel { Margin = new Thickness(0, 10, 0, 0) };
        callMute.Click += (_, _) => Emit("toggle-mute"); callDeafen.Click += (_, _) => Emit("toggle-deafen"); callScreen.Click += (_, _) => Emit("share"); camera.Click += (_, _) => Emit("toggle-camera"); callDisconnect.Click += (_, _) => Emit("disconnect-voice");
        foreach (var control in new[] { callMute, callDeafen, callScreen, camera, callDisconnect }) { control.Padding = new Thickness(12, 7, 12, 7); control.Margin = new Thickness(0, 0, 8, 0); controls.Children.Add(control); }
        callDisconnect.Content = "Salir de la llamada"; callDisconnect.Foreground = UiTheme.Danger; layout.Children.Add(controls);
        callMediaNote.Text = "Vídeo activo · la previsualización local y el vídeo recibido todavía no se muestran en este panel."; callMediaNote.Foreground = Muted; callMediaNote.FontSize = 12; callMediaNote.TextWrapping = TextWrapping.Wrap; callMediaNote.Margin = new Thickness(0, 8, 0, 0); layout.Children.Add(callMediaNote); callPanel.Child = layout;
    }
    private void ApplyCallState(bool canAct)
    {
        bool active = state.VoiceConnected || state.VoiceConnecting || state.ScreenPreparing || state.CameraPreparing;
        callPanel.Visibility = active ? Visibility.Visible : Visibility.Collapsed;
        callTitle.Text = "Llamada" + (state.CallChannelName.Length > 0 ? " · " + state.CallChannelName : "");
        callStatus.Text = state.CallStatusLabel.Length > 0 ? state.CallStatusLabel : state.VoiceConnecting ? "Conectando audio…" : state.VoiceConnected ? "Audio conectado" : "Sin conexión de voz";
        if (state.ScreenSharing) callStatus.Text += state.ScreenPaused ? " · Pantalla en pausa por conexión cifrada" : " · Compartiendo pantalla";
        if (state.CameraEnabled) callStatus.Text += " · Cámara encendida";
        callMediaNote.Visibility = state.CameraEnabled || state.CallParticipants.Any(p => p.SelfVideo) ? Visibility.Visible : Visibility.Collapsed;
        callMute.Content = state.SelfMute ? "Activar micrófono" : "Silenciar micrófono"; callDeafen.Content = state.SelfDeaf ? "Activar audio" : "Ensordecer";
        callScreen.Content = state.ScreenPaused ? "Cancelar pantalla en pausa" : state.ScreenPreparing ? "Preparando pantalla…" : state.ScreenSharing ? "Dejar de compartir" : "Compartir pantalla";
        share.Content = state.ScreenPaused ? "Ⅱ Pantalla" : state.ScreenSharing ? "■ Pantalla" : "▣ Pantalla"; share.ToolTip = state.ScreenPaused ? "Cancelar la pantalla en pausa" : state.ScreenSharing ? "Dejar de compartir pantalla" : "Compartir pantalla";
        camera.Content = state.CameraPreparing ? "Preparando cámara…" : state.CameraEnabled ? "Apagar cámara" : "Encender cámara";
        camera.ToolTip = state.CameraAvailable ? "La cámara se activa sólo cuando pulsas este botón y eliges un dispositivo" : "La cámara no está disponible en esta sesión";
        callMute.IsEnabled = callDeafen.IsEnabled = canAct && state.VoiceConnected;
        callScreen.IsEnabled = canAct && (state.ScreenSharing || state.VoiceConnected && state.ScreenAvailable && !state.ScreenPreparing);
        camera.IsEnabled = canAct && (state.CameraEnabled || state.VoiceConnected && state.CameraAvailable && !state.CameraPreparing);
        callDisconnect.IsEnabled = canAct && (state.VoiceConnected || state.VoiceConnecting);
        if (active && (renderedCallParticipants == null || !renderedCallParticipants.SequenceEqual(state.CallParticipants)))
        {
            callParticipants.Children.Clear();
            foreach (var person in state.CallParticipants.Take(50))
            {
                var tile = new StackPanel { Width = 112, Margin = new Thickness(0, 0, 10, 0) };
                var avatar = new Border { Child = RoundAvatar(person.User, 42), BorderBrush = person.Speaking ? PresenceOnline : Brushes.Transparent, BorderThickness = new Thickness(3), CornerRadius = new CornerRadius(26), HorizontalAlignment = HorizontalAlignment.Center };
                tile.Children.Add(avatar);
                var name = Text(person.User.Name + (person.IsSelf && !person.User.Name.Equals("tú", StringComparison.OrdinalIgnoreCase) ? " (tú)" : ""), 13); name.TextAlignment = TextAlignment.Center; name.TextTrimming = TextTrimming.CharacterEllipsis; name.Margin = new Thickness(0, 3, 0, 0); tile.Children.Add(name);
                var flags = new List<string>(); if (person.SelfDeaf) flags.Add("sin audio"); else if (person.SelfMute) flags.Add("micro apagado"); else if (person.Speaking) flags.Add("hablando"); if (person.SelfVideo) flags.Add("cámara"); if (person.ScreenSharing) flags.Add(person.IsSelf && state.ScreenPaused ? "pantalla en pausa" : "pantalla");
                var details = Text(string.Join(" · ", flags), 11, Muted); details.TextAlignment = TextAlignment.Center; details.TextTrimming = TextTrimming.CharacterEllipsis; tile.Children.Add(details); tile.ToolTip = person.User.Name + (flags.Count > 0 ? " · " + string.Join(" · ", flags) : ""); callParticipants.Children.Add(tile);
            }
            if (state.CallParticipants.Count == 0) callParticipants.Children.Add(Text(state.VoiceConnecting ? "Esperando a la conexión de voz…" : "Esperando información de participantes…", 13, Muted));
            renderedCallParticipants = state.CallParticipants.ToArray();
        }
        if (!active && callParticipants.Children.Count > 0) { callParticipants.Children.Clear(); renderedCallParticipants = null; }
        UpdateCallDuration(); SyncCallClock();
    }
    private void UpdateCallDuration()
    {
        if (state.CallStartedAt is not DateTimeOffset started) { callDuration.Text = state.VoiceConnecting ? "" : "Duración no disponible"; return; }
        var elapsed = DateTimeOffset.UtcNow - started; if (elapsed < TimeSpan.Zero) elapsed = TimeSpan.Zero;
        callDuration.Text = elapsed.TotalHours >= 1 ? $"{(int)elapsed.TotalHours}:{elapsed.Minutes:00}:{elapsed.Seconds:00}" : $"{(int)elapsed.TotalMinutes}:{elapsed.Seconds:00}";
    }
    private void SyncCallClock()
    {
        bool run = state.VoiceConnected && state.CallStartedAt != null && !state.Demo && IsVisible && WindowState != WindowState.Minimized;
        if (run && !callClock.IsEnabled) callClock.Start(); else if (!run && callClock.IsEnabled) callClock.Stop();
    }

    public void Apply(AppSnapshot snapshot)
    {
        if (!Dispatcher.CheckAccess()) { Dispatcher.Invoke(() => Apply(snapshot)); return; }
        bool changedChannel = state.SelectedChannelId != snapshot.SelectedChannelId;
        bool enteringPins = snapshot.View == "pins" && (state.View != "pins" || changedChannel);
        BeforeApplyComposer(snapshot);
        state = snapshot; applying = true;
        try
        {
            if (!ReferenceEquals(relationshipIndexSource, state.Relationships)) { friendIds = state.Relationships.Where(r => r.Type == 1).Select(r => r.User.Id).ToHashSet(); relationshipTypes = state.Relationships.GroupBy(r => r.User.Id).ToDictionary(g => g.Key, g => g.Last().Type, StringComparer.Ordinal); relationshipIndexSource = state.Relationships; }
            string workspaceName = state.SelectedGuildId.Length > 0 ? state.Guilds.FirstOrDefault(g => g.Id == state.SelectedGuildId)?.Name ?? "Servidor" : "NotDiscord";
            Title = state.Demo ? "NotDiscord Terminal — DEMO SIN CONEXIÓN" : (state.UnreadMessageCount > 0 ? $"({state.UnreadMessageCount}) " : "") + workspaceName + " — " + state.ChannelName;
            workspaceTitle.Text = workspaceName; workspaceTitle.ToolTip = workspaceName;
            connection.Text = state.Demo ? "● DEMO · SIN CONEXIÓN" : "● " + state.Connection;
            status.Text = state.Status; status.ToolTip = state.Status;
            heading.Text = state.View is "friends" ? "Amigos" : state.View is "requests" ? "Solicitudes" : state.View == "profile" ? "Perfil" : state.View == "search" ? "Resultados de búsqueda" : state.View == "pins" ? "Mensajes fijados" : (state.SelectedGuildId.Length > 0 && state.SelectedChannelId.Length > 0 ? "# " : "") + state.ChannelName;
            sideHeading.Text = state.SelectedGuildId.Length > 0 ? "CANALES" : "CONVERSACIONES";
            sideHeading.Visibility = state.SelectedGuildId.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            addFriend.Visibility = state.View is "friends" or "requests" ? Visibility.Visible : Visibility.Collapsed;
            var headerItem = state.SelectedGuildId.Length == 0 && state.View == "chat" ? state.DirectMessages.FirstOrDefault(n => n.Id == state.SelectedChannelId) : null;
            headerAvatar.Visibility = headerItem != null ? Visibility.Visible : Visibility.Collapsed;
            if (headerItem != renderedHeaderItem) { headerAvatar.Child = headerItem == null ? null : NavAvatar(headerItem, 32); renderedHeaderItem = headerItem; }
            login.Content = state.SignedIn ? "Cerrar sesión" : "Iniciar sesión"; login.IsEnabled = !state.Busy && !state.Demo; login.Visibility = state.SignedIn ? Visibility.Collapsed : Visibility.Visible;
            account.Text = state.CurrentUser?.Name ?? (state.Demo ? "Vista de demostración" : "Sin sesión");
            bool knownPresence = state.SelfPresence is "online" or "idle" or "dnd" or "invisible";
            accountPresenceDot.Visibility = knownPresence && (state.SignedIn || state.Demo) ? Visibility.Visible : Visibility.Collapsed;
            accountPresenceDot.Background = state.SelfPresence switch { "online" => PresenceOnline, "idle" => PresenceIdle, "dnd" => PresenceDnd, _ => Muted };
            string presenceTip = state.SignedIn ? (knownPresence ? "Estado: " + PresenceLabel(state.SelfPresence) : "Estado desconocido") + " · Cambiar estado" : state.Demo ? "Cuenta de demostración" : "Inicia sesión para elegir tu estado";
            account.ToolTip = presenceTip; accountAvatar.ToolTip = presenceTip;
            account.IsEnabled = accountAvatar.IsEnabled = state.SignedIn && !state.Demo && !state.Busy;
            if (accountAvatar.Child == null || renderedAccount != state.CurrentUser)
            {
                accountAvatar.Child = Avatar(state.CurrentUser, 40);
                renderedAccount = state.CurrentUser;
            }
            SyncNavigation(servers, state.Guilds, false, state.SelectedGuildId);
            ApplyAttentionRail();
            SyncNavigation(channels, state.SelectedGuildId.Length > 0 ? state.Channels : GroupDirectMessages(state.DirectMessages), state.SelectedGuildId.Length == 0, state.SelectedChannelId);
            bool chatView = state.SelectedChannelId.Length > 0 && state.View is not ("friends" or "requests" or "welcome" or "profile");
            // Preserve virtualized containers for unchanged messages instead of rebuilding the entire timeline.
            var incoming = state.View == "pins" ? state.PinnedMessages : state.Messages;
            if (changedChannel || incoming.Count == 0 || messageModels.Count > 0 && incoming.Count > 0 && messageModels[0].Message.ChannelId != incoming[0].ChannelId)
            {
                if (messageModels.Count > 0) messageModels.Clear();
                renderedMessages = null;
            }
            bool writable = state.SignedIn && state.View == "chat";
            if (chatView && (!ReferenceEquals(renderedMessages, incoming) || renderedMessagesWritable != writable || renderedMessagesDemo != state.Demo || renderedMessageUserId != state.CurrentUser?.Id || renderedStyleVersion != styleVersion || renderedMessageAccess != state.ChannelAccess || renderedMessageCanPin != state.CanPinMessages || renderedMessageView != state.View))
            {
                for (int i = 0; i < incoming.Count; i++)
                {
                    var msg = incoming[i];
                    bool own = msg.Author.Id == state.CurrentUser?.Id;
                    bool continuation = state.View == "chat" && i > 0 && CanGroup(incoming[i - 1], msg);
                    bool dayStart = state.View == "chat" && (i == 0 || incoming[i - 1].Timestamp.LocalDateTime.Date != msg.Timestamp.LocalDateTime.Date);
                    string channelLabel = state.View == "search" ? state.Channels.Concat(state.DirectMessages).FirstOrDefault(c => c.Id == msg.ChannelId)?.Name ?? msg.ChannelId : "";
                    if (i < messageModels.Count && messageModels[i].Message == msg && messageModels[i].SignedIn == writable && messageModels[i].Demo == state.Demo && messageModels[i].Own == own && messageModels[i].Continuation == continuation && messageModels[i].ChatSize == preferences.ChatSize && messageModels[i].Compact == preferences.Compact && messageModels[i].ChannelLabel == channelLabel && messageModels[i].DayStart == dayStart && messageModels[i].CanPin == state.CanPinMessages && messageModels[i].PinsView == (state.View == "pins") && messageModels[i].Access == state.ChannelAccess) continue;
                    var model = new UiMessageModel(msg, handleAction, own, state.Demo, writable, continuation, preferences.ChatSize, preferences.Compact, channelLabel, dayStart, state.CanPinMessages, state.View == "pins", state.ChannelAccess);
                    if (i < messageModels.Count && messageModels[i].Message.Id == msg.Id) messageModels[i] = model;
                    else if (i < messageModels.Count && incoming.Skip(i + 1).Any(m => m.Id == messageModels[i].Message.Id)) messageModels.Insert(i, model);
                    else if (i < messageModels.Count) messageModels[i] = model;
                    else messageModels.Add(model);
                }
                while (messageModels.Count > incoming.Count) messageModels.RemoveAt(messageModels.Count - 1);
                renderedMessages = incoming;
                renderedMessagesWritable = writable; renderedMessagesDemo = state.Demo; renderedMessageUserId = state.CurrentUser?.Id;
                renderedMessageAccess = state.ChannelAccess; renderedMessageCanPin = state.CanPinMessages; renderedMessageView = state.View;
                renderedStyleVersion = styleVersion;
            }
            messages.Visibility = chatView ? Visibility.Visible : Visibility.Collapsed;
            composerFrame.Visibility = chatView && state.View == "chat" ? Visibility.Visible : Visibility.Collapsed;
            bool membersView = state.View is "friends" or "requests";
            peoplePane.Visibility = membersView ? Visibility.Visible : Visibility.Collapsed;
            if (membersView && (renderedPeopleView != state.View || renderedPeopleDemo != state.Demo || renderedPeoplePresenceVersion != state.PresenceVersion || !SameRelationships(renderedRelationships, state.Relationships)))
            {
                people.ItemsSource = state.Relationships.Where(r => state.View == "requests" ? r.Type is 3 or 4 : r.Type == 1).Select(r => new UiPersonModel(r, handleAction, state.Demo, state.UserPresences.GetValueOrDefault(r.User.Id))).ToArray();
                renderedRelationships = state.Relationships; renderedPeopleView = state.View; renderedPeopleDemo = state.Demo; renderedPeoplePresenceVersion = state.PresenceVersion;
            }
            else if (!state.SignedIn && !state.Demo && people.ItemsSource != null)
            {
                people.ItemsSource = null; renderedRelationships = null; renderedPeopleView = null;
            }
            bool profileVisible = state.View == "profile" && state.ProfileUser != null;
            profilePane.Visibility = profileVisible ? Visibility.Visible : Visibility.Collapsed;
            if (!state.SignedIn && !state.Demo) { profilePane.Child = null; renderedProfile = null; renderedProfileDetails = null; renderedProfileSubtitle = null; }
            if (profileVisible && (renderedProfile != state.ProfileUser || renderedProfileSubtitle != state.ProfileSubtitle || renderedProfileDetails != state.ProfileDetails || !ReferenceEquals(renderedProfileRelationships, state.Relationships) || renderedProfileBusy != state.Busy || profilePane.Child == null))
            { profilePane.Child = ProfileContent(state.ProfileUser!, state.ProfileSubtitle); renderedProfile = state.ProfileUser; renderedProfileSubtitle = state.ProfileSubtitle; renderedProfileDetails = state.ProfileDetails; renderedProfileRelationships = state.Relationships; renderedProfileBusy = state.Busy; }
            bool welcomeVisible = !chatView && !membersView && !profileVisible;
            welcome.Visibility = welcomeVisible ? Visibility.Visible : Visibility.Collapsed;
            int welcomeKind = state.Demo ? 2 : state.SignedIn ? 1 : 0;
            if (welcomeVisible && (welcome.Child == null || renderedWelcomeKind != welcomeKind))
            { welcome.Child = WelcomeContent(); renderedWelcomeKind = welcomeKind; }
            bool canAct = state.SignedIn && !state.Demo && !state.Busy;
            foreach (var b in signedInButtons) b.IsEnabled = canAct;
            composer.IsEnabled = canAct && state.View == "chat" && state.ChannelAccess.CanSend;
            send.IsEnabled = composer.IsEnabled && (stagedAttachments.Count == 0 || state.ChannelAccess.CanAttach);
            string writeReason = !state.ChannelAccess.Known ? state.ChannelAccess.Reason : !state.ChannelAccess.CanReadHistory ? "No tienes permiso para consultar el historial de este canal." : "No tienes permiso para enviar mensajes en este canal.";
            composerPermissionHint.Text = state.ChannelAccess.CanSend ? "" : writeReason; composerPermissionHint.Visibility = composerPermissionHint.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            composer.ToolTip = state.ChannelAccess.CanSend ? "Escribe un mensaje · Enter envía · Shift+Enter crea una línea" : writeReason; ToolTipService.SetShowOnDisabled(composer, true);
            if (attachmentButton != null) { attachmentButton.IsEnabled = composer.IsEnabled && state.ChannelAccess.CanAttach; attachmentButton.ToolTip = attachmentButton.IsEnabled ? "Preparar archivos adjuntos" : !state.ChannelAccess.CanSend ? writeReason : "Este canal no permite adjuntar archivos."; }
            foreach (var extra in composerExtras) { extra.IsEnabled = composer.IsEnabled; ToolTipService.SetShowOnDisabled(extra, true); extra.ToolTip = !extra.IsEnabled ? writeReason : extra.Content?.ToString() == "GIF" ? "GIF: guardados, populares y búsqueda" : "Emojis de Windows y de tus servidores"; }
            search.IsEnabled = canAct && chatView && state.ChannelAccess.CanView && state.ChannelAccess.CanReadHistory;
            voice.IsEnabled = canAct && state.VoiceAvailable; share.IsEnabled = canAct && (state.ScreenSharing || state.ScreenAvailable);
            call.Visibility = state.SelectedGuildId.Length == 0 && state.SelectedChannelId.Length > 0 && state.View == "chat" ? Visibility.Visible : Visibility.Collapsed; call.IsEnabled = canAct && state.VoiceAvailable;
            videoCall.Visibility = call.Visibility; videoCall.IsEnabled = call.IsEnabled;
            pinned.Visibility = state.SelectedChannelId.Length > 0 && state.View is "chat" or "pins" ? Visibility.Visible : Visibility.Collapsed; pinned.IsEnabled = canAct && state.SelectedChannelId.Length > 0;
            pinned.Background = state.View == "pins" ? UiTheme.Selected : UiTheme.Surface; pinned.ToolTip = state.View == "pins" ? "Volver al chat" : "Mensajes fijados";
            addGroup.Visibility = state.SelectedChannelKind == "group" && state.View == "chat" ? Visibility.Visible : Visibility.Collapsed;
            membersToggle.Visibility = (state.SelectedChannelKind == "group" || state.SelectedGuildId.Length > 0 && state.ChannelAccess.CanViewMembers) && state.View == "chat" ? Visibility.Visible : Visibility.Collapsed;
            addGroup.IsEnabled = membersToggle.IsEnabled = canAct;
            footerVoiceControls.Visibility = state.VoiceConnected || state.VoiceConnecting ? Visibility.Visible : Visibility.Collapsed;
            voice.Content = state.VoiceConnecting ? "Conectando…" : state.VoiceConnected ? "◉ En llamada" : "◉ Voz";
            UiIcons.Set(mute, state.SelfMute ? UiIcons.MicOff : UiIcons.Mic); UiIcons.Set(deafen, state.SelfDeaf ? UiIcons.HeadphonesOff : UiIcons.Headphones); UiIcons.Set(activity, state.GameActivityEnabled ? UiIcons.Game : UiIcons.GameOff);
            activity.Background = state.GameActivityEnabled ? UiIcons.ActiveBackground : UiIcons.InactiveBackground;
            mute.ToolTip = state.SelfMute ? "Activar micrófono" : "Silenciar micrófono"; deafen.ToolTip = state.SelfDeaf ? "Activar audio" : "Ensordecer"; activity.ToolTip = state.GameActivityAvailable ? (state.GameActivityEnabled ? "Ocultar" : "Mostrar") + " actividad de juego en Discord" : "Visibilidad de actividad aún no verificada";
            mute.IsEnabled = canAct; deafen.IsEnabled = canAct; activity.IsEnabled = canAct && state.GameActivityAvailable;
            typing.Text = state.TypingText; typing.Visibility = state.TypingText.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            newMessageNotice.Text = state.NewMessageNotice; newMessageNotice.ToolTip = state.NewMessageNotice; newMessageNotice.Visibility = state.NewMessageNotice.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            ApplyCallState(canAct);
            ApplySocialState(changedChannel);
            ApplyHeaderLayout();
            older.Visibility = state.View == "chat" ? Visibility.Visible : Visibility.Collapsed; older.IsEnabled = canAct && state.View == "chat" && state.HasOlderMessages;
            present.Visibility = state.View == "chat" ? Visibility.Visible : Visibility.Collapsed; present.IsEnabled = canAct && state.ChannelAccess.CanView && state.ChannelAccess.CanReadHistory;
            if (changedChannel) search.Clear();
            AfterApplyComposer();
            if (enteringPins) ScrollPinnedStart(state.SelectedChannelId);
        }
        finally { applying = false; }
    }
    private void SyncNavigation(ListBox list, IReadOnlyList<NavItem> next, bool isDm, string selectedId)
    {
        var models = list.ItemsSource as UiNavModel[];
        bool same = models != null && models.Length == next.Count;
        bool canCall = state.SignedIn && !state.Demo && state.VoiceAvailable;
        if (same) for (int i = 0; i < next.Count; i++) if (models![i].Item != next[i] || models[i].IsDm != isDm || models[i].Compact != preferences.Compact || models[i].IsFriend != IsFriend(next[i].RecipientId) || models[i].CanCall != canCall) { same = false; break; }
        if (!same)
        {
            models = new UiNavModel[next.Count];
            for (int i = 0; i < next.Count; i++) models[i] = new(next[i], handleAction, isDm, IsFriend(next[i].RecipientId), preferences.Compact, canCall);
            list.ItemsSource = models;
        }
        if ((list.SelectedItem as UiNavModel)?.Item.Id != selectedId)
            list.SelectedItem = models!.FirstOrDefault(n => n.Item.Id == selectedId);
    }
    private bool IsFriend(string id) => id.Length > 0 && friendIds.Contains(id);
    private IReadOnlyList<NavItem> GroupDirectMessages(IReadOnlyList<NavItem> source)
    {
        if (groupedDmSource != null && (ReferenceEquals(groupedDmSource, source) || groupedDmSource.SequenceEqual(source))) return groupedDmItems!;
        var grouped = new List<NavItem>(source.Count + 3);
        if (source.Any(n => n.HasActiveCall)) { grouped.Add(new("ui:calls", "LLAMADAS", "section")); grouped.AddRange(source.Where(n => n.HasActiveCall)); }
        if (source.Any(n => n.IsPinned && !n.HasActiveCall)) { grouped.Add(new("ui:pinned", "FIJADOS", "section")); grouped.AddRange(source.Where(n => n.IsPinned && !n.HasActiveCall)); }
        grouped.Add(new("ui:recent", "MENSAJES DIRECTOS", "section")); grouped.AddRange(source.Where(n => !n.IsPinned && !n.HasActiveCall));
        groupedDmSource = source; groupedDmItems = grouped.ToArray(); return groupedDmItems;
    }
    private static bool CanGroup(ChatMessage previous, ChatMessage message)
        => previous.Author.Id == message.Author.Id && previous.ChannelId == message.ChannelId && previous.Timestamp.LocalDateTime.Date == message.Timestamp.LocalDateTime.Date
           && message.Timestamp >= previous.Timestamp && (message.Timestamp - previous.Timestamp).TotalMinutes < 5 && message.ReplyTo == null && !message.Deleted && !previous.Deleted && message.MessageType == 0 && previous.MessageType == 0;
    private UIElement ProfileContent(Person person, string subtitle)
    {
        return BuildProfileView(person, state.ProfileDetails, subtitle);
    }
    private static bool SameRelationships(IReadOnlyList<Relationship>? previous, IReadOnlyList<Relationship> next)
    {
        if (ReferenceEquals(previous, next)) return true;
        if (previous == null || previous.Count != next.Count) return false;
        for (int i = 0; i < next.Count; i++) if (previous[i] != next[i]) return false;
        return true;
    }
    private UIElement WelcomeContent()
    {
        var content = new StackPanel { VerticalAlignment = VerticalAlignment.Center, MaxWidth = 570 };
        content.Children.Add(Text("[ NOTDISCORD / TERMINAL ]", 23, Accent));
        content.Children.Add(new TextBlock { Text = state.Demo ? "Demostración local · ningún mensaje se envía" : state.SignedIn ? "Tu cuenta está conectada." : "Tu espacio de conversación, sin navegador en el chat.", FontSize = 17, Foreground = Ink, Margin = new Thickness(0, 20, 0, 14), TextWrapping = TextWrapping.Wrap });
        content.Children.Add(new TextBlock { Text = state.Demo ? "Estos datos son ficticios. Sirven para revisar el diseño, no para medir el consumo de una sesión real." : state.SignedIn ? "Selecciona una conversación o un canal. Puedes ver amigos y solicitudes desde la columna izquierda." : "Inicia sesión mediante el flujo normal. La cuenta y los mensajes no se incluyen en el instalador.", Foreground = Muted, TextWrapping = TextWrapping.Wrap, LineHeight = 23 });
        content.Children.Add(new TextBlock { Text = "CHAT · IMÁGENES · GIF · FUNCIONES INTEGRADAS\nVoz y pantalla se habilitan sólo cuando están disponibles.", Margin = new Thickness(0, 24, 0, 0), Foreground = Muted, TextWrapping = TextWrapping.Wrap, FontSize = 11, LineHeight = 21 });
        return content;
    }
    private void SendMessage()
    {
        SendPreparedMessage();
    }
    public void ClearComposer() => ClearCurrentDraft();
    public void ClearComposerIfMatches(string sentText, string replyMessageId)
    {
        if (composer.Text == sentText && replyId == replyMessageId) ClearComposer();
    }
    private void Handle(UiAction action)
    {
        if (string.IsNullOrEmpty(action.ChannelId)) action = action with { ChannelId = state.SelectedChannelId };
        if (action.Kind is "reply-local" or "edit-local" or "delete-local" or "react-local" or "forward-local" or "edit" or "delete" or "react" or "react-toggle" or "send" or "send-prepared" or "attach")
        {
            if (state.View != "chat" || !state.SignedIn || state.Demo || state.Busy || action.ChannelId != state.SelectedChannelId)
            { status.Text = "Abre el canal original para realizar esta acción."; return; }
        }
        if (action.Kind == "reply-local")
        {
            replyId = action.Id; reply.Text = "↳ " + UiPresenceViews.DisplayMentionText(action.Text, state.Messages.FirstOrDefault(m => m.Id == action.Id)?.Mentions); replyRow.Visibility = Visibility.Visible; composer.Focus(); return;
        }
        if (action.Kind == "edit-local")
        {
            if (state.Messages.FirstOrDefault(m => m.Id == action.Id)?.Forwarded == true) { status.Text = "Los mensajes reenviados no se pueden editar."; return; }
            var result = Prompt("Editar mensaje", "Contenido del mensaje", action.Text, true);
            if (result != null) Handle(action with { Kind = "edit", Text = result }); return;
        }
        if (action.Kind == "delete-local") { if (Confirm("Eliminar mensaje", "¿Eliminar este mensaje? Esta acción no se puede deshacer.")) Handle(action with { Kind = "delete" }); return; }
        if (action.Kind == "react-local") { if (!state.ChannelAccess.CanReact) { status.Text = "Este canal no permite añadir emojis de reacción nuevos."; return; } if (attachedController != null) ContentPicker.ShowEmoji(this, state, attachedController.GetPickerEmojisAsync, result => Handle(action with { Kind = "react-toggle", Text = result }), attachedDataDirectory); return; }
        if (action.Kind == "forward-local") { ShowForward(action); return; }
        if (action.Kind is "pin-message" or "unpin-message")
        { if (!state.SignedIn || state.Demo || state.Busy || !state.CanPinMessages || state.View is not ("chat" or "pins") || action.ChannelId != state.SelectedChannelId) { status.Text = "No puedes cambiar fijados desde esta vista."; return; } }
        if (action.Kind == "image-local") { ImageDialog(action.Text); return; }
        if (action.Kind == "save-media-local") { SaveMediaAsync(action.Text); return; }
        if (action.Kind == "copy-media-image-local") { CopyMediaImageAsync(action.Text); return; }
        if (action.Kind == "copy-media-link-local") { CopyMediaLink(action.Text); return; }
        if (action.Kind == "link-local") { OpenLink(action.Text); return; }
        if (action.Kind == "remove-friend" && !Confirm("Quitar amistad", "¿Quitar esta persona de tus amigos?")) return;
        if (action.Kind == "block-user" && !Confirm("Bloquear usuario", "¿Bloquear a esta persona?")) return;
        if (action.Kind == "guild-leave" && !Confirm("Salir del servidor", "¿Salir de este servidor? Para volver necesitarás otra invitación.")) return;
        if (action.Kind == "status-local") { status.Text = action.Text; return; }
        if (action.Kind == "channel") action = action with { Extra = GetChatRestoreAnchor(action.Id) };
        if (!state.Demo) ActionRequested?.Invoke(action);
    }
    private void Emit(string kind, string id = "", string text = "", string extra = "") => Handle(new(kind, id, text, extra));
    private static string PresenceLabel(string value) => value switch { "online" => "En línea", "idle" => "Ausente", "dnd" => "No molestar", "invisible" => "Invisible", _ => "Estado desconocido" };
    private ContextMenu BuildPresenceMenu()
    {
        var menu = UiMenus.Create();
        UiMenus.Add(menu, "Ver mi perfil", () => Emit("profile-self"), state.SignedIn && !state.Demo && !state.Busy);
        menu.Items.Add(new Separator());
        foreach (string value in new[] { "online", "idle", "dnd", "invisible" })
        {
            string next = value;
            UiMenus.Add(menu, (state.SelfPresence == value ? "✓  " : "    ") + PresenceLabel(value), () => Emit("status", text: next), state.SignedIn && !state.Demo && !state.Busy);
            var item = (MenuItem)menu.Items[^1]; item.IsCheckable = true; item.IsChecked = state.SelfPresence == value;
            item.ToolTip = value == "invisible" ? "Aparecer desconectado mientras sigues usando NotDiscord" : "Establecer estado: " + PresenceLabel(value);
        }
        return menu;
    }
    private void ShowPresenceMenu()
    {
        if (!state.SignedIn || state.Demo || state.Busy) return;
        var menu = BuildPresenceMenu(); menu.PlacementTarget = accountAvatar; menu.Placement = PlacementMode.Top; menu.IsOpen = true;
    }
    private void AttachFile()
    {
        ChooseComposerFiles();
    }
    private void GifDialog()
    {
        if (attachedController == null || !state.SignedIn || state.Demo) return;
        ContentPicker.ShowGif(this, state, attachedController.GetPickerGifCatalogAsync, value => { if (composer.SelectionStart > 0 && !char.IsWhiteSpace(composer.Text[composer.SelectionStart - 1])) InsertComposer(" "); InsertComposer(value + " "); }, attachedDataDirectory);
    }
    private void EmojiDialog()
    {
        if (attachedController == null || !state.SignedIn || state.Demo) return;
        ContentPicker.ShowEmoji(this, state, attachedController.GetPickerEmojisAsync, InsertComposer, attachedDataDirectory);
    }
    private void InsertComposer(string text) { composer.SelectedText = text; composer.CaretIndex = composer.SelectionStart + composer.SelectionLength; composer.Focus(); }
    private void InviteDialog()
    {
        var value = Prompt("Unirse mediante invitación", "Pega un enlace discord.gg o un código. Primero se consultará la invitación; no se aceptará automáticamente.");
        if (!string.IsNullOrWhiteSpace(value)) Emit("invite-preview", text: value.Trim());
    }
    public bool ConfirmInvitation(string description) => Confirm("Aceptar invitación", description + "\n\n¿Unirte a este servidor?");
    private void PreferencesDialog()
    {
        var dialog = Dialog("Ajustes de NotDiscord", 620, 800);
        var stack = new StackPanel { Margin = new Thickness(24) };
        stack.Children.Add(Text("Apariencia y accesibilidad", 21, Ink));
        var mediaPreferences = Button("Voz, vídeo y dispositivos", () => { dialog.Close(); Emit("media-settings"); }); mediaPreferences.Margin = new Thickness(0, 12, 0, 8); mediaPreferences.IsEnabled = state.SignedIn && !state.Demo; stack.Children.Add(mediaPreferences);
        stack.Children.Add(new TextBlock { Text = "Los colores se previsualizan en toda la aplicación. El texto, los paneles y los controles ajustan su contraste automáticamente. Cancelar recupera el tema guardado.", TextWrapping = TextWrapping.Wrap, Foreground = Muted, Margin = new Thickness(0, 10, 0, 5) });
        stack.Children.Add(Label("Escala de la interfaz · Ctrl + / Ctrl − / Ctrl 0"));
        var zoom = new Slider { Minimum = 80, Maximum = 150, Value = preferences.Zoom * 100, TickFrequency = 5, IsSnapToTickEnabled = true };
        var zoomValue = Text($"{zoom.Value:0} %", 14, Muted); zoom.ValueChanged += (_, _) => zoomValue.Text = $"{zoom.Value:0} %"; stack.Children.Add(zoom); stack.Children.Add(zoomValue);
        stack.Children.Add(Label("Tamaño del texto de los mensajes"));
        var chatSize = new Slider { Minimum = 14, Maximum = 22, Value = preferences.ChatSize, TickFrequency = 1, IsSnapToTickEnabled = true };
        var sizeValue = Text($"{chatSize.Value:0} px", 14, Muted); chatSize.ValueChanged += (_, _) => sizeValue.Text = $"{chatSize.Value:0} px"; stack.Children.Add(chatSize); stack.Children.Add(sizeValue);
        var compact = new CheckBox { Content = "Diseño compacto", IsChecked = preferences.Compact, Foreground = Ink, FontSize = 15, Margin = new Thickness(0, 15, 0, 0) }; stack.Children.Add(compact);
        var notifications = new CheckBox { Content = "Notificaciones de mensajes y llamadas en Windows", IsChecked = preferences.WindowsNotifications, Foreground = Ink, FontSize = 15, Margin = new Thickness(0, 12, 0, 4) }; stack.Children.Add(notifications);
        stack.Children.Add(new TextBlock { Text = "Se respetan No molestar y las conversaciones silenciadas. Windows controla si muestra los banners; pulsar uno abre el chat sin responder una llamada automáticamente.", TextWrapping = TextWrapping.Wrap, Foreground = Muted, FontSize = 12 });
        stack.Children.Add(Label("Color base de toda la interfaz · #RRGGBB")); var background = new TextBox { Text = preferences.Background }; stack.Children.Add(background);
        stack.Children.Add(Label("Color de las líneas separadoras · #RRGGBB")); var separators = new TextBox { Text = preferences.Separator }; stack.Children.Add(separators);
        stack.Children.Add(Label("Color de acento · #RRGGBB")); var accentColor = new TextBox { Text = preferences.AccentColor }; stack.Children.Add(accentColor);
        var presets = new WrapPanel { Margin = new Thickness(0, 12, 0, 8) };
        foreach (var preset in new[] { ("Discord", "#313338", "#45474D", "#5865F2"), ("Noche azul", "#13263A", "#36516A", "#54BDFF"), ("Bosque", "#142B25", "#386652", "#60D49A"), ("Claro", "#F1F3F6", "#ABB4C1", "#4055CB") })
        { var chosen = preset; var button = Button(chosen.Item1, () => { background.Text = chosen.Item2; separators.Text = chosen.Item3; accentColor.Text = chosen.Item4; }); button.Margin = new Thickness(0, 0, 7, 4); presets.Children.Add(button); } stack.Children.Add(presets);
        var themeSwatches = new UniformGrid { Columns = 4, Height = 62 };
        foreach (var swatch in new[] { ("Fondo", UiTheme.Background), ("Panel", UiTheme.Panel), ("Controles", UiTheme.Surface), ("Selección", UiTheme.Selected) }) themeSwatches.Children.Add(new Border { Background = swatch.Item2, BorderBrush = line, BorderThickness = new Thickness(1), Padding = new Thickness(9), Child = Text(swatch.Item1, 13, Ink) });
        stack.Children.Add(themeSwatches);
        var previewHelp = new TextBlock { Foreground = Muted, TextWrapping = TextWrapping.Wrap, FontSize = 12, Margin = new Thickness(0, 7, 0, 0) }; stack.Children.Add(previewHelp);
        bool ValidColor(string value) => Regex.IsMatch(value, "^#[0-9a-fA-F]{6}$");
        void PreviewTheme()
        {
            if (ValidColor(background.Text) && ValidColor(separators.Text) && ValidColor(accentColor.Text)) { UiTheme.Apply(background.Text, separators.Text, accentColor.Text); previewHelp.Text = "Vista previa activa · contraste de texto mínimo 4,5:1 en todas las superficies."; }
            else previewHelp.Text = "Completa los colores con # y seis dígitos. Se mantiene la última vista previa válida.";
        }
        background.TextChanged += (_, _) => PreviewTheme(); separators.TextChanged += (_, _) => PreviewTheme(); accentColor.TextChanged += (_, _) => PreviewTheme(); PreviewTheme();
        stack.Children.Add(Label("Transparencia del fondo"));
        stack.Children.Add(new TextBlock { Text = "Pendiente en este prototipo. Se mantiene el fondo sólido y el texto completamente opaco.", TextWrapping = TextWrapping.Wrap, Foreground = Muted, FontSize = 12, Margin = new Thickness(0, 0, 0, 16) });
        var gifs = new CheckBox { Content = "Permitir GIFs al pulsar Reproducir", IsChecked = preferences.AllowGifPlayback, Foreground = Ink, FontSize = 15, Margin = new Thickness(0, 2, 0, 6) }; stack.Children.Add(gifs);
        stack.Children.Add(new TextBlock { Text = "Sin reproducción automática. Máximo un GIF activo; se detiene al salir de la pantalla. Límite de 8 MiB de fotogramas y 12 MiB de descarga.", TextWrapping = TextWrapping.Wrap, Foreground = Muted, FontSize = 13 });
        stack.Children.Add(Label("Actividad"));
        var configureGame = Button(state.GameActivityEnabled ? "Ocultar actividad de juego en Discord" : "Mostrar actividad de juego en Discord", () => Emit("toggle-game-activity")); configureGame.IsEnabled = state.SignedIn && !state.Demo && state.GameActivityAvailable; stack.Children.Add(configureGame);
        var customActivity = Button("Publicar actividad personalizada…", () => { var name = Prompt("Actividad personalizada", "Escribe el nombre que quieres publicar como «Jugando a…». Esta opción es manual y no detecta programas."); if (!string.IsNullOrWhiteSpace(name)) Emit("custom-activity", text: name.Trim()); }); customActivity.IsEnabled = state.SignedIn && !state.Demo; customActivity.Margin = new Thickness(0, 5, 0, 0); stack.Children.Add(customActivity);
        var clearActivity = Button("Quitar actividad personalizada", () => Emit("custom-activity")); clearActivity.IsEnabled = state.SignedIn && !state.Demo; clearActivity.Margin = new Thickness(0, 5, 0, 0); stack.Children.Add(clearActivity);
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right };
        var reset = Button("Restablecer", () => { background.Text = "#313338"; separators.Text = "#25262B"; accentColor.Text = "#5865F2"; zoom.Value = 100; chatSize.Value = 16; compact.IsChecked = false; gifs.IsChecked = true; }); reset.Margin = new Thickness(0, 0, 8, 0); buttons.Children.Add(reset);
        var cancel = Button("Cancelar", () => dialog.Close()); cancel.Margin = new Thickness(0, 0, 8, 0); buttons.Children.Add(cancel);
        buttons.Children.Add(Button("Guardar", () =>
        {
            if (!ValidColor(background.Text) || !ValidColor(separators.Text) || !ValidColor(accentColor.Text)) { MessageBox.Show(dialog, "Usa colores como #10151C.", "Color inválido"); return; }
            var previous = preferences;
            preferences = new() { Background = background.Text, Separator = separators.Text, AccentColor = accentColor.Text, Opacity = 1, Zoom = zoom.Value / 100, ChatSize = chatSize.Value, Compact = compact.IsChecked == true, AllowGifPlayback = gifs.IsChecked == true, WindowsNotifications = notifications.IsChecked == true };
            try { SavePreferences(); styleVersion++; UpdateTheme(); ApplyZoom(); Apply(state); dialog.Close(); }
            catch (Exception ex) { preferences = previous; MessageBox.Show(dialog, "No se guardó la configuración: " + ex.Message, "Error"); }
        }));
        if (state.SignedIn) { var logout = Button("Cerrar sesión", () => { if (Confirm("Cerrar sesión", "¿Cerrar la sesión de este cliente?")) { dialog.Close(); Emit("logout"); } }); logout.Margin = new Thickness(0, 22, 0, 0); stack.Children.Add(logout); }
        var settingsLayout = new DockPanel { Background = bg };
        var settingsFooter = new Border { Background = panel, BorderBrush = line, BorderThickness = new Thickness(0, 1, 0, 0), Padding = new Thickness(18, 12, 18, 12), Child = buttons }; DockPanel.SetDock(settingsFooter, Dock.Bottom); settingsLayout.Children.Add(settingsFooter);
        settingsLayout.Children.Add(new ScrollViewer { Content = stack, Background = bg, VerticalScrollBarVisibility = ScrollBarVisibility.Auto });
        dialog.Closed += (_, _) => UpdateTheme(); dialog.Content = settingsLayout; dialog.ShowDialog();
    }
    private void SavePreferences() { Directory.CreateDirectory(Path.GetDirectoryName(preferencePath)!); File.WriteAllText(preferencePath, JsonSerializer.Serialize(preferences)); }
    private void ApplyZoom()
    {
        preferences.Zoom = Math.Clamp(preferences.Zoom, .8, 1.5); preferences.ChatSize = Math.Clamp(preferences.ChatSize, 14, 22);
        shell.LayoutTransform = new ScaleTransform(preferences.Zoom, preferences.Zoom);
        UiGifImage.ConfigurePlayback(preferences.AllowGifPlayback);
    }
    private void ZoomKey(object sender, KeyEventArgs e)
    {
        if ((Keyboard.Modifiers & ModifierKeys.Control) == 0) return;
        double next = e.Key is Key.Add or Key.OemPlus ? preferences.Zoom + .1 : e.Key is Key.Subtract or Key.OemMinus ? preferences.Zoom - .1 : e.Key is Key.D0 or Key.NumPad0 ? 1 : preferences.Zoom;
        next = Math.Round(Math.Clamp(next, .8, 1.5), 2);
        if (next == preferences.Zoom) return;
        preferences.Zoom = next; ApplyZoom();
        try { SavePreferences(); status.Text = $"Escala {next:P0} · Ctrl 0 restablece"; } catch { status.Text = "Escala aplicada; no se pudo guardar la preferencia."; }
        e.Handled = true;
    }
    private void UpdateTheme()
    {
        UiTheme.Apply(preferences.Background, preferences.Separator, preferences.AccentColor);
        Background = bg; shell.Background = bg; Opacity = 1;
        Resources["UiBackground"] = bg; Resources["UiLine"] = line;
        foreach (var b in shell.Children.OfType<Border>())
        {
            b.Background = Grid.GetRow(b) == 1 && Grid.GetColumn(b) == 0 ? UiTheme.Rail : Grid.GetColumn(b) < 2 ? panel : bg;
            b.BorderBrush = line;
        }
    }
    private void ImageDialog(string url) => ShowMediaViewerAsync(url);
    internal static void OpenLink(string value)
    {
        if (!Uri.TryCreate(value, UriKind.Absolute, out var u) || u.Scheme is not ("https" or "http") || !string.IsNullOrEmpty(u.UserInfo)) return;
        try { Process.Start(new ProcessStartInfo(u.AbsoluteUri) { UseShellExecute = true }); } catch { }
    }
    private Window Dialog(string title, double width, double height) => new() { Owner = IsLoaded ? this : null, Title = title, Width = width, Height = height, MinWidth = 360, WindowStartupLocation = WindowStartupLocation.CenterOwner, Background = bg, Foreground = Ink, FontFamily = FontFamily, FontSize = 13, Resources = Resources };
    private string? Prompt(string title, string description, string initial = "", bool multiline = false)
    {
        var dialog = Dialog(title, 540, multiline ? 360 : 300); dialog.ResizeMode = ResizeMode.NoResize;
        var layout = new StackPanel { Margin = new Thickness(22) };
        layout.Children.Add(new TextBlock { Text = description, TextWrapping = TextWrapping.Wrap, Foreground = Muted, Margin = new Thickness(0, 0, 0, 14), LineHeight = 20 });
        var input = new TextBox { Text = initial, AcceptsReturn = multiline, TextWrapping = TextWrapping.Wrap, MinHeight = multiline ? 100 : 36, MaxHeight = 150, VerticalScrollBarVisibility = ScrollBarVisibility.Auto }; layout.Children.Add(input);
        string? result = null; var buttons = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 18, 0, 0) };
        var cancel = Button("Cancelar", () => dialog.Close()); cancel.Margin = new Thickness(0, 0, 8, 0); buttons.Children.Add(cancel); buttons.Children.Add(Button("Continuar", () => { result = input.Text; dialog.Close(); })); layout.Children.Add(buttons); dialog.Content = layout; dialog.Loaded += (_, _) => { input.Focus(); input.SelectAll(); }; dialog.ShowDialog(); return result;
    }
    private bool Confirm(string title, string message) => CreateConfirmationDialog(title, message).ShowDialog() == true;
    internal static TextBlock Text(string text, double size = 15, Brush? color = null) => new() { Text = text, FontSize = size, Foreground = color ?? Ink, VerticalAlignment = VerticalAlignment.Center };
    private static TextBlock Label(string text) => new() { Text = text, Foreground = Muted, Margin = new Thickness(0, 14, 0, 6), FontSize = 14 };
    internal static Button Button(string label, Action click) { var b = new Button { Content = label }; b.Click += (_, _) => click(); return b; }
    private Border Frame(UIElement child) => new() { Child = child, BorderThickness = new Thickness(0, 0, 1, 1), BorderBrush = line, Background = bg };
    private void Place(UIElement element, int row, int col, int columns = 1) { Grid.SetRow(element, row); Grid.SetColumn(element, col); Grid.SetColumnSpan(element, columns); shell.Children.Add(element); }
    internal static void Virtual(ListBox list)
    {
        list.Background = Brushes.Transparent; list.BorderThickness = new Thickness(0); list.HorizontalContentAlignment = HorizontalAlignment.Stretch;
        VirtualizingPanel.SetIsVirtualizing(list, true); VirtualizingPanel.SetVirtualizationMode(list, VirtualizationMode.Recycling); VirtualizingPanel.SetScrollUnit(list, ScrollUnit.Pixel); VirtualizingPanel.SetCacheLength(list, new VirtualizationCacheLength(.5));
        ScrollViewer.SetCanContentScroll(list, true); ScrollViewer.SetHorizontalScrollBarVisibility(list, ScrollBarVisibility.Disabled); ScrollViewer.SetVerticalScrollBarVisibility(list, ScrollBarVisibility.Auto);
    }
    private static DataTemplate CellTemplate<T>() where T : FrameworkElement => new() { VisualTree = new FrameworkElementFactory(typeof(T)) };
    internal static UIElement Avatar(Person? person, int size)
    {
        var text = Text(person?.Name.Length > 0 ? System.Globalization.StringInfo.GetNextTextElement(person.Name).ToUpperInvariant() : "N", Math.Max(13, size * .46), UiTheme.OnAccent); text.TextAlignment = TextAlignment.Center; text.HorizontalAlignment = HorizontalAlignment.Center;
        var holder = new Grid { Width = size, Height = size }; holder.Children.Add(text);
        if (MediaImages.IsAllowed(person?.AvatarUrl)) holder.Children.Add(new UiMediaImage(person!.AvatarUrl, size * 2) { Width = size, Height = size });
        return holder;
    }
    internal static Border RoundAvatar(Person? person, int size) => new() { Width = size, Height = size, Background = UiTheme.AccentSurface, Clip = new EllipseGeometry(new Point(size / 2d, size / 2d), size / 2d, size / 2d), Child = Avatar(person, size) };
    internal static Border NavAvatar(NavItem item, int size)
    {
        if (MediaImages.IsAllowed(item.IconUrl)) return RoundAvatar(new Person(item.RecipientId, item.Name, item.IconUrl), size);
        var photos = item.RecipientAvatars?.Where(url => MediaImages.IsAllowed(url)).Distinct().Take(4).ToArray() ?? Array.Empty<string>();
        if (photos.Length == 0) return RoundAvatar(new Person(item.RecipientId, item.Name), size);
        var grid = new UniformGrid { Columns = photos.Length == 1 ? 1 : 2, Rows = photos.Length <= 2 ? 1 : 2 };
        foreach (string url in photos) grid.Children.Add(new UiMediaImage(url, size) { Stretch = Stretch.UniformToFill });
        return new Border { Width = size, Height = size, Clip = new EllipseGeometry(new Point(size / 2d, size / 2d), size / 2d, size / 2d), Child = grid, Background = UiTheme.Surface };
    }
    private static bool IsNearBottom(DependencyObject element)
    {
        var viewer = FindVisual<ScrollViewer>(element); return viewer == null || viewer.ScrollableHeight - viewer.VerticalOffset < 120;
    }
    private static T? FindVisual<T>(DependencyObject root) where T : DependencyObject
    {
        for (int i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++) { var child = VisualTreeHelper.GetChild(root, i); if (child is T value) return value; var nested = FindVisual<T>(child); if (nested != null) return nested; } return null;
    }
    public void SavePreview(string path)
    {
        UpdateLayout(); var root = (FrameworkElement)Content; int width = Math.Max(1, (int)Math.Ceiling(root.ActualWidth)), height = Math.Max(1, (int)Math.Ceiling(root.ActualHeight));
        var visual = new DrawingVisual(); using (var drawing = visual.RenderOpen()) drawing.DrawRectangle(new VisualBrush(root), null, new Rect(0, 0, width, height));
        var bitmap = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32); bitmap.Render(visual); var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using var stream = File.Create(path); encoder.Save(stream);
    }
}

internal sealed class UiPreferences { public string Background { get; set; } = "#313338"; public string Separator { get; set; } = "#25262B"; public string AccentColor { get; set; } = "#5865F2"; public double Opacity { get; set; } = 1; public double Zoom { get; set; } = 1; public double ChatSize { get; set; } = 16; public bool Compact { get; set; } public bool AllowGifPlayback { get; set; } = true; public bool WindowsNotifications { get; set; } = true; }

internal static class UiTheme
{
    // Bound brushes stay live even when WPF seals a template; changing theme never rebuilds the chat.
    private sealed class Tone : System.ComponentModel.INotifyPropertyChanged
    {
        private Color color;
        public Color Color { get => color; set { if (color != value) { color = value; PropertyChanged?.Invoke(this, new(nameof(Color))); } } }
        public event System.ComponentModel.PropertyChangedEventHandler? PropertyChanged;
    }
    private static SolidColorBrush Create(string hex)
    {
        var brush = new SolidColorBrush(); BindingOperations.SetBinding(brush, SolidColorBrush.ColorProperty, new Binding(nameof(Tone.Color)) { Source = new Tone { Color = Parse(hex, Colors.Black) } }); return brush;
    }
    public static readonly SolidColorBrush Background = Create("#313338"), Panel = Create("#2B2D31"), Rail = Create("#1E1F22"), Surface = Create("#383A40"), Hover = Create("#42454A"), Selected = Create("#404249"), Menu = Create("#232428"), Input = Create("#232428"), Line = Create("#45474D");
    public static readonly SolidColorBrush Ink = Create("#F2F3F5"), Muted = Create("#B5BAC1"), Accent = Create("#AAB1FF"), AccentSurface = Create("#5865F2"), Danger = Create("#FF8085");
    public static readonly SolidColorBrush OnAccent = TerminalWindow.Brush("#FFFFFF");
    private static void Set(SolidColorBrush brush, Color color) => ((Tone)BindingOperations.GetBinding(brush, SolidColorBrush.ColorProperty)!.Source).Color = color;
    private static Color Parse(string hex, Color fallback) { try { return (Color)ColorConverter.ConvertFromString(hex); } catch { return fallback; } }
    internal static Color Mix(Color a, Color b, double t) => Color.FromRgb((byte)Math.Round(a.R + (b.R - a.R) * t), (byte)Math.Round(a.G + (b.G - a.G) * t), (byte)Math.Round(a.B + (b.B - a.B) * t));
    private static double Luminance(Color c) { double Linear(byte v) { double x = v / 255d; return x <= .04045 ? x / 12.92 : Math.Pow((x + .055) / 1.055, 2.4); } return .2126 * Linear(c.R) + .7152 * Linear(c.G) + .0722 * Linear(c.B); }
    internal static double Contrast(Color a, Color b) { double x = Luminance(a), y = Luminance(b); return (Math.Max(x, y) + .05) / (Math.Min(x, y) + .05); }
    private static Color Readable(Color candidate, IReadOnlyList<Color> surfaces, double ratio, Color toward)
    {
        for (int i = 0; i <= 100; i++) { var test = Mix(candidate, toward, i / 100d); if (surfaces.All(s => Contrast(test, s) >= ratio)) return test; } return toward;
    }
    public static void Apply(string background, string separator, string accent)
    {
        var basis = Parse(background, Color.FromRgb(49, 51, 56)); bool light = Luminance(basis) > .35;
        // Extreme custom colors are supported; surfaces share their hue and a consistent light/dark scale.
        Color near = light ? Colors.White : Colors.Black, far = light ? Colors.Black : Colors.White;
        var values = new[] { basis, Mix(basis, Colors.Black, light ? .04 : .10), Mix(basis, Colors.Black, light ? .09 : .34), Mix(basis, far, light ? .055 : .055), Mix(basis, far, light ? .11 : .11), Mix(basis, far, light ? .15 : .15), Mix(basis, near, .20), Mix(basis, near, .10) };
        var brushes = new[] { Background, Panel, Rail, Surface, Hover, Selected, Menu, Input };
        // Keep the full surface range on one side of accessible text contrast.
        for (int i = 0; i < values.Length; i++) if (Contrast(far, values[i]) < 5.2) values[i] = Readable(values[i], new[] { far }, 5.2, near);
        for (int i = 0; i < brushes.Length; i++) Set(brushes[i], values[i]);
        Set(Ink, Readable(light ? Color.FromRgb(27, 29, 34) : Color.FromRgb(242, 243, 245), values, 4.5, far));
        Set(Muted, Readable(light ? Color.FromRgb(80, 83, 91) : Color.FromRgb(172, 177, 185), values, 4.5, far));
        var chosenAccent = Parse(accent, Color.FromRgb(88, 101, 242)); Set(Accent, Readable(chosenAccent, values, 4.5, far));
        Set(AccentSurface, Readable(chosenAccent, new[] { Colors.White }, 4.5, Colors.Black));
        Set(Line, Readable(Parse(separator, values[1]), new[] { values[0], values[1] }, 1.35, far));
        Set(Danger, Readable(Color.FromRgb(242, 63, 67), values, 4.5, far));
    }
}
public sealed record UiNavModel(NavItem Item, Action<UiAction> Action, bool IsDm, bool IsFriend = false, bool Compact = false, bool CanCall = false);
public sealed record UiMessageModel(ChatMessage Message, Action<UiAction> Action, bool Own, bool Demo, bool SignedIn, bool Continuation = false, double ChatSize = 16, bool Compact = false, string ChannelLabel = "", bool DayStart = false, bool CanPin = false, bool PinsView = false, ChannelAccess? Access = null);
public sealed record UiPersonModel(Relationship Relationship, Action<UiAction> Action, bool Demo, UserPresence? Presence = null);
internal sealed record UiEmojiChoice(CustomEmoji Emoji) { public override string ToString() => (Emoji.Animated ? "[GIF] " : ": ") + Emoji.Name; }

public sealed class UiServerCell : Border
{
    public UiServerCell() { Margin = new Thickness(4, 5, 4, 5); Width = 48; Height = 48; Background = Brushes.Transparent; DataContextChanged += (_, _) => Render(); }
    private void Render()
    {
        if (DataContext is not UiNavModel model) { Child = null; return; }
        ToolTip = model.Item.Name + (model.Item.Unread ? " · mensajes sin leer" : "") + (model.Item.MentionCount > 0 ? $" · {model.Item.MentionCount} menciones" : "");
        BorderBrush = model.Item.Unread ? TerminalWindow.Accent : Brushes.Transparent; BorderThickness = model.Item.Unread ? new Thickness(3) : new Thickness(0);
        var grid = new Grid();
        grid.Children.Add(TerminalWindow.NavAvatar(model.Item, 48));
        if (model.Item.MentionCount > 0)
        {
            var count = TerminalWindow.Text(model.Item.MentionCount > 99 ? "99+" : model.Item.MentionCount.ToString(), 11, UiTheme.OnAccent); count.HorizontalAlignment = HorizontalAlignment.Center;
            grid.Children.Add(new Border { Background = TerminalWindow.Brush("#DA373C"), BorderBrush = UiTheme.Rail, BorderThickness = new Thickness(2), CornerRadius = new CornerRadius(10), MinWidth = 20, Height = 20, Padding = new Thickness(3, 0, 3, 0), HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Bottom, Child = count });
        }
        Child = grid;
        var menu = UiMenus.Create();
        UiMenus.Add(menu, "Abrir servidor", () => model.Action(new("guild", model.Item.Id)));
        UiMenus.Add(menu, "Marcar como leído", () => model.Action(new("guild-mark-read", model.Item.Id)));
        UiMenus.Add(menu, model.Item.IsMuted ? "Reactivar notificaciones" : "Silenciar servidor", () => model.Action(new("guild-mute", model.Item.Id)));
        if (model.Item.CanInvite) UiMenus.Add(menu, "Invitar a personas", () => model.Action(new("guild-invite", model.Item.Id)));
        menu.Items.Add(new Separator());
        UiMenus.Add(menu, "Copiar ID del servidor", () => model.Action(new("copy-id", model.Item.Id)));
        UiMenus.Add(menu, "Salir del servidor", () => model.Action(new("guild-leave", model.Item.Id)), danger: true);
        ContextMenu = menu;
    }
}

public sealed class UiChannelCell : Border
{
    public UiChannelCell() { DataContextChanged += (_, _) => Render(); Loaded += (_, _) => UpdateContainer(); }
    private void UpdateContainer()
    {
        DependencyObject? parent = this;
        while (parent != null && parent is not ListBoxItem) parent = VisualTreeHelper.GetParent(parent);
        if (parent is ListBoxItem container) container.IsEnabled = DataContext is UiNavModel model && model.Item.Kind is not ("section" or "category" or "4");
    }
    private void Render()
    {
        if (DataContext is not UiNavModel model) { Child = null; return; }
        ContextMenu = null;
        bool section = model.Item.Kind is "section" or "category" or "4";
        UpdateContainer();
        Padding = section ? new Thickness(9, 15, 7, 5) : model.IsDm ? new Thickness(8, 6, 8, 6) : new Thickness(10, 8, 8, 8);
        MinHeight = section ? 30 : model.IsDm ? model.Compact ? 46 : 56 : 38;
        IsHitTestVisible = !section;
        if (section) { Child = TerminalWindow.Text((model.Item.Kind == "section" ? "" : "▾ ") + model.Item.Name, 12, TerminalWindow.Muted); return; }
        var row = new DockPanel();
        if (model.IsDm)
        {
            int avatarSize = model.Compact ? 34 : 40;
            var avatarHolder = new Grid { Width = avatarSize, Height = avatarSize, Margin = new Thickness(0, 0, 11, 0) };
            avatarHolder.Children.Add(TerminalWindow.NavAvatar(model.Item, avatarSize));
            if (model.Item.Presence.Length > 0)
            {
                string color = model.Item.Presence is "online" ? "#23A55A" : model.Item.Presence is "idle" ? "#F0B232" : model.Item.Presence is "dnd" ? "#F23F43" : "#80848E";
                avatarHolder.Children.Add(new Border { Width = 12, Height = 12, CornerRadius = new CornerRadius(6), Background = TerminalWindow.Brush(color), BorderBrush = UiTheme.Panel, BorderThickness = new Thickness(2), HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Bottom });
            }
            DockPanel.SetDock(avatarHolder, Dock.Left); row.Children.Add(avatarHolder);
            AddUnread(row, model.Item);
            if (model.Item.IsPinned) { var pin = new Image { Source = UiIcons.Pin, Width = 15, Height = 15, Margin = new Thickness(5, 0, 0, 0), ToolTip = "Conversación fijada" }; DockPanel.SetDock(pin, Dock.Right); row.Children.Add(pin); }
            var labels = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
            var name = TerminalWindow.Text(model.Item.Name, 16, model.Item.Unread ? TerminalWindow.Ink : TerminalWindow.Ink); name.TextTrimming = TextTrimming.CharacterEllipsis; if (model.Item.Unread) name.FontWeight = FontWeights.SemiBold; labels.Children.Add(name);
            if (model.Item.HasActiveCall) labels.Children.Add(TerminalWindow.Text(model.Item.IncomingCallRinging ? "☎ Llamada entrante" : "☎ Llamada en curso", 12, UiTheme.Accent));
            labels.Children.Add(UiPresenceViews.StatusLine(model.Item.CustomStatus, model.Item.CustomStatusEmoji, 12));
            if (model.Item.ActivityText.Length > 0) labels.Children.Add(UiPresenceViews.CompactActivity(model.Item.ActivityText, 12, TerminalWindow.Muted));
            else if (model.Item.CustomStatus.Length == 0 && model.Item.CustomStatusEmoji == null && model.Item.Subtitle.Length > 0) labels.Children.Add(UiPresenceViews.CompactActivity(model.Item.Subtitle, 12, TerminalWindow.Muted));
            row.Children.Add(labels);
            var menu = UiMenus.Create();
            UiMenus.Add(menu, "Abrir conversación", () => model.Action(new("channel", model.Item.Id)));
            UiMenus.Add(menu, "Ver perfil", () => model.Action(new("dm-profile", model.Item.Id)));
            UiMenus.Add(menu, "Llamar", () => model.Action(new("call-dm", model.Item.Id)), enabled: model.CanCall);
            menu.Items.Add(new Separator());
            UiMenus.Add(menu, model.Item.IsPinned ? "Desfijar conversación" : "Fijar conversación", () => model.Action(new("pin-dm", model.Item.Id)));
            UiMenus.Add(menu, model.Item.IsMuted ? "Reactivar notificaciones" : "Silenciar conversación", () => model.Action(new("mute-channel", model.Item.Id)));
            UiMenus.Add(menu, "Marcar como leído", () => model.Action(new("mark-read", model.Item.Id)));
            UiMenus.Add(menu, "Cerrar conversación", () => model.Action(new("close-dm", model.Item.Id)));
            UiMenus.Add(menu, "Copiar ID", () => model.Action(new("copy-id", model.Item.Id)));
            if (model.Item.RecipientId.Length > 0)
            {
                menu.Items.Add(new Separator());
                UiMenus.Add(menu, "Bloquear", () => model.Action(new("block-user", model.Item.RecipientId)), danger: true);
                if (model.IsFriend) UiMenus.Add(menu, "Quitar amistad", () => model.Action(new("remove-friend", model.Item.RecipientId)), danger: true);
            }
            ContextMenu = menu;
        }
        else
        {
            var symbol = TerminalWindow.Text(model.Item.Kind is "voice" or "2" ? "🔊" : "#", 21, TerminalWindow.Muted); symbol.Width = 30; DockPanel.SetDock(symbol, Dock.Left); row.Children.Add(symbol);
            AddUnread(row, model.Item);
            var name = TerminalWindow.Text(model.Item.Name, 16, model.Item.Unread ? TerminalWindow.Ink : TerminalWindow.Muted); name.TextTrimming = TextTrimming.CharacterEllipsis; row.Children.Add(name);
            var menu = UiMenus.Create(); UiMenus.Add(menu, "Abrir canal", () => model.Action(new("channel", model.Item.Id))); UiMenus.Add(menu, "Marcar como leído", () => model.Action(new("mark-read", model.Item.Id))); UiMenus.Add(menu, model.Item.IsMuted ? "Reactivar notificaciones" : "Silenciar canal", () => model.Action(new("mute-channel", model.Item.Id))); UiMenus.Add(menu, "Copiar ID", () => model.Action(new("copy-id", model.Item.Id))); ContextMenu = menu;
        }
        Child = row; ToolTip = model.Item.Name + (model.Item.IsMuted ? " · silenciado" : "");
    }
    private static void AddUnread(DockPanel row, NavItem item)
    {
        if (!item.Unread && item.MentionCount <= 0) return;
        var badge = new Border { Background = item.MentionCount > 0 ? UiTheme.AccentSurface : TerminalWindow.Ink, CornerRadius = new CornerRadius(9), VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(6, 0, 0, 0), ToolTip = item.MentionCount > 0 ? $"{item.MentionCount} menciones" : "Mensajes sin leer" };
        if (item.MentionCount > 0) { badge.MinWidth = 19; badge.Padding = new Thickness(5, 1, 5, 1); var count = TerminalWindow.Text(item.MentionCount > 99 ? "99+" : item.MentionCount.ToString(), 11, UiTheme.OnAccent); count.TextAlignment = TextAlignment.Center; badge.Child = count; }
        else { badge.Width = 7; badge.Height = 7; }
        DockPanel.SetDock(badge, Dock.Right); row.Children.Add(badge);
    }
}

internal static class UiMenus
{
    private static readonly ControlTemplate ItemTemplate = BuildItemTemplate();
    private static readonly ControlTemplate MenuTemplate = BuildMenuTemplate();
    public static ContextMenu Create()
    {
        var menu = new ContextMenu { Template = MenuTemplate, Background = UiTheme.Menu, Foreground = TerminalWindow.Ink, BorderBrush = UiTheme.Line, BorderThickness = new Thickness(1), FontFamily = new FontFamily("Segoe UI"), FontSize = 14, Padding = new Thickness(6), MinWidth = 225 };
        var separator = new Style(typeof(Separator)); var template = new ControlTemplate(typeof(Separator)); var line = new FrameworkElementFactory(typeof(Border)); line.SetValue(FrameworkElement.HeightProperty, 1d); line.SetValue(FrameworkElement.MarginProperty, new Thickness(8, 5, 8, 5)); line.SetValue(Border.BackgroundProperty, UiTheme.Line); template.VisualTree = line; separator.Setters.Add(new Setter(Control.TemplateProperty, template)); menu.Resources[typeof(Separator)] = separator; return menu;
    }
    public static void Add(ContextMenu menu, string label, Action action, bool enabled = true, bool danger = false)
    {
        var item = new MenuItem { Header = label, IsEnabled = enabled, Template = ItemTemplate, Foreground = danger ? UiTheme.Danger : TerminalWindow.Ink };
        item.Click += (_, _) => action(); menu.Items.Add(item);
    }
    private static ControlTemplate BuildItemTemplate()
    {
        var template = new ControlTemplate(typeof(MenuItem));
        var border = new FrameworkElementFactory(typeof(Border)); border.Name = "MenuBackground"; border.SetValue(Border.BackgroundProperty, Brushes.Transparent); border.SetValue(Border.CornerRadiusProperty, new CornerRadius(3));
        var content = new FrameworkElementFactory(typeof(ContentPresenter)); content.SetBinding(ContentPresenter.ContentProperty, new Binding("Header") { RelativeSource = new(RelativeSourceMode.TemplatedParent) }); content.SetValue(FrameworkElement.MarginProperty, new Thickness(10, 7, 10, 7)); border.AppendChild(content); template.VisualTree = border;
        var highlight = new Trigger { Property = MenuItem.IsHighlightedProperty, Value = true }; highlight.Setters.Add(new Setter(Border.BackgroundProperty, UiTheme.Surface, "MenuBackground")); template.Triggers.Add(highlight);
        var disabled = new Trigger { Property = UIElement.IsEnabledProperty, Value = false }; disabled.Setters.Add(new Setter(UIElement.OpacityProperty, .4)); template.Triggers.Add(disabled); return template;
    }
    private static ControlTemplate BuildMenuTemplate()
    {
        var template = new ControlTemplate(typeof(ContextMenu)); var border = new FrameworkElementFactory(typeof(Border));
        border.SetBinding(Border.BackgroundProperty, new Binding("Background") { RelativeSource = new(RelativeSourceMode.TemplatedParent) });
        border.SetBinding(Border.BorderBrushProperty, new Binding("BorderBrush") { RelativeSource = new(RelativeSourceMode.TemplatedParent) });
        border.SetBinding(Border.PaddingProperty, new Binding("Padding") { RelativeSource = new(RelativeSourceMode.TemplatedParent) }); border.SetValue(Border.BorderThicknessProperty, new Thickness(1)); border.SetValue(Border.CornerRadiusProperty, new CornerRadius(5));
        var scroll = new FrameworkElementFactory(typeof(ScrollViewer)); scroll.SetValue(ScrollViewer.VerticalScrollBarVisibilityProperty, ScrollBarVisibility.Auto); scroll.SetValue(ScrollViewer.HorizontalScrollBarVisibilityProperty, ScrollBarVisibility.Disabled); scroll.AppendChild(new FrameworkElementFactory(typeof(ItemsPresenter))); border.AppendChild(scroll); template.VisualTree = border; return template;
    }
}

internal static class UiIcons
{
    public static readonly Brush ActiveBackground = UiTheme.Selected, InactiveBackground = UiTheme.Surface;
    public static readonly ImageSource Mic = Draw("M9,3 A3,3 0 0 1 15,3 L15,11 A3,3 0 0 1 9,11 Z M6,10 L6,11 A6,6 0 0 0 18,11 L18,10 M12,17 L12,22 M8,22 L16,22");
    public static readonly ImageSource MicOff = Draw("M9,3 A3,3 0 0 1 15,3 L15,11 A3,3 0 0 1 9,11 Z M6,10 L6,11 A6,6 0 0 0 18,11 L18,10 M12,17 L12,22 M8,22 L16,22", true);
    public static readonly ImageSource Headphones = Draw("M3,13 L3,11 A9,9 0 0 1 21,11 L21,13 M3,12 L7,12 L7,21 L3,21 Z M17,12 L21,12 L21,21 L17,21 Z");
    public static readonly ImageSource HeadphonesOff = Draw("M3,13 L3,11 A9,9 0 0 1 21,11 L21,13 M3,12 L7,12 L7,21 L3,21 Z M17,12 L21,12 L21,21 L17,21 Z", true);
    public static readonly ImageSource Game = Draw("M6,6 C3,6 2,8 1,15 C0,20 3,22 6,18 L8,15 L16,15 L18,18 C21,22 24,20 23,15 C22,8 21,6 18,6 Z M5,10 L11,10 M8,7 L8,13 M17,9 L17,10 M20,12 L20,13");
    public static readonly ImageSource GameOff = Draw("M6,6 C3,6 2,8 1,15 C0,20 3,22 6,18 L8,15 L16,15 L18,18 C21,22 24,20 23,15 C22,8 21,6 18,6 Z M5,10 L11,10 M8,7 L8,13 M17,9 L17,10 M20,12 L20,13", true);
    public static readonly ImageSource Pin = Draw("M8,2 L18,12 M9,3 L6,8 L10,12 L15,9 M6,8 L3,9 L11,17 L12,14 M7,13 L2,22");
    public static readonly ImageSource Video = Draw("M2,5 L16,5 L16,19 L2,19 Z M16,10 L22,6 L22,18 L16,14");
    public static readonly ImageSource Phone = Draw("M5,2 L9,7 L6,10 C8,14 10,16 14,18 L17,15 L22,19 L20,22 C18,24 12,20 8,16 C4,12 1,6 2,4 Z");
    public static readonly ImageSource Members = Draw("M9,3 A4,4 0 1 0 9,11 A4,4 0 1 0 9,3 M2,22 L2,19 C2,12 16,12 16,19 L16,22 M17,3 A4,4 0 0 1 17,11 M19,15 C22,15 23,18 23,22");
    public static readonly ImageSource AddPerson = Draw("M9,3 A4,4 0 1 0 9,11 A4,4 0 1 0 9,3 M2,22 L2,19 C2,12 16,12 16,19 L16,22 M20,5 L20,13 M16,9 L24,9");
    private static ImageSource Draw(string geometry, bool slash = false)
    {
        var group = new DrawingGroup();
        group.Children.Add(new GeometryDrawing(Brushes.Transparent, null, new RectangleGeometry(new Rect(0, 0, 24, 24))));
        group.Children.Add(new GeometryDrawing(null, new Pen(TerminalWindow.Ink, 1.7) { StartLineCap = PenLineCap.Round, EndLineCap = PenLineCap.Round, LineJoin = PenLineJoin.Round }, Geometry.Parse(geometry)));
        if (slash) group.Children.Add(new GeometryDrawing(null, new Pen(TerminalWindow.Brush("#F23F43"), 2.2), Geometry.Parse("M2,22 L22,2")));
        return new DrawingImage(group);
    }
    public static void Set(Button button, ImageSource source)
    {
        if (button.Content is Image image) { if (!ReferenceEquals(image.Source, source)) image.Source = source; }
        else button.Content = new Image { Source = source, Width = 22, Height = 22 };
    }
}

public sealed class UiPersonCell : Border
{
    public UiPersonCell() { Padding = new Thickness(10, 12, 10, 12); BorderThickness = new Thickness(0, 0, 0, 1); BorderBrush = UiTheme.Line; DataContextChanged += (_, _) => Render(); }
    private void Render()
    {
        if (DataContext is not UiPersonModel model) { Child = null; return; }
        var relation = model.Relationship; var row = new DockPanel();
        var buttons = new StackPanel { Orientation = Orientation.Horizontal };
        if (relation.Type == 3) buttons.Children.Add(TerminalWindow.Button("Aceptar", () => model.Action(new("accept-friend", relation.User.Id))));
        if (relation.Type == 1) buttons.Children.Add(TerminalWindow.Button("Mensaje", () => model.Action(new("open-dm", relation.User.Id))));
        var remove = TerminalWindow.Button(relation.Type == 1 ? "Quitar" : relation.Type == 4 ? "Cancelar" : "Rechazar", () => model.Action(new("remove-friend", relation.User.Id))); remove.Margin = new Thickness(7, 0, 0, 0); buttons.Children.Add(remove); buttons.IsEnabled = !model.Demo; DockPanel.SetDock(buttons, Dock.Right); row.Children.Add(buttons);
        var avatar = UiPresenceViews.AvatarWithPresence(relation.User, model.Presence?.Status ?? "", 40); avatar.Margin = new Thickness(0, 0, 12, 0); avatar.Cursor = Cursors.Hand; avatar.MouseLeftButtonUp += (_, _) => { if (!model.Demo) model.Action(new("profile", relation.User.Id)); }; DockPanel.SetDock(avatar, Dock.Left); row.Children.Add(avatar);
        var name = new StackPanel(); var title = TerminalWindow.Text(relation.User.Name, 16); title.Cursor = Cursors.Hand; title.MouseLeftButtonUp += (_, _) => { if (!model.Demo) model.Action(new("profile", relation.User.Id)); }; name.Children.Add(title);
        if (relation.Type == 1 && model.Presence != null)
        {
            name.Children.Add(UiPresenceViews.StatusLine(model.Presence.CustomStatus, model.Presence.CustomStatusEmoji, 13));
            foreach (var a in model.Presence.Activities.Take(1)) name.Children.Add(UiPresenceViews.CompactActivity(UiPresenceViews.ActivityLabel(a), 13, TerminalWindow.Muted));
            if (model.Presence.CustomStatus.Length == 0 && model.Presence.Activities.Count == 0) name.Children.Add(TerminalWindow.Text(UiPresenceViews.PresenceLabel(model.Presence.Status), 13, TerminalWindow.Muted));
        }
        else name.Children.Add(TerminalWindow.Text(relation.Type == 3 ? "Solicitud recibida" : relation.Type == 4 ? "Solicitud enviada" : "Estado no disponible", 13, TerminalWindow.Muted)); row.Children.Add(name); Child = row;
    }
}

public sealed class UiMessageCell : Border
{
    private StackPanel? currentActions;
    private TextBlock? continuationTime;
    public UiMessageCell()
    {
        Padding = new Thickness(8, 9, 8, 9); DataContextChanged += (_, _) => Render();
        MouseEnter += (_, _) => { if (currentActions != null) currentActions.Visibility = Visibility.Visible; if (continuationTime != null) continuationTime.Visibility = Visibility.Visible; };
        MouseLeave += (_, _) => { if (currentActions != null) currentActions.Visibility = Visibility.Hidden; if (continuationTime != null) continuationTime.Visibility = Visibility.Hidden; };
    }
    private void Render()
    {
        if (DataContext is not UiMessageModel model) { Child = null; currentActions = null; continuationTime = null; return; }
        var message = model.Message;
        currentActions = null; continuationTime = null;
        if (message.MessageType == 3)
        {
            Padding = new Thickness(10, 12, 10, 12);
            var callLog = new DockPanel(); var icon = TerminalWindow.Text("☎", 24, TerminalWindow.Accent); icon.Width = 56; icon.VerticalAlignment = VerticalAlignment.Top; DockPanel.SetDock(icon, Dock.Left); callLog.Children.Add(icon);
            var detail = new StackPanel(); detail.Children.Add(new TextBlock { Text = message.Content.Length > 0 ? message.Content : "Registro de llamada", FontSize = model.ChatSize, Foreground = TerminalWindow.Ink, TextWrapping = TextWrapping.Wrap });
            string metadata = message.Timestamp.ToLocalTime().ToString("dd MMM · HH:mm");
            detail.Children.Add(TerminalWindow.Text(metadata, 12, TerminalWindow.Muted)); callLog.Children.Add(detail); Child = WithDate(callLog, model);
            var callMenu = UiMenus.Create(); UiMenus.Add(callMenu, "Copiar texto", () => { try { Clipboard.SetText(message.Content); } catch { } }); UiMenus.Add(callMenu, "Copiar ID del mensaje", () => model.Action(new("copy-id", message.Id))); ContextMenu = callMenu; return;
        }
        Padding = new Thickness(10, model.Compact ? 3 : model.Continuation ? 1 : 13, 10, model.Continuation ? 1 : 5);
        var grid = new Grid(); grid.ColumnDefinitions.Add(new() { Width = new GridLength(56) }); grid.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        continuationTime = null;
        if (!model.Continuation)
        {
            var avatar = TerminalWindow.RoundAvatar(message.Author, 40); avatar.VerticalAlignment = VerticalAlignment.Top; avatar.HorizontalAlignment = HorizontalAlignment.Left; avatar.Margin = new Thickness(0, 3, 0, 0); grid.Children.Add(avatar);
            avatar.Cursor = Cursors.Hand; avatar.MouseLeftButtonUp += (_, _) => model.Action(new("profile", message.Author.Id));
        }
        else
        {
            continuationTime = TerminalWindow.Text(message.Timestamp.ToLocalTime().ToString("HH:mm"), 11, TerminalWindow.Muted); continuationTime.Margin = new Thickness(0, 2, 9, 0); continuationTime.HorizontalAlignment = HorizontalAlignment.Right; continuationTime.VerticalAlignment = VerticalAlignment.Top; continuationTime.Visibility = Visibility.Hidden; grid.Children.Add(continuationTime);
        }
        var body = new StackPanel(); Grid.SetColumn(body, 1); grid.Children.Add(body);
        var meta = new DockPanel();
        var actions = new StackPanel { Orientation = Orientation.Horizontal, Visibility = Visibility.Hidden, HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top, Background = UiTheme.Panel, Margin = new Thickness(0, -12, 0, 0) };
        currentActions = actions;
        var access = model.Access ?? ChannelAccess.Private;
        void Add(string label, string kind, string tip) { var button = TerminalWindow.Button(label, () => model.Action(new(kind, message.Id, kind == "reply-local" ? message.Author.Name + ": " + message.Content : message.Content, ChannelId: message.ChannelId))); if (kind is "pin-message" or "unpin-message") button.Content = new Image { Source = UiIcons.Pin, Width = 16, Height = 16 }; bool permitted = kind == "react-local" ? access.CanReact : kind is "reply-local" or "edit-local" ? access.CanSend : access.CanView; button.ToolTip = permitted ? tip : kind == "react-local" ? "Este canal no permite añadir emojis de reacción nuevos." : "No tienes permiso para escribir en este canal."; ToolTipService.SetShowOnDisabled(button, true); button.Padding = new Thickness(6, 2, 6, 2); button.Margin = new Thickness(3, 0, 0, 0); button.IsEnabled = permitted && !model.Demo && model.SignedIn && !message.Deleted; actions.Children.Add(button); }
        if (model.SignedIn || model.Demo)
        {
            Add("↳", "reply-local", "Responder"); Add("➜", "forward-local", "Reenviar"); Add("☺", "react-local", "Añadir reacción");
            if (model.CanPin) Add("♧", message.Pinned ? "unpin-message" : "pin-message", message.Pinned ? "Desfijar mensaje" : "Fijar mensaje");
            if (model.Own) { if (!message.Forwarded) Add("✎", "edit-local", "Editar"); Add("×", "delete-local", "Eliminar"); }
        }
        if (!model.SignedIn && !model.Demo)
        {
            var openChannel = TerminalWindow.Button(model.PinsView ? "Ir al mensaje" : "Abrir canal", () => model.Action(model.PinsView ? new("jump-message", message.Id, ChannelId: message.ChannelId) : new("channel", message.ChannelId, ChannelId: message.ChannelId))); openChannel.Padding = new Thickness(7, 2, 7, 2); actions.Children.Add(openChannel);
            if (model.PinsView && model.CanPin) { var unpin = TerminalWindow.Button("Desfijar", () => model.Action(new("unpin-message", message.Id, ChannelId: message.ChannelId))); unpin.Padding = new Thickness(7, 2, 7, 2); actions.Children.Add(unpin); }
        }
        Grid.SetColumn(actions, 1); Panel.SetZIndex(actions, 2); grid.Children.Add(actions);
        var name = new TextBlock { Foreground = TerminalWindow.Ink, TextTrimming = TextTrimming.CharacterEllipsis, FontSize = 16 };
        name.Inlines.Add(new System.Windows.Documents.Run(message.Author.Name) { FontWeight = FontWeights.SemiBold });
        name.Inlines.Add(new System.Windows.Documents.Run("  " + message.Timestamp.ToLocalTime().ToString(model.ChannelLabel.Length > 0 ? "dd MMM · HH:mm" : "HH:mm") + (message.Edited ? " · editado" : "") + (message.Deleted ? " · eliminado / registro local" : "")) { Foreground = TerminalWindow.Muted, FontSize = 12 }); meta.Children.Add(name);
        if (model.ChannelLabel.Length > 0) { var where = TerminalWindow.Text("# " + model.ChannelLabel, 13, TerminalWindow.Accent); where.Margin = new Thickness(0, 0, 0, 5); body.Children.Add(where); }
        if (!model.Continuation) body.Children.Add(meta);
        if (message.ReplyTo != null) body.Children.Add(UiReplyViews.Build(message, model));
        if (message.Pinned) body.Children.Add(TerminalWindow.Text("Fijado", 11, TerminalWindow.Accent));
        if (message.Forwarded) body.Children.Add(TerminalWindow.Text("➜ Reenviado", 12, TerminalWindow.Muted));
        var content = UiPresenceViews.RichText(message.Content, model.ChatSize, message.Deleted ? TerminalWindow.Muted : TerminalWindow.Ink, url => model.Action(new("link-local", Text: url)), message.Mentions, person => model.Action(new("profile", person.Id)));
        content.Margin = new Thickness(0, model.Continuation ? 0 : 2, 0, 0); content.LineHeight = model.ChatSize * 1.45;
        body.Children.Add(content);
        if (message.Reactions is { Count: > 0 }) body.Children.Add(UiReactionViews.Build(message, model));
        var inlineGifUrls = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (Match match in Regex.Matches(message.Content, @"https://[^\s<>]+", RegexOptions.CultureInvariant).Cast<Match>().Take(6))
        {
            if (MediaImages.IsGifSource(match.Value) && inlineGifUrls.Add(match.Value))
                body.Children.Add(new UiGifImage(match.Value, model.Action));
        }
        foreach (var attachment in message.Attachments.Take(10))
        {
            bool image = attachment.ContentType?.StartsWith("image/") == true || Regex.IsMatch(attachment.Name, @"\.(png|jpg|jpeg|gif|webp)$", RegexOptions.IgnoreCase);
            bool gif = attachment.Animated || attachment.ContentType == "image/gif" || attachment.Name.EndsWith(".gif", StringComparison.OrdinalIgnoreCase);
            if (gif && MediaImages.IsGifSource(attachment.Url))
            {
                if (inlineGifUrls.Contains(attachment.Url) || attachment.PreviewUrl != null && inlineGifUrls.Contains(attachment.PreviewUrl)) continue;
                body.Children.Add(new UiGifImage(attachment.Url, model.Action));
                continue;
            }
            if (image && MediaImages.IsAllowed(attachment.Url))
            {
                var media = new StackPanel { HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, 9, 0, 0) };
                var thumb = new UiMediaImage(attachment.Url, 512) { MaxWidth = 480, MaxHeight = 320, Stretch = Stretch.Uniform, HorizontalAlignment = HorizontalAlignment.Left, Cursor = Cursors.Hand };
                thumb.MouseLeftButtonUp += (_, _) => model.Action(new("image-local", Text: attachment.Url));
                thumb.ContextMenu = TerminalWindow.MediaMenu(attachment.Url, model.Action);
                media.Children.Add(thumb);
                var caption = TerminalWindow.Button("▧ " + attachment.Name + " · ampliar", () => model.Action(new("image-local", Text: attachment.Url))); caption.Padding = new Thickness(6, 3, 6, 3); caption.FontSize = 12; caption.HorizontalAlignment = HorizontalAlignment.Left; media.Children.Add(caption); body.Children.Add(media);
                caption.ContextMenu = TerminalWindow.MediaMenu(attachment.Url, model.Action);
            }
            else
            {
                var file = TerminalWindow.Button("↧ " + attachment.Name + (attachment.Size > 0 ? "  " + (attachment.Size / 1024d).ToString("N0") + " KB" : ""), () => model.Action(new("link-local", Text: attachment.Url))); file.HorizontalAlignment = HorizontalAlignment.Left; file.Margin = new Thickness(0, 7, 0, 0); body.Children.Add(file);
            }
        }
        var menu = UiMenus.Create();
        if (model.PinsView && !model.Demo)
        { UiMenus.Add(menu, "Ir al mensaje", () => model.Action(new("jump-message", message.Id, ChannelId: message.ChannelId))); if (model.CanPin) UiMenus.Add(menu, "Desfijar mensaje", () => model.Action(new("unpin-message", message.Id, ChannelId: message.ChannelId))); }
        if (model.SignedIn && !model.Demo && !message.Deleted)
        {
            UiMenus.Add(menu, "Responder", () => model.Action(new("reply-local", message.Id, message.Author.Name + ": " + message.Content, ChannelId: message.ChannelId)), access.CanSend);
            UiMenus.Add(menu, "Reenviar", () => model.Action(new("forward-local", message.Id, ChannelId: message.ChannelId)));
            if (model.CanPin) UiMenus.Add(menu, message.Pinned ? "Desfijar mensaje" : "Fijar mensaje", () => model.Action(new(message.Pinned ? "unpin-message" : "pin-message", message.Id, ChannelId: message.ChannelId)));
            UiMenus.Add(menu, access.CanReact ? "Añadir reacción" : "Añadir reacción · sin permiso", () => model.Action(new("react-local", message.Id, ChannelId: message.ChannelId)), access.CanReact);
            if (model.Own) { if (!message.Forwarded) UiMenus.Add(menu, "Editar mensaje", () => model.Action(new("edit-local", message.Id, message.Content, ChannelId: message.ChannelId)), access.CanSend); UiMenus.Add(menu, "Eliminar mensaje", () => model.Action(new("delete-local", message.Id, ChannelId: message.ChannelId)), danger: true); }
            menu.Items.Add(new Separator());
        }
        UiMenus.Add(menu, "Copiar texto", () => { try { Clipboard.SetText(message.Content); } catch { } });
        UiMenus.Add(menu, "Copiar ID del mensaje", () => model.Action(new("copy-id", message.Id))); ContextMenu = menu;
        Child = WithDate(grid, model);
    }
    private static UIElement WithDate(UIElement content, UiMessageModel model)
    {
        if (!model.DayStart) return content;
        var stack = new StackPanel(); var date = new Grid { Margin = new Thickness(0, 16, 0, 13) };
        date.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) }); date.ColumnDefinitions.Add(new() { Width = GridLength.Auto }); date.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        foreach (int column in new[] { 0, 2 }) { var line = new Border { Height = 1, Background = UiTheme.Line, VerticalAlignment = VerticalAlignment.Center }; Grid.SetColumn(line, column); date.Children.Add(line); }
        var label = TerminalWindow.Text(model.Message.Timestamp.ToLocalTime().ToString("d 'de' MMMM 'de' yyyy", System.Globalization.CultureInfo.GetCultureInfo("es-ES")), 12, TerminalWindow.Muted); label.Margin = new Thickness(14, 0, 14, 0); label.FontWeight = FontWeights.SemiBold; Grid.SetColumn(label, 1); date.Children.Add(label); stack.Children.Add(date); stack.Children.Add(content); return stack;
    }
}



