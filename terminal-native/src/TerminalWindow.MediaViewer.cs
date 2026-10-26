using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Microsoft.Win32;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private Window? mediaViewerWindow;
    private async void ShowMediaViewerAsync(string url)
    {
        if (!MediaImages.IsAllowed(url) && !GifSources.IsPage(url)) { status.Text = "No hay un visor compatible para ese origen."; return; }
        mediaViewerWindow?.Close();
        var dialog = Dialog("Imagen / GIF", Math.Min(1040, SystemParameters.WorkArea.Width - 80), Math.Min(800, SystemParameters.WorkArea.Height - 80));
        mediaViewerWindow = dialog;
        var lifetime = new CancellationTokenSource();
        var root = new DockPanel();
        var controls = new WrapPanel { Margin = new Thickness(10) };
        var zoomLabel = Text("Cargando…", 13, Muted); zoomLabel.Margin = new Thickness(12, 0, 6, 0);
        var image = new Image { Stretch = Stretch.Fill, HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top };
        var canvas = new Grid { Background = Brushes.Transparent };
        var scroll = new ScrollViewer { Content = canvas, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, CanContentScroll = false, Background = UiTheme.Rail };
        canvas.Children.Add(image);
        double naturalWidth = 1, naturalHeight = 1, scale = 1, fit = 1;
        BitmapSource? poster = null;
        UiGifImage.GifData? animation = null;
        var gifTimer = new DispatcherTimer(DispatcherPriority.Background); int frameIndex = 0, playbackVersion = 0;
        CancellationTokenSource? gifWork = null;
        Button? gifButton = null; Action stopGif = null!;
        stopGif = () => { ++playbackVersion; gifWork?.Cancel(); gifWork = null; gifTimer.Stop(); animation = null; image.Source = poster; if (gifButton != null) gifButton.Content = "▶ Reproducir GIF"; UiGifImage.ReleaseExternalPlayback(stopGif); };
        gifTimer.Tick += (_, _) =>
        {
            if (animation == null || !dialog.IsVisible || dialog.WindowState == WindowState.Minimized) { stopGif(); return; }
            frameIndex = (frameIndex + 1) % animation.Frames.Count; var frame = animation.Frames[frameIndex]; image.Source = frame.Image; gifTimer.Interval = TimeSpan.FromMilliseconds(frame.DelayMs);
        };
        async void ToggleGif()
        {
            if (!preferences.AllowGifPlayback) { zoomLabel.Text = "GIF desactivado en Ajustes"; return; }
            if (animation != null || gifWork != null) { stopGif(); return; }
            UiGifImage.ClaimExternalPlayback(stopGif); int version = ++playbackVersion;
            using var work = CancellationTokenSource.CreateLinkedTokenSource(lifetime.Token); gifWork = work;
            gifButton!.Content = "Cargando… · cancelar";
            try
            {
                var bytes = await MediaImages.GetGifBytesAsync(url, work.Token);
                if (bytes == null) throw new IOException("GIF no disponible o mayor de 12 MiB.");
                var frames = await Task.Run(() => UiGifImage.Decode(bytes, work.Token), work.Token);
                if (version != playbackVersion || work.IsCancellationRequested || !dialog.IsVisible) return;
                animation = frames; frameIndex = 0; image.Source = frames.Frames[0].Image;
                gifButton.Content = "Ⅱ Pausar GIF"; gifTimer.Interval = TimeSpan.FromMilliseconds(frames.Frames[0].DelayMs); gifTimer.Start();
            }
            catch (OperationCanceledException) { }
            catch (Exception error) when (error is IOException or System.Net.Http.HttpRequestException or NotSupportedException or FileFormatException or ArgumentException or InvalidOperationException or System.Runtime.InteropServices.COMException)
            { if (version == playbackVersion) { zoomLabel.Text = "No se pudo reproducir este GIF"; stopGif(); } }
            finally { if (ReferenceEquals(gifWork, work)) gifWork = null; }
        }
        void ApplyScale(double requested, Point? anchor = null)
        {
            double previous = scale; scale = Math.Clamp(requested, Math.Min(.05, fit), 8);
            double ox = scroll.HorizontalOffset, oy = scroll.VerticalOffset;
            image.Width = naturalWidth * scale; image.Height = naturalHeight * scale;
            canvas.Width = Math.Max(image.Width, Math.Max(1, scroll.ViewportWidth)); canvas.Height = Math.Max(image.Height, Math.Max(1, scroll.ViewportHeight));
            image.Margin = new Thickness(Math.Max(0, (canvas.Width - image.Width) / 2), Math.Max(0, (canvas.Height - image.Height) / 2), 0, 0);
            zoomLabel.Text = $"{scale:P0}";
            if (anchor.HasValue && previous > 0)
            { double ratio = scale / previous; scroll.UpdateLayout(); scroll.ScrollToHorizontalOffset(Math.Clamp((ox + anchor.Value.X) * ratio - anchor.Value.X, 0, scroll.ScrollableWidth)); scroll.ScrollToVerticalOffset(Math.Clamp((oy + anchor.Value.Y) * ratio - anchor.Value.Y, 0, scroll.ScrollableHeight)); }
        }
        void Fit()
        { fit = Math.Min(1, Math.Min(Math.Max(1, scroll.ViewportWidth - 20) / naturalWidth, Math.Max(1, scroll.ViewportHeight - 20) / naturalHeight)); ApplyScale(fit); scroll.ScrollToHorizontalOffset(0); scroll.ScrollToVerticalOffset(0); }
        controls.Children.Add(Button("−", () => ApplyScale(scale / 1.2))); controls.Children.Add(Button("+", () => ApplyScale(scale * 1.2)));
        controls.Children.Add(Button("Ajustar", Fit)); controls.Children.Add(Button("100 %", () => ApplyScale(1)));
        if (MediaImages.IsGifSource(url)) { gifButton = Button("▶ Reproducir GIF", ToggleGif); controls.Children.Add(gifButton); }
        controls.Children.Add(Button("Guardar…", () => SaveMediaAsync(url))); controls.Children.Add(Button("Copiar imagen", () => CopyMediaImageAsync(url))); controls.Children.Add(Button("Copiar enlace", () => CopyMediaLink(url))); controls.Children.Add(Button("Abrir original", () => OpenLink(url))); controls.Children.Add(zoomLabel);
        DockPanel.SetDock(controls, Dock.Top); root.Children.Add(controls); root.Children.Add(scroll); dialog.Content = root;
        scroll.PreviewMouseWheel += (_, e) => { ApplyScale(scale * (e.Delta > 0 ? 1.2 : 1 / 1.2), e.GetPosition(scroll)); e.Handled = true; };
        Point? drag = null; double startX = 0, startY = 0;
        scroll.PreviewMouseLeftButtonDown += (_, e) =>
        { if (scroll.ScrollableWidth <= 0 && scroll.ScrollableHeight <= 0) return; drag = e.GetPosition(scroll); startX = scroll.HorizontalOffset; startY = scroll.VerticalOffset; scroll.CaptureMouse(); scroll.Cursor = Cursors.SizeAll; e.Handled = true; };
        scroll.PreviewMouseMove += (_, e) =>
        { if (!drag.HasValue || e.LeftButton != MouseButtonState.Pressed) return; Point now = e.GetPosition(scroll); scroll.ScrollToHorizontalOffset(Math.Clamp(startX + drag.Value.X - now.X, 0, scroll.ScrollableWidth)); scroll.ScrollToVerticalOffset(Math.Clamp(startY + drag.Value.Y - now.Y, 0, scroll.ScrollableHeight)); e.Handled = true; };
        void EndDrag() { drag = null; scroll.ReleaseMouseCapture(); scroll.Cursor = Cursors.Arrow; }
        scroll.PreviewMouseLeftButtonUp += (_, _) => EndDrag(); scroll.LostMouseCapture += (_, _) => { drag = null; scroll.Cursor = Cursors.Arrow; };
        var menu = UiMenus.Create(); UiMenus.Add(menu, "Copiar imagen", () => CopyMediaImageAsync(url)); UiMenus.Add(menu, "Guardar imagen / GIF…", () => SaveMediaAsync(url)); UiMenus.Add(menu, "Copiar enlace", () => CopyMediaLink(url)); image.ContextMenu = menu;
        dialog.Closed += (_, _) => { lifetime.Cancel(); stopGif(); image.Source = null; lifetime.Dispose(); if (ReferenceEquals(mediaViewerWindow, dialog)) mediaViewerWindow = null; };
        dialog.Show();
        var result = await MediaImages.GetAsync(url, 1600, lifetime.Token);
        if (lifetime.IsCancellationRequested || !dialog.IsVisible) return;
        if (result == null) { zoomLabel.Text = "No se pudo cargar"; return; }
        poster = result; if (animation == null) image.Source = result; naturalWidth = result.PixelWidth; naturalHeight = result.PixelHeight; scroll.UpdateLayout(); Fit();
    }
    private async void SaveMediaAsync(string url)
    {
        if (!MediaImages.IsAllowed(url) && !GifSources.IsPage(url)) return;
        string name = "imagen";
        if (Uri.TryCreate(url, UriKind.Absolute, out var uri)) { string possible = Path.GetFileName(Uri.UnescapeDataString(uri.AbsolutePath)); if (possible.Length is > 0 and < 150 && possible.IndexOfAny(Path.GetInvalidFileNameChars()) < 0) name = possible; }
        if (GifSources.IsPage(url)) name = "animacion.gif";
        else if (!Path.HasExtension(name)) name += ".png";
        var picker = new SaveFileDialog { Title = "Guardar imagen / GIF", FileName = name, Filter = "Imágenes y GIF|*.png;*.jpg;*.jpeg;*.gif;*.webp;*.bmp|Todos los archivos|*.*", OverwritePrompt = true };
        if (picker.ShowDialog(this) != true) return;
        try { await MediaDownloads.SaveAsync(url, picker.FileName); status.Text = "Imagen / GIF guardado"; }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or System.Net.Http.HttpRequestException or OperationCanceledException) { status.Text = "No se pudo guardar la imagen. " + error.Message; }
    }
    private void CopyMediaLink(string url) { try { Clipboard.SetText(url); status.Text = "Enlace copiado"; } catch (System.Runtime.InteropServices.COMException) { status.Text = "El portapapeles está ocupado; vuelve a intentarlo."; } }
}
