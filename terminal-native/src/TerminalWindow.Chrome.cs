using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Shell;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private static readonly Lazy<ImageSource?> DiscordImage = new(() =>
    {
        try
        {
            var image = BitmapFrame.Create(new Uri("pack://application:,,,/NotDiscord.Terminal;component/assets/discord.ico"));
            image.Freeze(); return image;
        }
        catch (Exception error) when (error is System.IO.IOException or InvalidOperationException or NotSupportedException) { return null; }
    });
    internal static Image DiscordLogo(double size) => new() { Source = DiscordImage.Value, Width = size, Height = size, Stretch = Stretch.Uniform };

    private UIElement BuildWindowChrome(UIElement body)
    {
        WindowStyle = WindowStyle.None;
        ResizeMode = ResizeMode.CanResize;
        Icon = DiscordImage.Value;
        WindowChrome.SetWindowChrome(this, new WindowChrome { CaptionHeight = 32, ResizeBorderThickness = new Thickness(7), GlassFrameThickness = new Thickness(0), CornerRadius = new CornerRadius(0), UseAeroCaptionButtons = false });
        var frame = new Grid { Background = bg };
        frame.RowDefinitions.Add(new() { Height = new GridLength(32) });
        frame.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        var caption = new DockPanel { Background = bg, LastChildFill = true };
        var controls = new StackPanel { Orientation = Orientation.Horizontal };
        Button CaptionButton(string glyph, string name, Action action)
        {
            var button = Button(glyph, action); button.FontFamily = new FontFamily("Segoe MDL2 Assets"); button.FontSize = 10;
            button.Width = 46; button.Height = 32; button.Padding = new Thickness(0); button.Background = Brushes.Transparent; button.ToolTip = name;
            AutomationProperties.SetName(button, name); WindowChrome.SetIsHitTestVisibleInChrome(button, true); controls.Children.Add(button); return button;
        }
        CaptionButton("\uE921", "Minimizar", () => SystemCommands.MinimizeWindow(this));
        var maximize = CaptionButton("\uE922", "Maximizar", () => { if (WindowState == WindowState.Maximized) SystemCommands.RestoreWindow(this); else SystemCommands.MaximizeWindow(this); });
        CaptionButton("\uE8BB", "Cerrar", () => SystemCommands.CloseWindow(this));
        DockPanel.SetDock(controls, Dock.Right); caption.Children.Add(controls);
        var logo = DiscordLogo(18); logo.Margin = new Thickness(10, 0, 8, 0); DockPanel.SetDock(logo, Dock.Left); caption.Children.Add(logo);
        var title = new TextBlock { Foreground = Ink, FontSize = 12, VerticalAlignment = VerticalAlignment.Center, IsHitTestVisible = false, TextTrimming = TextTrimming.CharacterEllipsis };
        title.SetBinding(TextBlock.TextProperty, new Binding(nameof(Title)) { Source = this }); caption.Children.Add(title);
        frame.Children.Add(caption); Grid.SetRow(body, 1); frame.Children.Add(body);
        StateChanged += (_, _) =>
        {
            bool maximized = WindowState == WindowState.Maximized;
            maximize.Content = maximized ? "\uE923" : "\uE922";
            maximize.ToolTip = maximized ? "Restaurar" : "Maximizar";
            AutomationProperties.SetName(maximize, maximize.ToolTip.ToString());
            frame.Margin = maximized ? new Thickness(7) : new Thickness(0);
        };
        return frame;
    }
}
