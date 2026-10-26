using System.Windows;
using System.Windows.Controls;
using System.Windows.Shell;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private Window CreateConfirmationDialog(string title, string message)
    {
        var dialog = Dialog(title, 530, 250);
        dialog.WindowStyle = WindowStyle.None; dialog.ResizeMode = ResizeMode.NoResize;
        dialog.ShowInTaskbar = false; dialog.SizeToContent = SizeToContent.Height;
        dialog.MaxHeight = SystemParameters.WorkArea.Height - 80;
        WindowChrome.SetWindowChrome(dialog, new WindowChrome { CaptionHeight = 42, ResizeBorderThickness = new Thickness(0), GlassFrameThickness = new Thickness(0), UseAeroCaptionButtons = false });
        var layout = new DockPanel { Background = UiTheme.Background };
        var header = new DockPanel { Height = 42, Background = UiTheme.Panel };
        var close = Button("×", () => dialog.Close()); close.Width = 42; close.Height = 34; close.Padding = new Thickness(0); close.ToolTip = "Cancelar";
        WindowChrome.SetIsHitTestVisibleInChrome(close, true); DockPanel.SetDock(close, Dock.Right); header.Children.Add(close);
        var caption = Text(title, 16, Ink); caption.Margin = new Thickness(18, 0, 0, 0); header.Children.Add(caption); DockPanel.SetDock(header, Dock.Top); layout.Children.Add(header);
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(18, 0, 18, 18) };
        var cancel = Button("Cancelar", () => dialog.DialogResult = false); cancel.IsCancel = true; cancel.IsDefault = true; cancel.MinWidth = 100;
        var accept = Button(title.StartsWith("Eliminar", StringComparison.Ordinal) ? "Eliminar" : "Sí, continuar", () => dialog.DialogResult = true); accept.Margin = new Thickness(10, 0, 0, 0); accept.MinWidth = 100; accept.BorderBrush = UiTheme.Line;
        buttons.Children.Add(cancel); buttons.Children.Add(accept); DockPanel.SetDock(buttons, Dock.Bottom); layout.Children.Add(buttons);
        var text = new TextBlock { Text = message, TextWrapping = TextWrapping.Wrap, FontSize = 15, Foreground = Ink, Margin = new Thickness(22), LineHeight = 23 };
        layout.Children.Add(new ScrollViewer { Content = text, MaxHeight = 350, VerticalScrollBarVisibility = ScrollBarVisibility.Auto });
        dialog.Content = new Border { Child = layout, BorderBrush = UiTheme.Line, BorderThickness = new Thickness(1), Background = UiTheme.Background };
        dialog.Loaded += (_, _) => cancel.Focus(); return dialog;
    }
}
