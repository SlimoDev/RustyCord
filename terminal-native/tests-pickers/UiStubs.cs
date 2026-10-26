using System.Windows.Controls;
using System.Windows.Media;

namespace NotDiscord.Terminal;
public sealed class ServiceException(string message) : Exception(message);
internal static class UiTheme
{
    public static readonly SolidColorBrush Panel = new(Colors.DarkSlateGray), Ink = new(Colors.White), Input = new(Colors.Black), Line = new(Colors.Gray), Muted = new(Colors.LightGray), Surface = new(Colors.DimGray), Hover = new(Colors.DarkGray), Menu = new(Colors.Black);
}
public sealed class UiMediaImage : Image
{
    public event Action<bool>? ImageLoaded;
    public UiMediaImage(string? url, int width) { Loaded += (_, _) => ImageLoaded?.Invoke(false); }
}
