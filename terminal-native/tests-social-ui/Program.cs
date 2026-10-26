using System.Collections;
using System.IO;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using NotDiscord.Terminal;

internal static class Program
{
    [STAThread]
    private static int Main()
    {
        var failures = new List<string>(); int checks = 0;
        void Check(bool value, string name) { checks++; if (!value) failures.Add(name); }
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
        var window = new TerminalWindow { Width = 1000, Height = 760 };
        void Layout() { var content = (FrameworkElement)window.Content; content.Measure(new Size(1000, 760)); content.Arrange(new Rect(0, 0, 1000, 760)); content.UpdateLayout(); }
        T Field<T>(string name) => (T)typeof(TerminalWindow).GetField(name, BindingFlags.NonPublic | BindingFlags.Instance)!.GetValue(window)!;
        var alice = new Person("101", "Synthetic Alice", "https://cdn.discordapp.com/avatars/101/synthetic.png", "synthetic_alice", "Synthetic Alice");
        var bob = new Person("102", "Synthetic Bob", Username: "synthetic_bob");
        DateTimeOffset first = DateTimeOffset.Parse("2026-10-17T12:00:00Z");
        var messages = new[] {
            new ChatMessage("1", "200", alice, "Synthetic first day", first, Array.Empty<ChatAttachment>()),
            new ChatMessage("2", "200", alice, "Synthetic consecutive", first.AddMinutes(1), Array.Empty<ChatAttachment>()),
            new ChatMessage("3", "200", bob, "Synthetic next day", first.AddDays(1), Array.Empty<ChatAttachment>()) };
        var snapshot = new AppSnapshot { Demo = true, SignedIn = true, View = "chat", Connection = "Synthetic offline fixture", SelectedChannelId = "200", SelectedChannelKind = "group", ChannelName = "Synthetic group", CurrentUser = bob,
            Messages = messages, DirectMessages = new[] { new NavItem("200", "Synthetic group", "group") }, CanPinMessages = true,
            GroupMembers = new[] { new ChannelMember(alice, "online", "Synthetic status", new StatusEmoji("😀"), new[] { new PresenceActivity(0, "Synthetic game") }), new ChannelMember(bob, "dnd") } };
        window.Apply(snapshot);
        Layout();
        var models = Field<IEnumerable>("messageModels").Cast<UiMessageModel>().ToArray();
        Check(models.Select(m => m.DayStart).SequenceEqual(new[] { true, false, true }), "Date separator starts each local calendar day");
        Check(models.Select(m => m.Continuation).SequenceEqual(new[] { false, true, false }), "Consecutive messages group only inside the same day");
        var messageCell = new UiMessageCell { DataContext = models[0] };
        Check(Descendants(messageCell).OfType<UiMediaImage>().Any(), "First message of a group contains the real author image control");
        Check(Descendants(messageCell).OfType<Border>().Any(b => b.Clip is EllipseGeometry && b.Width == 40 && b.Height == 40 && b.HorizontalAlignment != HorizontalAlignment.Right), "Author avatar has a fixed visible clipped circular viewport");
        Check(Descendants(messageCell).OfType<TextBlock>().Any(t => t.Text.Contains("2026")), "The date separator is a visible text element");
        var compactCell = new UiMessageCell { DataContext = models[0] with { Compact = true } };
        Check(Descendants(compactCell).OfType<UiMediaImage>().Any() && Descendants(compactCell).OfType<Border>().Any(b => b.Clip is EllipseGeometry && b.Width == 40), "Compact layout retains the first author avatar instead of becoming text-only");
        var member = new UiMemberCell { DataContext = new UiMemberModel(snapshot.GroupMembers[0], _ => { }) };
        Check(Descendants(member).OfType<TextBlock>().Any(t => Text(t).Contains("Synthetic status")) && Descendants(member).OfType<TextBlock>().Any(t => Text(t).Contains("Synthetic game")), "Group members show custom status and game together");
        Check(Descendants(member).OfType<Border>().Any(b => b.CornerRadius.TopLeft > 0 && b.Width is >= 10 and <= 15), "Group members retain their online state indicator alongside custom activity");
        var groupList = Field<ListBox>("groupList");
        Check(groupList.Items.Count == 2, "Group sidebar receives the full member snapshot");
        var footerVoice = Field<System.Windows.Controls.Primitives.UniformGrid>("footerVoiceControls");
        bool footerCollapsedWithoutCall = footerVoice.Visibility == Visibility.Collapsed;
        window.Apply(snapshot with { VoiceConnected = true, CallStartedAt = first, CallChannelName = "Synthetic group" });
        Check(footerCollapsedWithoutCall && footerVoice.Visibility == Visibility.Visible, "Footer voice controls are hidden while idle and visible during a call");
        window.Apply(snapshot with { View = "pins", PinnedMessages = new[] { messages[0] with { Pinned = true } }, HasOlderMessages = true });
        Check(Field<Button>("older").Visibility == Visibility.Collapsed && !Field<Button>("older").IsEnabled, "Older-message action stays hidden in pinned messages even when the chat has older history");
        var emitted = new List<UiAction>();
        var actionCell = new UiMessageCell { DataContext = new UiMessageModel(messages[0], emitted.Add, false, false, true, CanPin: true) };
        var actionMenu = actionCell.ContextMenu.Items.OfType<MenuItem>().ToArray();
        var forwardItem = actionMenu.FirstOrDefault(m => m.Header?.ToString() == "Reenviar");
        var pinItem = actionMenu.FirstOrDefault(m => m.Header?.ToString() == "Fijar mensaje");
        forwardItem?.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent)); pinItem?.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Check(emitted.Any(a => a.Kind == "forward-local" && a.ChannelId == "200" && a.Id == "1") && emitted.Any(a => a.Kind == "pin-message" && a.ChannelId == "200" && a.Id == "1"), "Forward and pin menus emit actions bound to the original message and channel");
        var profile = new ProfileDetails(alice, first.AddYears(-5), first.AddYears(-1), "Synthetic bio", "online", "Synthetic status", new StatusEmoji("😀"), new[] { new PresenceActivity(2, "Spotify", "Synthetic song", "Synthetic artist") }, new[] { bob }, new[] { new NavItem("500", "Synthetic guild", "guild") });
        window.Apply(snapshot with { View = "profile", ProfileUser = alice, ProfileDetails = profile, ProfileSubtitle = "Synthetic profile" });
        Layout();
        var profilePane = Field<Border>("profilePane");
        var texts = Descendants(profilePane).OfType<TextBlock>().Select(Text).ToArray();
        Check(texts.Contains("@synthetic_alice") && texts.Any(t => t.Contains("CUENTA CREADA")) && texts.Any(t => t.Contains("AMIGOS DESDE")), "Profile exposes username and real date sections");
        Check(texts.Any(t => t.Contains("Synthetic song")) && texts.Any(t => t.Contains("Synthetic artist")), "Profile activity includes the track and artist");
        Check(!window.IsLoaded, "No presentation source is opened and media controls never load network images");
        window.Apply(new AppSnapshot());
        Check(profilePane.Child == null && typeof(TerminalWindow).GetField("renderedProfileDetails", BindingFlags.NonPublic | BindingFlags.Instance)!.GetValue(window) == null, "Logout releases the previously rendered private profile and activity visual");
        window.Close();
        var previewWindow = new TerminalWindow { Width = 1360, Height = 860 };
        var previewPreferences = typeof(TerminalWindow).GetField("preferences", BindingFlags.NonPublic | BindingFlags.Instance)!.GetValue(previewWindow)!;
        previewPreferences.GetType().GetProperty("Zoom")!.SetValue(previewPreferences, 1d);
        previewPreferences.GetType().GetProperty("ChatSize")!.SetValue(previewPreferences, 16d);
        previewPreferences.GetType().GetProperty("Compact")!.SetValue(previewPreferences, false);
        typeof(TerminalWindow).GetMethod("ApplyZoom", BindingFlags.NonPublic | BindingFlags.Instance)!.Invoke(previewWindow, null);
        var safeAlice = alice with { AvatarUrl = null }; var safeBob = bob with { AvatarUrl = null };
        var previewSnapshot = snapshot with { CurrentUser = safeBob, ChannelName = "Proyecto · grupo de prueba", SelfPresence = "online", Connection = "DEMO SIN CONEXIÓN", Status = "Todos los datos de esta vista son ficticios", Messages = messages.Select(m => m with { Author = m.Author.Id == alice.Id ? safeAlice : safeBob }).ToArray(),
            DirectMessages = new[] { new NavItem("200", "Proyecto · grupo de prueba", "group", IsPinned: true), new NavItem("201", "Alice", "dm", Presence: "online", RecipientId: alice.Id, CustomStatus: "Creando algo nuevo", CustomStatusEmoji: new StatusEmoji("💡"), ActivityText: "Jugando a Celeste", Activities: new[] { new PresenceActivity(0, "Celeste") }), new NavItem("202", "Bob", "dm", Presence: "idle", RecipientId: bob.Id, ActivityText: "Escuchando música") },
            GroupMembers = new[] { new ChannelMember(safeAlice, "online", "Creando algo nuevo", new StatusEmoji("💡"), new[] { new PresenceActivity(0, "Celeste") }, true), new ChannelMember(safeBob, "idle", "", null, new[] { new PresenceActivity(2, "Spotify", "Una canción de prueba", "Artista de prueba") }), new ChannelMember(new Person("103", "Charlie"), "offline") } };
        string evidence = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../evidence")); Directory.CreateDirectory(evidence);
        previewWindow.Apply(previewSnapshot); previewWindow.Show(); previewWindow.UpdateLayout();
        previewWindow.SavePreview(Path.Combine(evidence, "social-ui-20261019.png"));
        previewWindow.Apply(previewSnapshot with { View = "profile", ProfileUser = safeAlice, ProfileDetails = profile with { User = safeAlice, MutualFriends = new[] { safeBob }, CustomStatus = "Creando algo nuevo", CustomStatusEmoji = new StatusEmoji("💡") }, ProfileSubtitle = "Perfil ficticio" });
        previewWindow.UpdateLayout(); previewWindow.SavePreview(Path.Combine(evidence, "social-profile-ui-20261019.png"));
        previewWindow.Close(); app.Shutdown();
        string report = JsonSerializer.Serialize(new { success = failures.Count == 0, checks, failures, syntheticDataOnly = true, realNetworkUsed = false, accountAccessed = false, syntheticPreviewWindowsOpened = true, screenshots = new[] { Path.Combine(evidence, "social-ui-20261019.png"), Path.Combine(evidence, "social-profile-ui-20261019.png") } }, new JsonSerializerOptions { WriteIndented = true });
        File.WriteAllText(Path.Combine(evidence, "social-ui-checks-20261019.json"), report); Console.WriteLine(report);
        return failures.Count == 0 ? 0 : 1;
    }
    private static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        yield return root;
        for (int i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
            foreach (var child in Descendants(VisualTreeHelper.GetChild(root, i))) yield return child;
    }
    private static string Text(TextBlock block) => new System.Windows.Documents.TextRange(block.ContentStart, block.ContentEnd).Text;
}
