using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private void ScrollPinnedStart(string channel)
    {
        long epoch = chatNavigationEpoch;
        Dispatcher.BeginInvoke(System.Windows.Threading.DispatcherPriority.ContextIdle, new Action(() =>
        {
            if (epoch != chatNavigationEpoch || state.View != "pins" || state.SelectedChannelId != channel) return;
            messages.UpdateLayout();
            if (messages.Items.Count > 0) messages.ScrollIntoView(messages.Items[0]);
            FindVisual<ScrollViewer>(messages)?.ScrollToTop();
        }));
    }
    private static void WireNavigationContextMenus(ListBox list)
    {
        // ListBoxItem normally selects on right-down. The menu must not navigate,
        // change a draft's active channel or mark another chat as read.
        list.PreviewMouseRightButtonDown += (_, e) => e.Handled = true;
        list.PreviewMouseRightButtonUp += (_, e) =>
        {
            e.Handled = true;
            if (e.OriginalSource is not DependencyObject source || ItemsControl.ContainerFromElement(list, source) is not ListBoxItem item) return;
            FrameworkElement? cell = FindVisual<UiChannelCell>(item) ?? (FrameworkElement?)FindVisual<UiServerCell>(item) ?? FindVisual<UiAttentionCell>(item);
            if (cell?.ContextMenu is not ContextMenu menu) return;
            menu.PlacementTarget = cell; menu.Placement = PlacementMode.MousePoint; menu.IsOpen = true;
        };
    }
    internal static ContextMenu MediaMenu(string url, Action<UiAction> action)
    {
        var menu = UiMenus.Create();
        UiMenus.Add(menu, "Ampliar imagen / GIF", () => action(new("image-local", Text: url)));
        UiMenus.Add(menu, "Copiar imagen", () => action(new("copy-media-image-local", Text: url)));
        UiMenus.Add(menu, "Guardar imagen / GIF…", () => action(new("save-media-local", Text: url)));
        UiMenus.Add(menu, "Copiar enlace", () => action(new("copy-media-link-local", Text: url)));
        return menu;
    }
}
