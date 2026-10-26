using System.IO;
using System.Net.Http;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace NotDiscord.Terminal;

/// <summary>On-demand native GIF playback; one active animation, bounded frames and no idle timer.</summary>
public sealed class UiGifImage : StackPanel
{
    internal sealed record GifFrame(BitmapSource Image, int DelayMs);
    internal sealed record GifData(IReadOnlyList<GifFrame> Frames);
    private static WeakReference<UiGifImage>? current;
    private static Action? externalPlaybackStop;
    internal static void ClaimExternalPlayback(Action stop)
    {
        if (current != null && current.TryGetTarget(out var active)) active.Stop();
        var previous = externalPlaybackStop; externalPlaybackStop = null; previous?.Invoke(); externalPlaybackStop = stop;
    }
    internal static void ReleaseExternalPlayback(Action stop) { if (ReferenceEquals(externalPlaybackStop, stop)) externalPlaybackStop = null; }
    private static bool playbackEnabled = true;
    public static void ConfigurePlayback(bool enabled)
    {
        playbackEnabled = enabled;
        if (!enabled && current != null && current.TryGetTarget(out var active)) active.Stop();
        if (!enabled) { var stop = externalPlaybackStop; externalPlaybackStop = null; stop?.Invoke(); }
    }
    private readonly string url;
    private readonly Image image;
    private readonly UiMediaImage poster;
    private readonly TextBlock loading;
    private readonly Button play;
    private readonly Action<UiAction> action;
    private readonly DispatcherTimer timer;
    private CancellationTokenSource? pending;
    private GifData? animation;
    private int index;
    private int generation;

