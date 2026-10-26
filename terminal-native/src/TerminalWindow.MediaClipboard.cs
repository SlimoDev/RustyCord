using System.IO;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Media.Imaging;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private bool copyingMediaImage;
    private async void CopyMediaImageAsync(string url)
    {
        if (copyingMediaImage || !MediaImages.IsAllowed(url) && !GifSources.IsPage(url)) return;
        copyingMediaImage = true; status.Text = "Preparando imagen para copiar…";
        using var lifetime = new CancellationTokenSource();
        void Cancel(object? sender, EventArgs args) => lifetime.Cancel();
        Closed += Cancel;
        try
        {
            var image = await MediaImages.GetAsync(url, 1600, lifetime.Token);
            if (lifetime.IsCancellationRequested) return;
            if (image == null) { status.Text = "No se pudo cargar la imagen para copiarla."; return; }
            using var png = new MemoryStream(); var data = ImageClipboard.Create(image, png);
            Clipboard.SetDataObject(data, true); status.Text = "Imagen copiada";
        }
        catch (OperationCanceledException) { }
        catch (COMException) { status.Text = "El portapapeles está ocupado; vuelve a intentarlo."; }
        catch (Exception error) when (error is IOException or NotSupportedException or ArgumentException or InvalidOperationException)
        { status.Text = "No se pudo copiar la imagen."; }
        finally { Closed -= Cancel; copyingMediaImage = false; }
    }
}

internal static class ImageClipboard
{
    // PNG preserves transparency for apps that support it; Bitmap supports older
    // Windows consumers. No clipboard is read and no second full-size decode runs.
    internal static DataObject Create(BitmapSource image, MemoryStream png)
    {
        var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(image)); encoder.Save(png); png.Position = 0;
        var data = new DataObject(); data.SetData(DataFormats.Bitmap, image, false); data.SetData("PNG", png, false); return data;
    }
}
