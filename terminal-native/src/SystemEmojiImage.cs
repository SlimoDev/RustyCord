using System.Runtime.InteropServices;
using System.Collections.Concurrent;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace NotDiscord.Terminal;

/// <summary>Renders the installed Windows emoji font using DirectWrite color support.</summary>
public sealed class SystemEmojiImage : Grid
{
    private readonly string sequence;
    private readonly double size;
    private readonly Image image;
    private readonly TextBlock fallback;
    private CancellationTokenSource? pending;
    public SystemEmojiImage(string sequence, double size)
    {
        this.sequence = sequence; this.size = Math.Clamp(size, 12, 96);
        Width = Height = this.size; VerticalAlignment = VerticalAlignment.Center;
        fallback = new TextBlock { Text = sequence, FontFamily = new FontFamily("Segoe UI Emoji"), FontSize = this.size * .78, Foreground = UiTheme.Ink, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        image = new Image { Stretch = Stretch.Uniform, Width = this.size, Height = this.size };
        Children.Add(fallback); Children.Add(image);
        System.Windows.Automation.AutomationProperties.SetName(this, sequence);
        Loaded += Load;
        Unloaded += (_, _) => { pending?.Cancel(); pending?.Dispose(); pending = null; image.Source = null; fallback.Visibility = Visibility.Visible; };
    }
    private async void Load(object sender, RoutedEventArgs args)
    {
        if (pending != null) return;
        var request = pending = new CancellationTokenSource();
        try
        {
            int pixels = (int)Math.Ceiling(size * VisualTreeHelper.GetDpi(this).DpiScaleX);
            var result = await SystemEmojiRendering.GetAsync(sequence, pixels, request.Token);
            if (!request.IsCancellationRequested && ReferenceEquals(pending, request) && IsLoaded && result != null)
            { image.Source = result; fallback.Visibility = Visibility.Collapsed; }
        }
        catch (OperationCanceledException) { }
    }
}

public static class SystemEmojiRendering
{
    private sealed record Entry(BitmapSource Image, long Bytes);
    private const long Budget = 2 * 1024 * 1024;
    private static readonly object Gate = new();
    private static readonly Dictionary<string, Entry> Cache = new();
    private static readonly LinkedList<string> Order = new();
    private static readonly SemaphoreSlim Rendering = new(1, 1);
    private static readonly Lazy<RenderWorker> Worker = new(() => new());
    private static long used;
    private static bool unavailable;

    public static async Task<BitmapSource?> GetAsync(string sequence, int pixels, CancellationToken cancellation = default)
    {
        if (unavailable || sequence.Length is < 1 or > 64 || sequence.Any(char.IsControl)) return null;
        pixels = Math.Clamp(pixels, 16, 192);
        string key = pixels + "|" + sequence;
        lock (Gate) if (Cache.TryGetValue(key, out var hit)) { Order.Remove(key); Order.AddLast(key); return hit.Image; }
        await Rendering.WaitAsync(cancellation).ConfigureAwait(false);
        try
        {
            lock (Gate) if (Cache.TryGetValue(key, out var hit)) return hit.Image;
            byte[]? data = await Worker.Value.RenderAsync(sequence, pixels, cancellation).ConfigureAwait(false);
            if (data == null) return null;
            cancellation.ThrowIfCancellationRequested();
            var image = BitmapSource.Create(pixels, pixels, 96, 96, PixelFormats.Pbgra32, null, data, pixels * 4); image.Freeze();
            long weight = data.Length;
            lock (Gate)
            {
                while (used + weight > Budget && Order.First != null)
                {
                    string old = Order.First.Value; Order.RemoveFirst(); used -= Cache[old].Bytes; Cache.Remove(old);
                }
                if (!Cache.ContainsKey(key)) { Cache[key] = new(image, weight); Order.AddLast(key); used += weight; }
            }
            return image;
        }
        finally { Rendering.Release(); }
    }
    [DllImport("NotDiscord.Emoji.dll", ExactSpelling = true, CharSet = CharSet.Unicode, CallingConvention = CallingConvention.Cdecl)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.AssemblyDirectory)]
    private static extern int RenderSystemEmoji(string text, int length, int size, [Out] byte[] pixels, int capacity);

    // One lazy background worker owns the native factories. Thread-pool migration
    // must not create one D2D/WIC font context per worker thread over app lifetime.
    private sealed class RenderWorker
    {
        private sealed record Request(string Text, int Pixels, CancellationToken Cancellation, TaskCompletionSource<byte[]?> Completion);
        private readonly BlockingCollection<Request> queue = new(1);
        public RenderWorker()
        {
            var thread = new Thread(Run) { IsBackground = true, Name = "NotDiscord system emoji" };
            thread.SetApartmentState(ApartmentState.MTA); thread.Start();
        }
        public Task<byte[]?> RenderAsync(string text, int pixels, CancellationToken cancellation)
        {
            var completion = new TaskCompletionSource<byte[]?>(TaskCreationOptions.RunContinuationsAsynchronously);
            queue.Add(new(text, pixels, cancellation, completion), cancellation); return completion.Task;
        }
        private void Run()
        {
            foreach (var request in queue.GetConsumingEnumerable())
            {
                if (request.Cancellation.IsCancellationRequested) { request.Completion.TrySetCanceled(request.Cancellation); continue; }
                byte[]? data = null;
                try
                {
                    if (!unavailable)
                    {
                        data = new byte[request.Pixels * request.Pixels * 4];
                        if (RenderSystemEmoji(request.Text, request.Text.Length, request.Pixels, data, data.Length) < 0) data = null;
                    }
                }
                catch (Exception e) when (e is DllNotFoundException or EntryPointNotFoundException or BadImageFormatException or ExternalException)
                { unavailable = true; data = null; }
                request.Completion.TrySetResult(data);
            }
        }
    }
}
