using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private sealed record ChatViewport(string AnchorId, double AnchorTop, double Offset, bool AtBottom);
    private readonly Dictionary<string, ChatViewport> chatViewports = new(StringComparer.Ordinal);
    private string? restorePositionChannel;
    private ChatViewport? pendingChatViewport;
    private long chatNavigationEpoch, chatInputEpoch, scheduledChatEpoch = -1, consumedFocusVersion = -1;
    private string consumedFocusId = "";
    private string pendingFocusId = "";
    private bool restoringPosition, userScrollPending;
    private int viewportStyleVersion = -1;
    private readonly DispatcherTimer focusHighlightTimer = new() { Interval = TimeSpan.FromSeconds(2) };
    private UiMessageModel? highlightedMessage;

    private void WireChatNavigation()
    {
        messages.Loaded += (_, _) => RestoreChatPosition();
        messages.PreviewMouseWheel += (_, _) => NotifyChatUserNavigation();
        messages.PreviewKeyDown += (_, e) =>
        {
            if (e.Key is Key.PageUp or Key.PageDown or Key.Home or Key.End or Key.Up or Key.Down)
                NotifyChatUserNavigation();
        };
        messages.PreviewMouseDown += (_, e) =>
        {
            for (DependencyObject? node = e.OriginalSource as DependencyObject; node is Visual; node = VisualTreeHelper.GetParent(node))
                if (node is ScrollBar) { NotifyChatUserNavigation(); break; }
        };
        messages.AddHandler(ScrollBar.ScrollEvent, new ScrollEventHandler((_, _) => NotifyChatUserNavigation()));
        messages.AddHandler(ScrollViewer.ScrollChangedEvent, new ScrollChangedEventHandler((_, e) =>
        {
            if (state.View != "chat" || applying || restoringPosition || state.Messages.Count == 0) return;
            if (e.OriginalSource is not ScrollViewer viewer || viewer != FindVisual<ScrollViewer>(messages)) return;
            if (restorePositionChannel != null) { RestoreChatPosition(); return; }
            if (userScrollPending || e.VerticalChange != 0 && e.ExtentHeightChange == 0 && e.ViewportHeightChange == 0)
                RememberChatPosition();
            else if ((e.ExtentHeightChange != 0 || e.ViewportHeightChange != 0) && chatViewports.TryGetValue(state.SelectedChannelId, out var saved))
            {
                // An image/layout change must not move the reader away from the
                // same message, and a chat following the end should keep doing so.
                restorePositionChannel = state.SelectedChannelId; pendingChatViewport = saved;
                ++chatNavigationEpoch; RestoreChatPosition();
            }
        }));
        focusHighlightTimer.Tick += (_, _) => ClearMessageHighlight();
        Closed += (_, _) => { focusHighlightTimer.Stop(); chatViewports.Clear(); ++chatNavigationEpoch; };
    }

    private void NotifyChatUserNavigation()
    {
        if (state.View != "chat") return;
        ++chatNavigationEpoch; restorePositionChannel = null; pendingChatViewport = null;
        pendingFocusId = "";
        userScrollPending = true;
        long inputEpoch = ++chatInputEpoch;
        Dispatcher.BeginInvoke(DispatcherPriority.ApplicationIdle, new Action(() =>
        {
            if (inputEpoch != chatInputEpoch) return;
            RememberChatPosition(); userScrollPending = false;
        }));
    }

    private void RememberChatPosition()
    {
        if (restoringPosition || state.View != "chat" || state.SelectedChannelId.Length == 0 || state.Messages.Count == 0 || restorePositionChannel == state.SelectedChannelId) return;
        var viewer = FindVisual<ScrollViewer>(messages);
        if (viewer == null || viewer.ViewportHeight <= 0 || messages.Visibility != Visibility.Visible) return;
        string anchor = ""; double top = 0;
        foreach (var row in VisibleChatRows(viewer))
        {
            anchor = row.Model.Message.Id; top = row.Top; break;
        }
        var saved = new ChatViewport(anchor, top, viewer.VerticalOffset, viewer.ScrollableHeight - viewer.VerticalOffset <= 6);
        if (!chatViewports.ContainsKey(state.SelectedChannelId) && chatViewports.Count >= 1024) chatViewports.Remove(chatViewports.Keys.First());
        chatViewports[state.SelectedChannelId] = saved;
    }

    private IEnumerable<(UiMessageModel Model, double Top)> VisibleChatRows(ScrollViewer viewer)
    {
        // Only realised containers are inspected; long histories stay virtualised.
        var panel = FindVisual<VirtualizingStackPanel>(messages);
        if (panel == null) yield break;
        foreach (var item in panel.Children.OfType<ListBoxItem>())
        {
            if (item.DataContext is not UiMessageModel model || item.ActualHeight <= 0) continue;
            double top;
            try { top = item.TransformToAncestor(viewer).Transform(new Point()).Y; }
            catch (InvalidOperationException) { continue; }
            if (top + item.ActualHeight > 0 && top < viewer.ActualHeight) yield return (model, top);
        }
    }

    private void PrepareChatPositionRestore(AppSnapshot incoming)
    {
        bool accountChanged = incoming.CurrentUser?.Id != state.CurrentUser?.Id;
        bool scopeChanged = incoming.SelectedChannelId != state.SelectedChannelId || incoming.View != state.View;
        if (accountChanged)
        {
            chatViewports.Clear(); restorePositionChannel = null; pendingChatViewport = null;
            consumedFocusVersion = -1; consumedFocusId = ""; ClearMessageHighlight();
        }
        if (accountChanged || scopeChanged)
        {
            ++chatNavigationEpoch; ++chatInputEpoch; userScrollPending = false;
            restorePositionChannel = null; pendingChatViewport = null; pendingFocusId = "";
            if (incoming.View == "chat" && incoming.SelectedChannelId.Length > 0)
            {
                restorePositionChannel = incoming.SelectedChannelId;
                chatViewports.TryGetValue(incoming.SelectedChannelId, out pendingChatViewport);
            }
        }
        else if (incoming.View == "chat" && incoming.SelectedChannelId.Length > 0 &&
            (!ReferenceEquals(incoming.Messages, state.Messages) || viewportStyleVersion != styleVersion))
        {
            // Keep a pending target through loading snapshots with no messages.
            // Capturing an empty/new layout here would replace the old viewport.
            if (restorePositionChannel != incoming.SelectedChannelId)
            {
                restorePositionChannel = incoming.SelectedChannelId;
                chatViewports.TryGetValue(incoming.SelectedChannelId, out pendingChatViewport);
            }
            ++chatNavigationEpoch;
        }
        viewportStyleVersion = styleVersion;
    }

    private void RestoreChatPosition()
    {
        if (state.View != "chat" || state.SelectedChannelId.Length == 0 || state.Messages.Count == 0) return;
        string channel = state.SelectedChannelId;
        bool newFocus = state.FocusMessageId.Length > 0 &&
            (state.FocusMessageVersion != consumedFocusVersion || state.FocusMessageId != consumedFocusId);
        if (newFocus)
        {
            consumedFocusVersion = state.FocusMessageVersion; consumedFocusId = state.FocusMessageId;
            restorePositionChannel = channel; pendingChatViewport = null; pendingFocusId = state.FocusMessageId; ++chatNavigationEpoch;
        }
        if (restorePositionChannel != channel || scheduledChatEpoch == chatNavigationEpoch) return;
        long epoch = chatNavigationEpoch; scheduledChatEpoch = epoch;
        string focus = pendingFocusId;
        var saved = pendingChatViewport;
        Dispatcher.BeginInvoke(DispatcherPriority.ContextIdle, new Action(() =>
        {
            if (epoch != chatNavigationEpoch || state.View != "chat" || state.SelectedChannelId != channel || restorePositionChannel != channel) return;
            var viewer = FindVisual<ScrollViewer>(messages);
            if (viewer == null || viewer.ViewportHeight <= 0) { scheduledChatEpoch = -1; return; }
            restoringPosition = true;
            try
            {
                messages.UpdateLayout();
                var target = focus.Length > 0 ? messageModels.FirstOrDefault(m => m.Message.Id == focus) :
                    saved?.AnchorId.Length > 0 ? messageModels.FirstOrDefault(m => m.Message.Id == saved.AnchorId) : null;
                if (target != null && (focus.Length > 0 || saved?.AtBottom == false))
                {
                    messages.ScrollIntoView(target); messages.UpdateLayout();
                    if (messages.ItemContainerGenerator.ContainerFromItem(target) is ListBoxItem row)
                    {
                        double top = row.TransformToAncestor(viewer).Transform(new Point()).Y;
                        double desiredTop = focus.Length > 0 ? Math.Max(0, (viewer.ViewportHeight - row.ActualHeight) / 2) : saved!.AnchorTop;
                        viewer.ScrollToVerticalOffset(Math.Max(0, viewer.VerticalOffset + top - desiredTop));
                        if (focus.Length > 0) HighlightMessage(target);
                    }
                    else if (saved != null) viewer.ScrollToVerticalOffset(saved.Offset);
                }
                else if (saved == null || saved.AtBottom || saved.AnchorId.Length > 0)
                {
                    // A recorded message that no longer exists cannot be
                    // restored by applying its old offset to unrelated history.
                    // The backend's latest-page fallback should show its end.
                    if (messageModels.Count > 0) messages.ScrollIntoView(messageModels[^1]);
                    messages.UpdateLayout(); viewer.ScrollToEnd();
                }
                else viewer.ScrollToVerticalOffset(saved.Offset);
                messages.UpdateLayout();
            }
            finally
            {
                restoringPosition = false;
                if (epoch == chatNavigationEpoch) { restorePositionChannel = null; pendingChatViewport = null; pendingFocusId = ""; RememberChatPosition(); }
            }
        }));
    }

    private void HighlightMessage(UiMessageModel target)
    {
        focusHighlightTimer.Stop(); highlightedMessage = target; messages.SelectedItem = target; focusHighlightTimer.Start();
    }
    private void ClearMessageHighlight()
    {
        focusHighlightTimer.Stop();
        if (highlightedMessage != null && ReferenceEquals(messages.SelectedItem, highlightedMessage)) messages.SelectedItem = null;
        highlightedMessage = null;
    }
    private void ReturnToPreviousChat()
    {
        if (state.SelectedChannelId.Length > 0) Emit("return-chat", state.SelectedChannelId);
        else Emit("view", "friends");
    }
    private string GetChatRestoreAnchor(string channel)
        => chatViewports.TryGetValue(channel, out var saved) && !saved.AtBottom ? saved.AnchorId : "";
}
