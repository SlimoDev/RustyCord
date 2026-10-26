using System.IO;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using NotDiscord.Terminal;

internal static class MessageUiTests
{
    [STAThread] static int Main(string[] args)
    {
        var failures = new List<string>(); int checks = 0;
        void Check(bool value, string label) { checks++; if (!value) failures.Add(label); }
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
        var window = new TerminalWindow { Width = 1380, Height = 880 };
        T Field<T>(string name) => (T)typeof(TerminalWindow).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        var me = new Person("100", "Cuenta ficticia"); var alice = new Person("101", "Apodo de Alice", Username: "alice", DisplayName: "Alice global");
        var reference = new MessageReplyPreview("2000", "200", alice, "Texto original de prueba <@100>", Mentions: new[] { me });
        var message = new ChatMessage("2001", "200", me, "Hola <@101> y <@!101>, nos vemos.", DateTimeOffset.UnixEpoch, Array.Empty<ChatAttachment>(), ReplyTo: "2000", Mentions: new[] { alice }, ReplyPreview: reference);
        var actions = new List<UiAction>();
        var row = new UiMessageCell { DataContext = new UiMessageModel(message, actions.Add, true, false, true, Access: ChannelAccess.Private) };
        string TextOf(DependencyObject root) => string.Join(" ", Descendants(root).OfType<TextBlock>().Select(t => new TextRange(t.ContentStart, t.ContentEnd).Text));
        Check(TextOf(row).Contains("@Apodo de Alice") && !TextOf(row).Contains("<@"), "Mention transport IDs are replaced with the contextual display name in chat and reply previews");
        var tag = Descendants(row).OfType<TextBlock>().SelectMany(t => t.Inlines.OfType<Hyperlink>()).First(h => new TextRange(h.ContentStart, h.ContentEnd).Text == "@Apodo de Alice"); tag.RaiseEvent(new RoutedEventArgs(Hyperlink.ClickEvent));
        Check(actions.Last() is { Kind: "profile", Id: "101" }, "Clicking a resolved mention opens only that person's profile");
        var unknown = new UiMessageCell { DataContext = new UiMessageModel(message with { Content = "Hola <@999999>", ReplyTo = null, ReplyPreview = null, Mentions = Array.Empty<Person>() }, actions.Add, true, false, true, Access: ChannelAccess.Private) };
        Check(TextOf(unknown).Contains("@usuario desconocido") && !TextOf(unknown).Contains("999999"), "Unresolved mentions have an honest label without leaking raw transport syntax");
        var reply = Descendants(row).OfType<Button>().Single(b => b.ToolTip?.ToString() == "Ver mensaje original");
        Check(TextOf(reply).Contains("Apodo de Alice:") && TextOf(reply).Contains("Texto original de prueba @Cuenta ficticia"), "Reply preview includes the original author, body and resolved mentions");
        reply.RaiseEvent(new RoutedEventArgs(Button.ClickEvent));
        Check(actions.Last() is { Kind: "jump-message", Id: "2000", ChannelId: "200" }, "Clicking the reply preview requests the exact original message and channel");
        var deleted = new UiMessageCell { DataContext = new UiMessageModel(message with { ReplyPreview = reference with { Deleted = true } }, actions.Add, true, false, true, Access: ChannelAccess.Private) };
        Check(Descendants(deleted).OfType<Button>().Any(b => !b.IsEnabled && b.ToolTip?.ToString()?.Contains("fue eliminado") == true) && TextOf(deleted).Contains("Mensaje eliminado"), "Deleted originals display a disabled, explicit reply preview");
        var denied = new UiMessageCell { DataContext = new UiMessageModel(message, actions.Add, true, false, true, Access: ChannelAccess.Private with { CanReadHistory = false }) };
        Check(Descendants(denied).OfType<Button>().Any(b => b.ToolTip?.ToString() == "Ver mensaje original" && !b.IsEnabled), "History permission also gates jumping from the preview");
        var state = new AppSnapshot { SignedIn = true, CurrentUser = me, View = "chat", SelectedGuildId = "500", SelectedChannelId = "200", ChannelName = "general", SelectedChannelKind = "text", ChannelAccess = ChannelAccess.Private, Guilds = new[] { new NavItem("500", "Servidor de prueba", "guild") }, Messages = new[] { message }, GameActivityAvailable = true };
        window.Apply(state);
        Check(Field<TextBlock>("workspaceTitle").Text == "Servidor de prueba" && window.Title.Contains("Servidor de prueba"), "Selecting a server updates both workspace heading and window caption");
        window.Apply(state with { SelectedGuildId = "", View = "friends", ChannelName = "Amigos" });
        Check(Field<TextBlock>("workspaceTitle").Text == "NotDiscord" && window.Title.StartsWith("NotDiscord"), "Returning to friends restores the NotDiscord title");
        var dialog = (Window)typeof(TerminalWindow).GetMethod("CreateConfirmationDialog", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, new object[] { "Eliminar mensaje", "¿Eliminar este mensaje de prueba?" })!;
        Check(ReferenceEquals(dialog.Background, window.Background) && dialog.WindowStyle == WindowStyle.None && TextOf((DependencyObject)dialog.Content).Contains("¿Eliminar este mensaje de prueba?"), "Deletion confirmation uses the configured colors and its own themed title bar");
        Check(Descendants((DependencyObject)dialog.Content).OfType<Button>().Any(b => b.IsCancel && b.IsDefault) && Descendants((DependencyObject)dialog.Content).OfType<Button>().Any(b => b.Content?.ToString() == "Eliminar"), "The themed confirmation defaults safely to Cancel and clearly names Delete");
        dialog.Close();
        window.Apply(state with { GameActivityEnabled = false });
        var offImage = ((Image)Field<Button>("activity").Content).Source;
        window.Apply(state with { GameActivityEnabled = true });
        var onImage = ((Image)Field<Button>("activity").Content).Source;
        Check(HasRedPixels(offImage) && !HasRedPixels(onImage), "Game activity shows a red slash only when disabled");
        const string url = "https://cdn.discordapp.com/attachments/200/300/synthetic.png";
        var media = new UiMessageCell { DataContext = new UiMessageModel(message with { ReplyTo = null, ReplyPreview = null, Attachments = new[] { new ChatAttachment("300", "synthetic.png", url, "image/png", 2, 2) } }, actions.Add, true, false, true, Access: ChannelAccess.Private) };
        var imageMenu = Descendants(media).OfType<UiMediaImage>().Select(i => i.ContextMenu).First(m => m != null)!;
        var copy = imageMenu.Items.OfType<MenuItem>().Single(i => i.Header?.ToString() == "Copiar imagen"); copy.RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Check(actions.Last() is { Kind: "copy-media-image-local", Text: url }, "Image right-click offers an explicit copy-image action, separate from its link");
        var bitmap = BitmapSource.Create(2, 1, 96, 96, PixelFormats.Bgra32, null, new byte[] { 0, 0, 255, 255, 0, 255, 0, 0 }, 8); bitmap.Freeze();
        using var png = new MemoryStream();
        var data = (DataObject)typeof(TerminalWindow).Assembly.GetType("NotDiscord.Terminal.ImageClipboard")!.GetMethod("Create", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, new object[] { bitmap, png })!;
        Check(data.GetDataPresent(DataFormats.Bitmap, false) && data.GetDataPresent("PNG", false), "Copy data includes bitmap compatibility and a PNG image, without accessing the real clipboard");
        png.Position = 0; var decoded = BitmapDecoder.Create(png, BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.OnLoad).Frames[0]; var pixels = new byte[8]; new FormatConvertedBitmap(decoded, PixelFormats.Bgra32, null, 0).CopyPixels(pixels, 8, 0);
        Check(decoded.PixelWidth == 2 && decoded.PixelHeight == 1 && pixels[3] == 255 && pixels[7] == 0, "Clipboard PNG retains image dimensions and alpha transparency");
        var member = new ChannelMember(alice);
        var memberCell = new UiMemberCell { DataContext = new UiMemberModel(member, actions.Add) };
        var menuNames = memberCell.ContextMenu.Items.OfType<MenuItem>().Select(i => i.Header?.ToString() ?? "").ToArray();
        Check(menuNames.Contains("Ver perfil") && menuNames.Contains("Enviar mensaje") && menuNames.Contains("Añadir amigo") && !menuNames.Any(s => s.Contains("conversación") || s.Contains("notificaciones") || s.Contains("leído")), "Server member menu has person actions and excludes conversation-management commands");
        int beforeRight = actions.Count; memberCell.RaiseEvent(new System.Windows.Input.MouseButtonEventArgs(System.Windows.Input.Mouse.PrimaryDevice, 0, System.Windows.Input.MouseButton.Right) { RoutedEvent = UIElement.MouseRightButtonUpEvent });
        Check(actions.Count == beforeRight, "Right-clicking a member never opens a profile or sends a request automatically");
        memberCell.ContextMenu.Items.OfType<MenuItem>().Single(i => i.Header?.ToString() == "Añadir amigo").RaiseEvent(new RoutedEventArgs(MenuItem.ClickEvent));
        Check(actions.Last() is { Kind: "add-friend", Id: "101", Text: "alice" }, "Member Add friend explicitly targets the selected person's username and ID");
        memberCell.DataContext = new UiMemberModel(member, actions.Add, RelationshipType: 1);
        Check(!memberCell.ContextMenu.Items.OfType<MenuItem>().Any(i => i.Header?.ToString() == "Añadir amigo"), "Existing friends never receive a duplicate Add friend action");
        memberCell.DataContext = new UiMemberModel(member, actions.Add, IsSelf: true);
        Check(!memberCell.ContextMenu.Items.OfType<MenuItem>().Any(i => i.Header?.ToString() is "Añadir amigo" or "Enviar mensaje"), "Own member menu excludes messaging and adding oneself");
        window.Apply(state with { View = "profile", ProfileUser = alice });
        Check(Descendants(Field<Border>("profilePane")).OfType<Button>().Any(b => b.Content?.ToString() == "Añadir amigo" && b.IsEnabled), "A non-friend's profile offers an enabled Add friend button");
        window.Apply(state with { View = "profile", ProfileUser = alice, Relationships = new[] { new Relationship(alice, 4) } });
        Check(Descendants(Field<Border>("profilePane")).OfType<Button>().Any(b => b.Content?.ToString() == "Solicitud enviada" && !b.IsEnabled) && !Descendants(Field<Border>("profilePane")).OfType<Button>().Any(b => b.Content?.ToString() == "Añadir amigo"), "Pending friendship refreshes the same profile into a non-repeatable Request sent state");
        Check(!window.IsLoaded, "All checks use synthetic UI without account/network, actual deletion or clipboard writes");
        if (args.Length > 0)
        {
            window.Apply(state with { Demo = true, ChannelName = "general", Status = "Datos ficticios · menciones y respuestas" });
            var content = (FrameworkElement)window.Content; content.Measure(new Size(1380, 880)); content.Arrange(new Rect(0, 0, 1380, 880)); content.UpdateLayout(); window.SavePreview(args[0]);
        }
        window.Close(); app.Shutdown();
        Console.WriteLine(JsonSerializer.Serialize(new { success = failures.Count == 0, checks, failures, syntheticDataOnly = true, realNetworkUsed = false, realClipboardWritten = false, realDeletes = 0 })); return failures.Count == 0 ? 0 : 1;
    }
    static bool HasRedPixels(ImageSource source)
    {
        var visual = new DrawingVisual(); using (var dc = visual.RenderOpen()) dc.DrawImage(source, new Rect(0, 0, 48, 48));
        var bitmap = new RenderTargetBitmap(48, 48, 96, 96, PixelFormats.Pbgra32); bitmap.Render(visual); var pixels = new byte[48 * 48 * 4]; bitmap.CopyPixels(pixels, 48 * 4, 0);
        for (int i = 0; i < pixels.Length; i += 4) if (pixels[i + 2] > 190 && pixels[i + 1] < 100 && pixels[i] < 110 && pixels[i + 3] > 150) return true;
        return false;
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
}
