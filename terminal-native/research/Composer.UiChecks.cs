using System.Collections;
using System.IO;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using NotDiscord.Terminal;

internal static class ComposerUiChecks
{
    [STAThread]
    static int Main()
    {
        int checks = 0; var failures = new List<string>();
        void Check(bool value, string label) { checks++; if (!value) failures.Add(label); }
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
        var window = new TerminalWindow { Width = 1200, Height = 820 };
        T Field<T>(string name) => (T)typeof(TerminalWindow).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(window)!;
        object? Invoke(string name, params object[] values) => typeof(TerminalWindow).GetMethod(name, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, values);
        var me = new Person("100", "Synthetic self");
        var snapshot = new AppSnapshot { CurrentUser = me, SignedIn = true, View = "chat", SelectedChannelId = "200", SelectedChannelKind = "dm", ChannelName = "Chat de prueba", ChannelAccess = ChannelAccess.Private,
            DirectMessages = new[] { new NavItem("200", "Chat A", "dm", Access: ChannelAccess.Private), new NavItem("201", "Chat B", "dm", Access: ChannelAccess.Private) } };
        var actions = new List<UiAction>(); window.ActionRequested += actions.Add;
        var fixtureFolder = Path.Combine(@"E:\DESCARGAS\REA\NotDiscord\terminal-native\research", "composer-fixture-" + Guid.NewGuid().ToString("N")); Directory.CreateDirectory(fixtureFolder);
        string picturePath = Path.Combine(fixtureFolder, "synthetic.png");
        var bitmap = BitmapSource.Create(2, 2, 96, 96, PixelFormats.Bgra32, null, new byte[] { 0,0,255,255, 0,255,0,255, 255,0,0,255, 255,255,255,255 }, 8); bitmap.Freeze();
        var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(bitmap)); using (var output = File.Create(picturePath)) encoder.Save(output);
        try
        {
            window.Apply(snapshot); var composer = Field<TextBox>("composer"); composer.Text = "Texto A";
            Invoke("StageFiles", (object)new[] { picturePath });
            Check(actions.Count == 0 && Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Count == 1, "Choosing a file only stages its preview and never sends it");
            Check(Field<WrapPanel>("stagedAttachmentView").Children.Count == 1, "A staged image produces a removable preview in the composer");
            Invoke("SendPreparedMessage");
            var send = actions.LastOrDefault(a => a.Kind == "send-prepared");
            var prepared = send == null ? null : JsonSerializer.Deserialize<PreparedSend>(send.Extra);
            Check(send?.Text == "Texto A" && prepared?.Paths.Single() == picturePath && composer.Text == "Texto A", "Explicit send captures text and files while retaining the draft until ACK");
            window.Apply(snapshot with { SelectedChannelId = "201" }); composer.Text = "Texto B";
            Check(Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Count == 0, "Another conversation begins with its own empty attachment list");
            window.Apply(snapshot);
            Check(composer.Text == "Texto A" && Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Count == 1, "Returning to the first chat restores text and its staged file");
            window.Apply(snapshot with { View = "profile", ProfileUser = me });
            Check(composer.Text == "Texto A" && Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Count == 1, "Opening a profile does not erase the chat draft");
            Invoke("ReturnToPreviousChat"); Check(actions.Last().Kind == "return-chat", "Profile back returns through the cached-context action rather than reloading a channel");
            window.Apply(snapshot with { SelectedChannelId = "201" });
            if (prepared != null) window.AcknowledgePreparedSend("200", "Texto A", "", prepared.SubmissionId);
            Check(composer.Text == "Texto B", "An ACK from a different chat never clears the visible draft");
            window.Apply(snapshot); Check(composer.Text.Length == 0 && Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Count == 0, "The sent chat is cleared after its own ACK even after navigation");
            int sentBeforeClipboard = actions.Count;
            Invoke("StageClipboardImage", bitmap);
            var clipboard = Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Single();
            Check(clipboard.Temporary && File.Exists(clipboard.Path) && actions.Count == sentBeforeClipboard, "Pasting image pixels creates a temporary staged PNG without sending");
            Invoke("ClearCurrentDraft");
            Check(!File.Exists(clipboard.Path) && File.Exists(picturePath), "Discard removes only app-created temporary images and preserves selected originals");
            using (var encoded = File.OpenRead(picturePath)) Invoke("StageClipboardPng", encoded);
            Check(Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Single().Temporary && actions.Count == sentBeforeClipboard, "A PNG-only clipboard provider also produces a staged image without invoking text Paste or sending");
            Invoke("ClearCurrentDraft");
            composer.Text = "Texto sin archivos permitido"; Invoke("StageFiles", (object)new[] { picturePath });
            window.Apply(snapshot with { ChannelAccess = new ChannelAccess(true, true, true, true, false, true, true, "No puedes adjuntar") });
            Check(!Field<Button>("send").IsEnabled && Field<WrapPanel>("stagedAttachmentView").IsEnabled, "Losing attachment permission disables sending the batch while still allowing local removal");
            string rejectedAttachment = Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Single().Id; Invoke("RemoveStaged", rejectedAttachment);
            Check(Field<Button>("send").IsEnabled && composer.Text == "Texto sin archivos permitido", "Removing the disallowed attachment immediately re-enables sending retained text without waiting for a Gateway update");
            window.Apply(snapshot with { ChannelAccess = new ChannelAccess(true, true, true, false, false, false, true, "Sólo lectura") });
            Invoke("StageFiles", (object)new[] { picturePath }); Invoke("SendPreparedMessage");
            Check(actions.Count == sentBeforeClipboard && Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Count == 0, "Read-only permissions prevent both staging and submission");
            var timeline = Enumerable.Range(0, 100).Select(i => new ChatMessage((1000 + i).ToString(), "200", me, "Mensaje ficticio de desplazamiento " + i, DateTimeOffset.UnixEpoch.AddMinutes(i * 6), Array.Empty<ChatAttachment>())).ToArray();
            var chat = snapshot with { Messages = timeline };
            void Layout() { var root = (FrameworkElement)window.Content; root.Measure(new Size(1200, 820)); root.Arrange(new Rect(0, 0, 1200, 820)); root.UpdateLayout(); }
            void Pump() { var frame = new DispatcherFrame(); Dispatcher.CurrentDispatcher.BeginInvoke(DispatcherPriority.ApplicationIdle, new Action(() => frame.Continue = false)); Dispatcher.PushFrame(frame); }
            window.Apply(chat); Layout(); Pump();
            var viewer = Descendants(Field<ListBox>("messages")).OfType<ScrollViewer>().First();
            void BeginUserScroll() => Field<ListBox>("messages").RaiseEvent(new MouseWheelEventArgs(Mouse.PrimaryDevice, 0, 120) { RoutedEvent = UIElement.PreviewMouseWheelEvent });
            BeginUserScroll(); viewer.ScrollToVerticalOffset(250); Layout(); Pump(); double savedOffset = viewer.VerticalOffset;
            window.Apply(chat with { View = "profile", ProfileUser = me }); Layout(); Pump();
            window.Apply(chat); Layout(); Pump(); Layout();
            Check(savedOffset > 0 && Math.Abs(viewer.VerticalOffset - savedOffset) < 2, "Returning from a profile restores the existing chat viewport after layout");
            window.Apply(chat with { View = "pins", PinnedMessages = timeline.TakeLast(40).Reverse().Select(m => m with { Pinned = true }).ToArray() }); Layout(); Pump(); Layout();
            Check(Math.Abs(viewer.VerticalOffset) < 2, "Pinned messages begin at the top even after viewing the middle of a chat");
            window.Apply(chat); Layout(); Pump(); Layout();
            Check(Math.Abs(viewer.VerticalOffset - savedOffset) < 2, "Leaving pinned messages restores the original chat scroll position (expected=" + savedOffset + ",actual=" + viewer.VerticalOffset + ")");
            var timelineB = timeline.Select(m => m with { ChannelId = "201", Id = "2" + m.Id }).ToArray();
            var chatB = chat with { SelectedChannelId = "201", Messages = timelineB };
            window.Apply(chatB with { Messages = Array.Empty<ChatMessage>() }); Layout(); Pump();
            window.Apply(chatB); Layout(); Pump(); Layout();
            Check(viewer.ScrollableHeight > 0 && viewer.ScrollableHeight - viewer.VerticalOffset < 3, "First entry follows the newest message even when the channel loads through an empty snapshot");
            Check((string)Invoke("GetChatRestoreAnchor", "201")! == "", "A chat following the end sends no obsolete anchor hint when reopening");
            var extraB = timelineB[^1] with { Id = "29999", Content = "Nuevo mensaje ficticio", Timestamp = timelineB[^1].Timestamp.AddMinutes(6) };
            var extendedB = chatB with { Messages = timelineB.Append(extraB).ToArray() };
            window.Apply(extendedB); Layout(); Pump(); Layout();
            Check(viewer.ScrollableHeight - viewer.VerticalOffset < 3, "A conversation following the end continues following appended messages");
            window.Apply(chat with { Messages = Array.Empty<ChatMessage>() }); Layout(); Pump();
            window.Apply(chat); Layout(); Pump(); Layout();
            Check(Math.Abs(viewer.VerticalOffset - savedOffset) < 2, "A remembered chat survives empty loading snapshots and restores its previous viewport (expected=" + savedOffset + ",actual=" + viewer.VerticalOffset + ")");
            string readingAnchor = (string)Invoke("GetChatRestoreAnchor", "200")!;
            Check(readingAnchor.Length > 0 && timeline.Any(m => m.Id == readingAnchor), "A manually scrolled chat exposes its visible message anchor for bounded backend restoration");
            (string Id, double Top) VisibleAnchor()
            {
                foreach (var row in Descendants(Field<ListBox>("messages")).OfType<ListBoxItem>())
                {
                    if (row.DataContext is not UiMessageModel model || row.ActualHeight <= 0) continue;
                    double top = row.TransformToAncestor(viewer).Transform(new Point()).Y;
                    if (top + row.ActualHeight > 0 && top < viewer.ActualHeight) return (model.Message.Id, top);
                }
                return ("", 0);
            }
            var anchorBefore = VisibleAnchor();
            var older = Enumerable.Range(0, 50).Select(i => timeline[0] with { Id = (900 + i).ToString(), Timestamp = timeline[0].Timestamp.AddMinutes((i - 50) * 6), Content = "Historial anterior " + i }).ToArray();
            var prepended = chat with { Messages = older.Concat(timeline).ToArray() };
            window.Apply(prepended); Layout(); Pump(); Layout(); var anchorAfter = VisibleAnchor();
            Check(anchorBefore.Id.Length > 0 && anchorBefore.Id == anchorAfter.Id && Math.Abs(anchorBefore.Top - anchorAfter.Top) < 3, "Prepending older history preserves the same visible message and its position");
            BeginUserScroll(); viewer.ScrollToVerticalOffset(0); Layout(); Pump();
            var readingUpdated = prepended with { Messages = prepended.Messages.Append(extraB with { ChannelId = "200", Id = "19999" }).ToArray() };
            window.Apply(readingUpdated); Layout(); Pump(); Layout();
            Check(viewer.VerticalOffset < 3, "New messages do not pull a reader away after manually scrolling up");
            BeginUserScroll(); viewer.ScrollToVerticalOffset(100); Layout();
            window.Apply(readingUpdated with { Messages = readingUpdated.Messages.ToArray() }); Layout(); Pump(); Layout();
            Check(viewer.VerticalOffset < 900 && !Field<bool>("userScrollPending"), "An incoming refresh during wheel input neither jumps to the end nor leaves the input gesture permanently active");
            window.Apply(chatB with { SelectedChannelId = "202", Messages = timelineB.Select(m => m with { ChannelId = "202" }).ToArray() }); Layout();
            BeginUserScroll(); viewer.ScrollToVerticalOffset(400); Layout(); Pump(); Layout();
            Check(viewer.VerticalOffset > 0 && viewer.VerticalOffset < 900 && viewer.ScrollableHeight - viewer.VerticalOffset > 100, "Manual scrolling cancels an older queued initial jump to the end (actual=" + viewer.VerticalOffset + ")");
            var focused = chat with { FocusMessageId = timeline[35].Id, FocusMessageVersion = 1 };
            window.Apply(focused); Layout(); Pump(); Layout();
            var focusRow = Field<ListBox>("messages").ItemContainerGenerator.ContainerFromItem(Field<ListBox>("messages").SelectedItem) as ListBoxItem;
            double focusTop = focusRow == null ? -999 : focusRow.TransformToAncestor(viewer).Transform(new Point()).Y;
            Check(Field<ListBox>("messages").SelectedItem is UiMessageModel focusModel && focusModel.Message.Id == timeline[35].Id && focusTop >= -2 && focusTop < viewer.ActualHeight, "An explicit reply target takes priority over the remembered chat position and is visibly highlighted");
            BeginUserScroll(); viewer.ScrollToVerticalOffset(20); Layout(); Pump();
            window.Apply(focused with { Status = "Refresco sin un salto nuevo" }); Layout(); Pump(); Layout();
            Check(Math.Abs(viewer.VerticalOffset - 20) < 3, "Repeating the same focus version does not pull the user back after scrolling");
            window.Apply(focused with { FocusMessageVersion = 2 });
            window.Apply(focused with { FocusMessageVersion = 2, Messages = timeline.ToArray() }); Layout(); Pump(); Layout();
            Check(Field<ListBox>("messages").SelectedItem is UiMessageModel repeatedFocus && repeatedFocus.Message.Id == timeline[35].Id && viewer.VerticalOffset > 20, "A new explicit focus survives an intervening same-channel message refresh before layout");
            string savedAcrossChannels = (string)Invoke("GetChatRestoreAnchor", "200")!;
            foreach (string channel in new[] { "203", "204", "205", "206" })
            {
                window.Apply(chat with { SelectedChannelId = channel, Messages = timeline.Select(m => m with { ChannelId = channel }).ToArray(), FocusMessageId = "" }); Layout(); Pump();
            }
            Check(savedAcrossChannels.Length > 0 && (string)Invoke("GetChatRestoreAnchor", "200")! == savedAcrossChannels, "The reading anchor survives navigation across more channels than the three-channel message cache");
            window.Apply(chat); Layout(); Pump(); BeginUserScroll(); viewer.ScrollToVerticalOffset(250); Layout(); Pump();
            Invoke("GoToPresent");
            var latestAction = actions.LastOrDefault();
            window.Apply(chat with { Messages = timeline.ToArray(), FocusMessageId = "" }); Layout(); Pump(); Layout();
            Check(latestAction?.Kind == "latest-messages" && latestAction.ChannelId == "200" && viewer.ScrollableHeight - viewer.VerticalOffset < 3 && (string)Invoke("GetChatRestoreAnchor", "200")! == "", "Go to present explicitly loads the current channel's latest page, clears its old reading anchor and follows the newest message");
            BeginUserScroll(); viewer.ScrollToVerticalOffset(250); Layout(); Pump();
            string unavailableAnchor = (string)Invoke("GetChatRestoreAnchor", "200")!;
            window.Apply(chatB); Layout(); Pump();
            window.Apply(chat with { Messages = Array.Empty<ChatMessage>() }); Layout(); Pump();
            var latestAfterDeletion = timeline.TakeLast(30).Where(m => m.Id != unavailableAnchor).ToArray();
            window.Apply(chat with { Messages = latestAfterDeletion }); Layout(); Pump(); Layout();
            Check(unavailableAnchor.Length > 0 && latestAfterDeletion.All(m => m.Id != unavailableAnchor) && viewer.ScrollableHeight - viewer.VerticalOffset < 3 && (string)Invoke("GetChatRestoreAnchor", "200")! == "", "An unavailable remembered message falls back to the newest message instead of applying its obsolete offset to unrelated history");
            var delayedLayoutWindow = new TerminalWindow { Width = 1200, Height = 820 };
            try
            {
                delayedLayoutWindow.Apply(chat); Pump();
                var delayedRoot = (FrameworkElement)delayedLayoutWindow.Content;
                delayedRoot.Measure(new Size(1200, 820)); delayedRoot.Arrange(new Rect(0, 0, 1200, 820)); delayedRoot.UpdateLayout(); Pump(); delayedRoot.UpdateLayout();
                var delayedMessages = (ListBox)typeof(TerminalWindow).GetField("messages", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(delayedLayoutWindow)!;
                var delayedViewer = Descendants(delayedMessages).OfType<ScrollViewer>().First();
                Check(delayedViewer.ScrollableHeight > 0 && delayedViewer.ScrollableHeight - delayedViewer.VerticalOffset < 3, "A pending initial restore retries when its first dispatcher callback precedes the initial visual layout");
            }
            finally { delayedLayoutWindow.Close(); }
            window.Apply(snapshot); composer.Text = "No conservar tras logout"; Invoke("StageClipboardImage", bitmap);
            string lastTemporary = Field<IReadOnlyList<PendingAttachment>>("stagedAttachments").Single().Path;
            window.Apply(new AppSnapshot());
            Check(composer.Text.Length == 0 && Field<ComposerDraftStore>("composerDrafts").Count == 0 && !File.Exists(lastTemporary), "Logout clears draft text, staged controls and private clipboard files");
            Check(!window.IsLoaded, "Fixture never opens a presentation source or downloads account media");
        }
        finally
        {
            window.Close(); app.Shutdown();
            // This folder contains only the single synthetic file created above.
            File.Delete(picturePath); Directory.Delete(fixtureFolder);
        }
        Console.WriteLine(JsonSerializer.Serialize(new { success = failures.Count == 0, checks, failures, syntheticDataOnly = true, realNetworkUsed = false, realSends = 0, clipboardRead = false })); return failures.Count == 0 ? 0 : 1;
    }
    static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    { for (int i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++) { var child = VisualTreeHelper.GetChild(root, i); yield return child; foreach (var nested in Descendants(child)) yield return nested; } }
}