    public UiGifImage(string url, Action<UiAction> action)
    {
        this.url = url; this.action = action;
        HorizontalAlignment = HorizontalAlignment.Left; Margin = new Thickness(0, 8, 0, 0);
        ContextMenu = TerminalWindow.MediaMenu(url, action);
        var viewport = new Grid { MaxWidth = 360, MaxHeight = 220, MinHeight = 40, HorizontalAlignment = HorizontalAlignment.Left };
        poster = new UiMediaImage(url, 384) { MaxWidth = 360, MaxHeight = 220, Stretch = Stretch.Uniform };
        image = new Image { MaxWidth = 360, MaxHeight = 220, Stretch = Stretch.Uniform, Visibility = Visibility.Collapsed };
        loading = new TextBlock { Text = "Cargando vista previa…", Foreground = TerminalWindow.Muted, FontSize = 12, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(4), VerticalAlignment = VerticalAlignment.Center };
        poster.ImageLoaded += ok => { loading.Text = ok ? "" : "Vista previa no disponible · pulsa Reproducir"; loading.Visibility = ok || animation != null ? Visibility.Collapsed : Visibility.Visible; };
        viewport.Children.Add(poster); viewport.Children.Add(image); viewport.Children.Add(loading); Children.Add(viewport);
        var toolbar = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 4, 0, 0) };
        play = TerminalWindow.Button("▶ GIF", Toggle); play.Padding = new Thickness(8, 3, 8, 3); play.FontSize = 11;
        toolbar.Children.Add(play);
        var open = TerminalWindow.Button("Abrir original", () => action(new("link-local", Text: url))); open.Padding = new Thickness(8, 3, 8, 3); open.FontSize = 11; open.Margin = new Thickness(5, 0, 0, 0); toolbar.Children.Add(open); Children.Add(toolbar);
        timer = new DispatcherTimer(DispatcherPriority.Background); timer.Tick += Tick;
        Unloaded += (_, _) => Stop();
        IsVisibleChanged += (_, _) => { if (!IsVisible) Stop(); };
    }

    private async void Toggle()
    {
        if (!playbackEnabled) { action(new("status-local", Text: "La reproducción de GIF está desactivada en Ajustes.")); return; }
        if (animation != null || pending != null) { Stop(); return; }
        var external = externalPlaybackStop; externalPlaybackStop = null; external?.Invoke();
        if (current != null && current.TryGetTarget(out var previous) && previous != this) previous.Stop();
        current = new(this);
        int version = ++generation;
        using var request = new CancellationTokenSource(); pending = request; play.Content = "Cargando… · cancelar";
        try
        {
            var data = await MediaImages.GetGifBytesAsync(url, request.Token);
            if (data == null) throw new InvalidDataException("GIF no disponible o mayor de 12 MiB.");
            var decoded = await Task.Run(() => Decode(data, request.Token), request.Token);
            if (version != generation || request.IsCancellationRequested || !IsLoaded || !WithinViewport()) return;
            animation = decoded; index = 0; image.Source = decoded.Frames[0].Image; image.Visibility = Visibility.Visible; poster.Visibility = Visibility.Collapsed; loading.Visibility = Visibility.Collapsed;
            play.Content = "Ⅱ Pausar GIF"; timer.Interval = TimeSpan.FromMilliseconds(decoded.Frames[0].DelayMs); timer.Start();
        }
        catch (OperationCanceledException) { }
        catch (Exception ex) when (ex is InvalidDataException or HttpRequestException or IOException or NotSupportedException or ArgumentException or InvalidOperationException or System.IO.FileFormatException or System.Runtime.InteropServices.COMException)
        {
            if (version == generation)
            {
                action(new("status-local", Text: "No se pudo cargar este GIF: puede no estar disponible o superar los límites de tamaño. Puedes reintentar o abrir el original."));
                play.Content = "▶ GIF · reintentar";
            }
        }
        finally
        {
            if (ReferenceEquals(pending, request)) pending = null;
            if (version == generation && animation == null && Equals(play.Content, "Cargando… · cancelar")) play.Content = "▶ GIF";
        }
    }
    private void Tick(object? sender, EventArgs args)
    {
        if (animation == null || !IsLoaded || !IsVisible || Window.GetWindow(this)?.WindowState == WindowState.Minimized || !WithinViewport()) { Stop(); return; }
        index = (index + 1) % animation.Frames.Count;
        var frame = animation.Frames[index]; image.Source = frame.Image; timer.Interval = TimeSpan.FromMilliseconds(frame.DelayMs);
    }
    private void Stop()
    {
        ++generation; timer.Stop(); pending?.Cancel(); pending = null; animation = null; image.Source = null;
        image.Visibility = Visibility.Collapsed; poster.Visibility = Visibility.Visible; play.Content = "▶ GIF";
        loading.Visibility = poster.Source == null ? Visibility.Visible : Visibility.Collapsed;
        if (current != null && current.TryGetTarget(out var target) && ReferenceEquals(target, this)) current = null;
    }
    private bool WithinViewport()
    {
        DependencyObject? parent = this;
        while (parent != null && parent is not ScrollViewer) parent = VisualTreeHelper.GetParent(parent);
        if (parent is not ScrollViewer viewport) return IsVisible;
        try { return TransformToAncestor(viewport).TransformBounds(new Rect(RenderSize)).IntersectsWith(new Rect(viewport.RenderSize)); }
        catch (InvalidOperationException) { return false; }
    }

    internal static GifData Decode(byte[] bytes, CancellationToken cancellation)
    {
        using var stream = new MemoryStream(bytes, false);
        var decoder = new GifBitmapDecoder(stream, BitmapCreateOptions.PreservePixelFormat, BitmapCacheOption.OnDemand);
        if (decoder.Frames.Count is < 1 or > 400) throw new InvalidDataException();
        int width = Number(decoder.Metadata as BitmapMetadata, "/logscrdesc/Width", decoder.Frames[0].PixelWidth);
        int height = Number(decoder.Metadata as BitmapMetadata, "/logscrdesc/Height", decoder.Frames[0].PixelHeight);
        if (width is < 1 or > 1024 || height is < 1 or > 1024 || (long)width * height > 1_048_576) throw new InvalidDataException();
        if ((long)width * height * decoder.Frames.Count > 64_000_000) throw new InvalidDataException();
        // Keep the same decoded memory budget; longer GIFs use smaller thumbnails
        // instead of failing merely because they contain more than 120 frames.
        double memoryScale = Math.Sqrt((8d * 1024 * 1024) / (4d * width * height * decoder.Frames.Count));
        double scale = Math.Min(memoryScale, Math.Min(1d, Math.Min(360d / width, 220d / height)));
        int targetWidth = Math.Max(1, (int)(width * scale)), targetHeight = Math.Max(1, (int)(height * scale));
        if ((long)targetWidth * targetHeight * 4 * decoder.Frames.Count > 8 * 1024 * 1024) throw new InvalidDataException();
        byte[] canvas = new byte[checked(width * height * 4)];
        var frames = new List<GifFrame>(decoder.Frames.Count);
        byte[]? restore = null;
        int previousDisposal = 0, previousLeft = 0, previousTop = 0, previousWidth = 0, previousHeight = 0;
        foreach (var frame in decoder.Frames)
        {
            cancellation.ThrowIfCancellationRequested();
            if (previousDisposal == 2)
            {
                for (int y = previousTop; y < Math.Min(height, previousTop + previousHeight); y++)
                    System.Array.Clear(canvas, (y * width + previousLeft) * 4, Math.Max(0, Math.Min(previousWidth, width - previousLeft)) * 4);
            }
            else if (previousDisposal == 3 && restore != null) Buffer.BlockCopy(restore, 0, canvas, 0, canvas.Length);
            var metadata = frame.Metadata as BitmapMetadata;
            int left = Number(metadata, "/imgdesc/Left", 0), top = Number(metadata, "/imgdesc/Top", 0);
            int disposal = Number(metadata, "/grctlext/Disposal", 0);
            if (left < 0 || top < 0 || left >= width || top >= height || frame.PixelWidth > 1024 || frame.PixelHeight > 1024) throw new InvalidDataException();
            restore = disposal == 3 ? (byte[])canvas.Clone() : null;
            var converted = new FormatConvertedBitmap(frame, PixelFormats.Bgra32, null, 0);
            int frameWidth = converted.PixelWidth, frameHeight = converted.PixelHeight;
            byte[] pixels = new byte[checked(frameWidth * frameHeight * 4)]; converted.CopyPixels(pixels, frameWidth * 4, 0);
            for (int y = 0; y < frameHeight && top + y < height; y++)
            {
                int source = y * frameWidth * 4, destination = ((top + y) * width + left) * 4;
                for (int x = 0; x < frameWidth && left + x < width; x++, source += 4, destination += 4)
                {
                    if (pixels[source + 3] == 0) continue;
                    canvas[destination] = pixels[source]; canvas[destination + 1] = pixels[source + 1]; canvas[destination + 2] = pixels[source + 2]; canvas[destination + 3] = pixels[source + 3];
                }
            }
            byte[] thumbnail = new byte[targetWidth * targetHeight * 4];
            for (int y = 0; y < targetHeight; y++)
            {
                int sourceY = y * height / targetHeight;
                for (int x = 0; x < targetWidth; x++)
                {
                    int src = (sourceY * width + x * width / targetWidth) * 4, dst = (y * targetWidth + x) * 4;
                    thumbnail[dst] = canvas[src]; thumbnail[dst + 1] = canvas[src + 1]; thumbnail[dst + 2] = canvas[src + 2]; thumbnail[dst + 3] = canvas[src + 3];
                }
            }
            var bitmap = BitmapSource.Create(targetWidth, targetHeight, 96, 96, PixelFormats.Bgra32, null, thumbnail, targetWidth * 4); bitmap.Freeze();
            int delay = Math.Clamp(Number(metadata, "/grctlext/Delay", 10) * 10, 20, 10_000);
            frames.Add(new(bitmap, delay));
            previousDisposal = disposal; previousLeft = left; previousTop = top; previousWidth = frameWidth; previousHeight = frameHeight;
        }
        return new(frames);
    }
    private static int Number(BitmapMetadata? metadata, string query, int fallback)
    {
        try { return metadata?.ContainsQuery(query) == true ? Convert.ToInt32(metadata.GetQuery(query)) : fallback; }
        catch (Exception ex) when (ex is NotSupportedException or InvalidOperationException or FormatException or OverflowException or InvalidCastException or System.Runtime.InteropServices.COMException) { return fallback; }
    }
}
