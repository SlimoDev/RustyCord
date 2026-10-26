using System.Collections;
using System.IO;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using NotDiscord.Terminal;

internal static class UiCheckProgram
{
    [STAThread]
    public static int Main(string[] args)
    {
        var checks = new Dictionary<string, object>();
        var window = new TerminalWindow();
        var actions = new List<UiAction>();
        window.ActionRequested += actions.Add;
        var handle = typeof(TerminalWindow).GetMethod("Handle", BindingFlags.NonPublic | BindingFlags.Instance)!;
        var chat = new AppSnapshot { SignedIn = true, View = "chat", SelectedChannelId = "111111111111111111" };
        window.Apply(chat);
        handle.Invoke(window, new object[] { new UiAction("delete", "222222222222222222", ChannelId: "333333333333333333") });
        Require(actions.Count == 0, "cross-channel action rejected");
        checks["crossChannelRejected"] = true;
        handle.Invoke(window, new object[] { new UiAction("delete", "222222222222222222", ChannelId: chat.SelectedChannelId) });
        Require(actions.Count == 1 && actions[0].ChannelId == chat.SelectedChannelId, "same-channel action retains channel");
        checks["sameChannelPreserved"] = true;
        foreach (string view in new[] { "history", "search" })
        {
            window.Apply(chat with { View = view });
            handle.Invoke(window, new object[] { new UiAction("delete", "222222222222222222", ChannelId: chat.SelectedChannelId) });
            Require(actions.Count == 1, view + " must be read-only");
        }
        checks["historySearchReadOnly"] = true;
        window.Apply(chat with { Demo = true });
        handle.Invoke(window, new object[] { new UiAction("send", Text: "fictitious", ChannelId: chat.SelectedChannelId) });
        Require(actions.Count == 1, "demo must not emit sends"); checks["demoDoesNotSend"] = true;
        var composer = (System.Windows.Controls.TextBox)typeof(TerminalWindow).GetField("composer", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        var reply = typeof(TerminalWindow).GetField("replyId", BindingFlags.Instance | BindingFlags.NonPublic)!;
        composer.Text = "new draft typed during send"; reply.SetValue(window, "42");
        window.ClearComposerIfMatches("older sent text", "42");
        Require(composer.Text == "new draft typed during send", "new draft survives prior send acknowledgment");
        window.ClearComposerIfMatches(composer.Text, "different reply");
        Require(composer.Text.Length > 0, "new reply survives prior acknowledgment");
        window.ClearComposerIfMatches(composer.Text, "42");
        Require(composer.Text == "", "exact acknowledged draft clears"); checks["concurrentDraftPreserved"] = true;

        var avatarBorder = (System.Windows.Controls.Border)typeof(TerminalWindow).GetField("accountAvatar", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        var welcome = (System.Windows.Controls.Border)typeof(TerminalWindow).GetField("welcome", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        var people = (System.Windows.Controls.ListBox)typeof(TerminalWindow).GetField("people", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        var models = (IList)typeof(TerminalWindow).GetField("messageModels", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        var person = new Person("444444444444444444", "test");
        var timeline = chat with { CurrentUser = person, Messages = new[] { new ChatMessage("555555555555555555", chat.SelectedChannelId, person, "local fixture", DateTimeOffset.UtcNow, Array.Empty<ChatAttachment>()) } };
        window.Apply(timeline);
        object avatarVisual = avatarBorder.Child, hiddenWelcome = welcome.Child, rowModel = models[0]!;
        window.Apply(timeline with { Status = "A status event unrelated to the visible messages" });
        Require(ReferenceEquals(avatarVisual, avatarBorder.Child) && ReferenceEquals(hiddenWelcome, welcome.Child) && ReferenceEquals(rowModel, models[0]), "status update reuses unchanged visuals");
        var relations = new[] { new Relationship(person, 1) };
        var friends = timeline with { View = "friends", Relationships = relations };
        window.Apply(friends); object peopleSource = people.ItemsSource;
        window.Apply(friends with { Relationships = new[] { new Relationship(person, 1) }, Status = "Another status" });
        Require(ReferenceEquals(peopleSource, people.ItemsSource), "equivalent relationships reuse list");
        var changedRelationships = new[] { new Relationship(person, 1), new Relationship(new Person("666666666666666666", "friend"), 1) };
        window.Apply(timeline with { Relationships = changedRelationships });
        Require(ReferenceEquals(peopleSource, people.ItemsSource), "hidden people list is not rebuilt");
        window.Apply(friends with { Relationships = changedRelationships });
        Require(!ReferenceEquals(peopleSource, people.ItemsSource) && people.Items.Count == 2, "changed relationships render when opened");
        checks["unchangedVisualsReused"] = true; checks["hiddenPanelsDeferred"] = true;
        var firstMessage = timeline.Messages[0];
        window.Apply(timeline with { Messages = new[] { firstMessage, firstMessage with { Id = "555555555555555556", Content = "continued", Timestamp = firstMessage.Timestamp.AddMinutes(1) }, firstMessage with { Id = "555555555555555557", Author = new Person("777777777777777777", "other"), Timestamp = firstMessage.Timestamp.AddMinutes(2) } } });
        Require(!((UiMessageModel)models[0]!).Continuation && ((UiMessageModel)models[1]!).Continuation && !((UiMessageModel)models[2]!).Continuation, "adjacent messages group only for the same author"); checks["messageGrouping"] = true;
        var dmCell = new UiChannelCell { DataContext = new UiNavModel(new NavItem("111111111111111112", "Friend", "dm", RecipientId: person.Id), actions.Add, true, IsFriend: true, CanCall: true) };
        string[] dmMenu = dmCell.ContextMenu.Items.OfType<System.Windows.Controls.MenuItem>().Select(i => (string)i.Header).ToArray();
        Require(new[] { "Abrir conversación", "Ver perfil", "Llamar", "Fijar conversación", "Silenciar conversación", "Marcar como leído", "Cerrar conversación", "Copiar ID", "Bloquear", "Quitar amistad" }.All(dmMenu.Contains), "DM menu includes implemented actions"); checks["dmContextActions"] = true;
        var guildCell = new UiServerCell { DataContext = new UiNavModel(new NavItem("888888888888888888", "Server", "guild", CanInvite: false), actions.Add, false) };
        Require(!guildCell.ContextMenu.Items.OfType<System.Windows.Controls.MenuItem>().Any(i => Equals(i.Header, "Invitar a personas")), "invite hidden without capability");
        guildCell.DataContext = new UiNavModel(new NavItem("888888888888888888", "Server", "guild", CanInvite: true), actions.Add, false);
        Require(guildCell.ContextMenu.Items.OfType<System.Windows.Controls.MenuItem>().Any(i => Equals(i.Header, "Invitar a personas")), "invite visible with capability"); checks["serverMenuCapabilities"] = true;

        var presenceBuilder = typeof(TerminalWindow).GetMethod("BuildPresenceMenu", BindingFlags.Instance | BindingFlags.NonPublic)!;
        window.Apply(chat with { SelfPresence = "idle" });
        var presenceMenu = (System.Windows.Controls.ContextMenu)presenceBuilder.Invoke(window, null)!;
        var presenceItems = presenceMenu.Items.OfType<System.Windows.Controls.MenuItem>().ToArray();
        Require(presenceItems.Length == 4 && presenceItems[1].IsChecked && presenceItems.Count(i => i.IsChecked) == 1, "only confirmed presence is checked");
        int presenceActionStart = actions.Count;
        foreach (var item in presenceItems) item.RaiseEvent(new System.Windows.RoutedEventArgs(System.Windows.Controls.MenuItem.ClickEvent));
        Require(actions.Skip(presenceActionStart).Select(a => (a.Kind, a.Text)).SequenceEqual(new[] { ("status", "online"), ("status", "idle"), ("status", "dnd"), ("status", "invisible") }), "presence menu emits exact backend values");
        window.Apply(chat with { SelfPresence = "", Connection = "En línea" });
        var unknownMenu = (System.Windows.Controls.ContextMenu)presenceBuilder.Invoke(window, null)!;
        var presenceDot = (System.Windows.Controls.Border)typeof(TerminalWindow).GetField("accountPresenceDot", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        Require(!unknownMenu.Items.OfType<System.Windows.Controls.MenuItem>().Any(i => i.IsChecked) && presenceDot.Visibility == System.Windows.Visibility.Collapsed, "connection status must not imply presence");
        window.Apply(chat with { SelfPresence = "online", Demo = true });
        var demoPresence = (System.Windows.Controls.ContextMenu)presenceBuilder.Invoke(window, null)!;
        Require(demoPresence.Items.OfType<System.Windows.Controls.MenuItem>().All(i => !i.IsEnabled), "demo presence choices are disabled");
        checks["presenceMenuValues"] = true; checks["unknownPresenceNotInferred"] = true; checks["demoPresenceDisabled"] = true;

        var theme = typeof(TerminalWindow).Assembly.GetType("NotDiscord.Terminal.UiTheme")!;
        var applyTheme = theme.GetMethod("Apply", BindingFlags.Static | BindingFlags.Public)!;
        var contrast = theme.GetMethod("Contrast", BindingFlags.Static | BindingFlags.NonPublic)!;
        SolidColorBrush ThemeBrush(string name) => (SolidColorBrush)theme.GetField(name, BindingFlags.Static | BindingFlags.Public)!.GetValue(null)!;
        object originalInk = ThemeBrush("Ink"); double minContrast = 30;
        foreach (string baseColor in new[] { "#313338", "#13263A", "#142B25", "#F1F3F6", "#FFFFFF", "#000000", "#FF0000", "#00FF00", "#0000FF", "#888888" })
        {
            applyTheme.Invoke(null, new object[] { baseColor, baseColor, "#5865F2" });
            foreach (string textRole in new[] { "Ink", "Muted", "Accent", "Danger" }) foreach (string surface in new[] { "Background", "Panel", "Rail", "Surface", "Hover", "Selected", "Menu", "Input" })
            {
                double ratio = (double)contrast.Invoke(null, new object[] { ThemeBrush(textRole).Color, ThemeBrush(surface).Color })!;
                Require(ratio >= 4.49, "readable theme: " + baseColor + " " + textRole + " on " + surface); minContrast = Math.Min(minContrast, ratio);
            }
            Require(ReferenceEquals(originalInk, ThemeBrush("Ink")) && !ThemeBrush("Ink").IsFrozen, "theme brushes remain live without replacing visuals");
        }
        typeof(TerminalWindow).GetMethod("UpdateTheme", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, null);
        checks["themeContrast"] = new { palettes = 10, minimum = Math.Round(minContrast, 3), liveBrushes = true };

        int beforeCall = actions.Count;
        var callSnapshot = chat with { CurrentUser = person, VoiceConnected = true, VoiceAvailable = true, ScreenAvailable = true, CameraAvailable = true, CameraEnabled = true, ScreenSharing = true, CallChannelName = "Test call", CallStatusLabel = "Audio conectado", CallStartedAt = DateTimeOffset.UtcNow.AddMinutes(-2), CallParticipants = new[] { new CallParticipant(person, true, false, false, true, true, true), new CallParticipant(new Person("700000000000000003", "Test peer"), false, true, false, false, false, false) } };
        window.Apply(callSnapshot);
        var callPanel = (System.Windows.Controls.Border)typeof(TerminalWindow).GetField("callPanel", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        var participants = (System.Windows.Controls.WrapPanel)typeof(TerminalWindow).GetField("callParticipants", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        var cameraButton = (System.Windows.Controls.Button)typeof(TerminalWindow).GetField("camera", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        Require(callPanel.Visibility == System.Windows.Visibility.Visible && participants.Children.Count == 2 && actions.Count == beforeCall, "call state rendering does not start camera or calls");
        Require(cameraButton.IsEnabled && Equals(cameraButton.Content, "Apagar cámara"), "camera shows reported state");
        cameraButton.RaiseEvent(new System.Windows.RoutedEventArgs(System.Windows.Controls.Button.ClickEvent));
        Require(actions.Count == beforeCall + 1 && actions[^1].Kind == "toggle-camera", "camera changes only on explicit click");
        window.Apply(callSnapshot with { CameraPreparing = true }); Require(!cameraButton.IsEnabled, "camera unavailable while preparing");
        window.Apply(chat); Require(callPanel.Visibility == System.Windows.Visibility.Collapsed && participants.Children.Count == 0, "call panel clears on disconnect");
        var callRecord = firstMessage with { MessageType = 3, Content = "Llamada finalizada · 2 min", CallStartedAt = firstMessage.Timestamp, CallEndedAt = firstMessage.Timestamp.AddMinutes(2), CallParticipantIds = new[] { person.Id } };
        var callCell = new UiMessageCell { DataContext = new UiMessageModel(callRecord, actions.Add, true, false, true) };
        Require(callCell.ContextMenu.Items.OfType<System.Windows.Controls.MenuItem>().All(i => ((string)i.Header).StartsWith("Copiar")), "call logs do not expose message editing");
        checks["callStateRendering"] = true; checks["cameraExplicitActionOnly"] = true; checks["callLogReadOnly"] = true;
        var groupAvatar = typeof(TerminalWindow).GetMethod("NavAvatar", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, new object[] { new NavItem("700000000000000099", "Local group", "dm", RecipientAvatars: new[] { "https://cdn.discordapp.com/avatars/fixture/a.png", "https://cdn.discordapp.com/avatars/fixture/b.png" }), 40 });
        Require(((System.Windows.Controls.Border)groupAvatar!).Child is System.Windows.Controls.Primitives.UniformGrid mosaic && mosaic.Children.Count == 2, "group fallback uses bounded recipient mosaic"); checks["groupAvatarFallback"] = true;

        window.Apply(chat with { Demo = true, SelectedGuildId = "", DirectMessages = Enumerable.Range(0, 300).Select(i => new NavItem((800000000000000000L + i).ToString(), "Local fixture " + i, "dm", IsPinned: i < 2)).ToArray() });
        window.Show();
        var preferences = typeof(TerminalWindow).GetField("preferences", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        preferences.GetType().GetProperty("Zoom")!.SetValue(preferences, 1.25);
        typeof(TerminalWindow).GetMethod("ApplyZoom", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, null);
        window.Dispatcher.Invoke(() => { }, System.Windows.Threading.DispatcherPriority.ApplicationIdle); window.UpdateLayout();
        var navigation = (System.Windows.Controls.ListBox)typeof(TerminalWindow).GetField("channels", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        int realized = Enumerable.Range(0, navigation.Items.Count).Count(i => navigation.ItemContainerGenerator.ContainerFromIndex(i) != null);
        Require(realized > 0 && realized < 50, "navigation remains virtualized at 125 percent zoom"); checks["zoomVirtualization"] = new { zoom = 1.25, items = navigation.Items.Count, realized };
        Require(navigation.Items.Cast<UiNavModel>().Count(n => n.Item.Kind == "section") == 2, "pinned and normal messages have separate sections"); checks["pinnedSection"] = true;
        preferences.GetType().GetProperty("Zoom")!.SetValue(preferences, 1d); typeof(TerminalWindow).GetMethod("ApplyZoom", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, null);

        var decode = typeof(UiGifImage).GetMethod("Decode", BindingFlags.NonPublic | BindingFlags.Static)!;
        var media = new List<object>();
        foreach (string path in args.Where(a => a.EndsWith(".gif", StringComparison.OrdinalIgnoreCase)))
        {
            object result = decode.Invoke(null, new object[] { File.ReadAllBytes(path), CancellationToken.None })!;
            var frames = ((IEnumerable)result.GetType().GetProperty("Frames")!.GetValue(result)!).Cast<object>().ToArray();
            Require(frames.Length > 1, "GIF has multiple frames");
            var images = frames.Select(f => (BitmapSource)f.GetType().GetProperty("Image")!.GetValue(f)!).ToArray();
            long decodedBytes = images.Sum(i => (long)i.PixelWidth * i.PixelHeight * 4);
            Require(decodedBytes <= 8 * 1024 * 1024 && images.All(i => i.PixelWidth <= 360 && i.PixelHeight <= 220), "GIF frame budget");
            var hashes = images.Select(Hash).Distinct().ToArray(); Require(hashes.Length > 1, "GIF frames visibly differ");
            media.Add(new { file = Path.GetFileName(path), frames = frames.Length, distinctFrames = hashes.Length, decodedBytes, width = images[0].PixelWidth, height = images[0].PixelHeight });
        }
        checks["gifFixtures"] = media;
        var encoder = new GifBitmapEncoder();
        var pixel = BitmapSource.Create(1, 1, 96, 96, PixelFormats.Bgra32, null, new byte[] { 255, 0, 0, 255 }, 4);
        for (int i = 0; i < 401; i++) encoder.Frames.Add(BitmapFrame.Create(pixel));
        using var stream = new MemoryStream(); encoder.Save(stream);
        bool rejected = false;
        try { decode.Invoke(null, new object[] { stream.ToArray(), CancellationToken.None }); }
        catch (TargetInvocationException e) when (e.InnerException is InvalidDataException) { rejected = true; }
        Require(rejected, "oversized frame count rejected"); checks["gifFrameLimitEnforced"] = true;
        var longEncoder = new GifBitmapEncoder(); for (int i = 0; i < 160; i++) longEncoder.Frames.Add(BitmapFrame.Create(pixel)); using var longStream = new MemoryStream(); longEncoder.Save(longStream);
        object longGif = decode.Invoke(null, new object[] { longStream.ToArray(), CancellationToken.None })!; Require(((IEnumerable)longGif.GetType().GetProperty("Frames")!.GetValue(longGif)!).Cast<object>().Count() == 160, "160-frame GIF is now supported"); checks["longGifSupported"] = true;
        int previewIndex = Array.IndexOf(args, "--preview");
        if (previewIndex >= 0 && previewIndex + 1 < args.Length)
        {
            bool originalCompact = (bool)preferences.GetType().GetProperty("Compact")!.GetValue(preferences)!;
            preferences.GetType().GetProperty("Compact")!.SetValue(preferences, false);
            var me = new Person("700000000000000001", "tú"); var nora = new Person("700000000000000002", "Nora"); var ivo = new Person("700000000000000003", "Ivo");
            var now = DateTimeOffset.Now;
            window.Apply(new AppSnapshot
            {
                Demo = true, CurrentUser = me, SelfPresence = "online", View = "chat", SelectedChannelId = "700000000000000010", ChannelName = "Nora", Connection = "Demostración local", Status = "Datos ficticios · revisión visual sin conexión ni pruebas de rendimiento",
                Guilds = new[] { new NavItem("700000000000000020", "Amigos", "guild"), new NavItem("700000000000000021", "Juegos", "guild"), new NavItem("700000000000000022", "Diseño", "guild") },
                DirectMessages = new[]
                {
                    new NavItem("700000000000000011", "Equipo nocturno", "dm", IsPinned: true, Subtitle: "4 miembros"),
                    new NavItem("700000000000000012", "Ivo", "dm", IsPinned: true, Subtitle: "Jugando a Terraria", Presence: "online", RecipientId: ivo.Id),
                    new NavItem("700000000000000010", "Nora", "dm", Subtitle: "En línea", Presence: "online", RecipientId: nora.Id),
                    new NavItem("700000000000000013", "Mara", "dm", Unread: true, Subtitle: "Escuchando música", Presence: "idle", RecipientId: "700000000000000004", MentionCount: 2),
                    new NavItem("700000000000000014", "Leo", "dm", Subtitle: "No molestar", Presence: "dnd", RecipientId: "700000000000000005"),
                    new NavItem("700000000000000015", "Proyecto personal", "dm", Unread: true, Subtitle: "3 miembros"),
                    new NavItem("700000000000000016", "Dani", "dm", Subtitle: "Desconectado", Presence: "offline", RecipientId: "700000000000000006")
                },
                Messages = new[]
                {
                    new ChatMessage("700000000000000100", "700000000000000010", nora, "¿Qué tal se ve ahora la interfaz?", now.AddMinutes(-8), Array.Empty<ChatAttachment>()),
                    new ChatMessage("700000000000000101", "700000000000000010", nora, "Los mensajes del mismo autor se agrupan y las conversaciones fijadas tienen su propia sección.", now.AddMinutes(-7), Array.Empty<ChatAttachment>()),
                    new ChatMessage("700000000000000102", "700000000000000010", me, "Mucho más cómoda. Ahora el texto tiene el tamaño habitual y el campo de escribir deja más espacio al chat.", now.AddMinutes(-5), Array.Empty<ChatAttachment>()),
                    new ChatMessage("700000000000000103", "700000000000000010", me, "También puedo cambiar el zoom con Ctrl + y Ctrl −.", now.AddMinutes(-4), Array.Empty<ChatAttachment>()),
                    new ChatMessage("700000000000000104", "700000000000000010", nora, "El clic derecho abre las acciones de mensajes, conversaciones y servidores. Los GIFs siguen reproduciéndose sólo cuando tú lo pides.", now.AddMinutes(-2), Array.Empty<ChatAttachment>()),
                    new ChatMessage("700000000000000105", "700000000000000010", me, "Perfecto, así puedo seguir usando el ratón como siempre.", now.AddMinutes(-1), Array.Empty<ChatAttachment>())
                }
            });
            window.Show(); window.Dispatcher.Invoke(() => { }, System.Windows.Threading.DispatcherPriority.ApplicationIdle); window.SavePreview(args[previewIndex + 1]);
            var menu = dmCell.ContextMenu; menu.PlacementTarget = window; menu.IsOpen = true; window.Dispatcher.Invoke(() => { }, System.Windows.Threading.DispatcherPriority.ApplicationIdle);
            var menuImage = new RenderTargetBitmap(Math.Max(1, (int)Math.Ceiling(menu.ActualWidth)), Math.Max(1, (int)Math.Ceiling(menu.ActualHeight)), 96, 96, PixelFormats.Pbgra32); menuImage.Render(menu);
            var menuEncoder = new PngBitmapEncoder(); menuEncoder.Frames.Add(BitmapFrame.Create(menuImage)); using (var menuFile = File.Create(Path.ChangeExtension(args[previewIndex + 1], ".dm-menu.png"))) menuEncoder.Save(menuFile); menu.IsOpen = false;
            presenceMenu.PlacementTarget = window; presenceMenu.IsOpen = true; window.Dispatcher.Invoke(() => { }, System.Windows.Threading.DispatcherPriority.ApplicationIdle);
            var presenceImage = new RenderTargetBitmap(Math.Max(1, (int)Math.Ceiling(presenceMenu.ActualWidth)), Math.Max(1, (int)Math.Ceiling(presenceMenu.ActualHeight)), 96, 96, PixelFormats.Pbgra32); presenceImage.Render(presenceMenu);
            var presenceEncoder = new PngBitmapEncoder(); presenceEncoder.Frames.Add(BitmapFrame.Create(presenceImage)); using (var presenceFile = File.Create(Path.ChangeExtension(args[previewIndex + 1], ".presence-menu.png"))) presenceEncoder.Save(presenceFile); presenceMenu.IsOpen = false;
            applyTheme.Invoke(null, new object[] { "#13263A", "#36516A", "#54BDFF" }); window.SavePreview(Path.ChangeExtension(args[previewIndex + 1], ".blue.png"));
            applyTheme.Invoke(null, new object[] { "#F1F3F6", "#ABB4C1", "#4055CB" }); window.SavePreview(Path.ChangeExtension(args[previewIndex + 1], ".light.png"));
            applyTheme.Invoke(null, new object[] { "#13263A", "#36516A", "#54BDFF" });
            var demoState = (AppSnapshot)typeof(TerminalWindow).GetField("state", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
            window.Apply(demoState with { VoiceConnected = true, VoiceAvailable = true, ScreenAvailable = true, CameraAvailable = true, ScreenSharing = true, CallChannelName = "Nora", CallStatusLabel = "Audio conectado · demostración", CallStartedAt = DateTimeOffset.UtcNow.AddSeconds(-154), UnreadMessageCount = 3, NewMessageNotice = "Mensajes sin leer en otras conversaciones", TypingText = "Nora está escribiendo…", CallParticipants = new[] { new CallParticipant(me, true, false, false, false, false, true), new CallParticipant(nora, false, false, false, true, true, false), new CallParticipant(ivo, false, true, false, false, false, false) }, Messages = demoState.Messages.Concat(new[] { new ChatMessage("700000000000000106", demoState.SelectedChannelId, nora, "Nora inició una llamada · 3 participantes", now, Array.Empty<ChatAttachment>(), MessageType: 3, CallStartedAt: now, CallParticipantIds: new[] { me.Id, nora.Id, ivo.Id }) }).ToArray() });
            window.Dispatcher.Invoke(() => { }, System.Windows.Threading.DispatcherPriority.ApplicationIdle); window.SavePreview(Path.ChangeExtension(args[previewIndex + 1], ".call.png"));
            window.Apply(demoState);
            typeof(TerminalWindow).GetMethod("UpdateTheme", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, null);
            var themeBeforeDialog = ThemeBrush("Background").Color;
            string preferenceBeforeDialog = (string)preferences.GetType().GetProperty("Background")!.GetValue(preferences)!;
            bool previewObserved = false;
            window.Dispatcher.BeginInvoke(() =>
            {
                var settings = window.OwnedWindows.Cast<System.Windows.Window>().Single(w => w.Title == "Ajustes de NotDiscord");
                var textInputs = Descendants(settings).OfType<System.Windows.Controls.TextBox>().ToArray();
                Require(textInputs.Length >= 3, "settings expose base, separator and accent fields");
                textInputs[0].Text = "#B11226"; settings.UpdateLayout();
                Require(ThemeBrush("Background").Color != themeBeforeDialog && (string)preferences.GetType().GetProperty("Background")!.GetValue(preferences)! == preferenceBeforeDialog, "live theme preview does not mutate preferences");
                if (settings.Content is System.Windows.FrameworkElement content)
                {
                    var settingsImage = new RenderTargetBitmap(Math.Max(1, (int)Math.Ceiling(content.ActualWidth)), Math.Max(1, (int)Math.Ceiling(content.ActualHeight)), 96, 96, PixelFormats.Pbgra32); settingsImage.Render(content);
                    var settingsEncoder = new PngBitmapEncoder(); settingsEncoder.Frames.Add(BitmapFrame.Create(settingsImage)); using var settingsFile = File.Create(Path.ChangeExtension(args[previewIndex + 1], ".settings.png")); settingsEncoder.Save(settingsFile);
                }
                previewObserved = true; settings.Close();
            }, System.Windows.Threading.DispatcherPriority.ApplicationIdle);
            typeof(TerminalWindow).GetMethod("PreferencesDialog", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, null);
            Require(previewObserved && ThemeBrush("Background").Color == themeBeforeDialog, "closing unsaved settings restores the prior theme"); checks["themePreviewCancelRestores"] = true;
            preferences.GetType().GetProperty("Compact")!.SetValue(preferences, originalCompact);
            checks["previewSaved"] = true;
        }
        Console.WriteLine(JsonSerializer.Serialize(checks, new JsonSerializerOptions { WriteIndented = true }));
        window.Close(); return 0;
    }
    private static void Require(bool valid, string message) { if (!valid) throw new InvalidOperationException(message); }
    private static IEnumerable<System.Windows.DependencyObject> Descendants(System.Windows.DependencyObject root)
    {
        for (int i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++) { var child = VisualTreeHelper.GetChild(root, i); yield return child; foreach (var nested in Descendants(child)) yield return nested; }
    }
    private static string Hash(BitmapSource image)
    {
        var pixels = new byte[image.PixelWidth * image.PixelHeight * 4]; image.CopyPixels(pixels, image.PixelWidth * 4, 0); return Convert.ToHexString(SHA256.HashData(pixels));
    }
}
