using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using NotDiscord.Terminal;

internal static class UiProgram
{
    [STAThread]
    private static void Main()
    {
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown };
        var bitmap = SystemEmojiRendering.GetAsync("😀", 48).GetAwaiter().GetResult() ?? throw new Exception("Native emoji renderer returned no image.");
        byte[] pixels = new byte[48 * 48 * 4]; bitmap.CopyPixels(pixels, 48 * 4, 0);
        int colored = 0;
        for (int i = 0; i < pixels.Length; i += 4) if (pixels[i + 3] > 0 && (pixels[i] != pixels[i + 1] || pixels[i + 1] != pixels[i + 2])) colored++;
        if (colored < 100) throw new Exception("System emoji did not render in color.");
        Console.WriteLine("System font native color pixels: " + colored);
        var owner = new Window { Width = 1, Height = 1, Left = -12000, Top = -12000, ShowInTaskbar = false, WindowStyle = WindowStyle.None };
        owner.Show();
        string folder = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../evidence"));
        Directory.CreateDirectory(folder);
        string local = Path.Combine(Path.GetTempPath(), "NotDiscord.PickerUi." + Guid.NewGuid().ToString("N"));
        var seed = new PickerHistory(local, "demo");
        foreach (string emoji in new[] { "😀", "❤️", "👍", "😂", "😎", "🎉", "👀", "🔥", "✅" }) seed.Recent(new(emoji, emoji), false);
        seed.Recent(new("<:wave:123>", "wave", "https://cdn.discordapp.com/emojis/123.png"), false);
        int captures = 0;
        var timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(700) };
        timer.Tick += (_, _) =>
        {
            var window = app.Windows.Cast<Window>().FirstOrDefault(w => w != owner && w.IsLoaded);
            if (window == null) return;
            try
            {
                window.UpdateLayout();
                var rows = Descendants(window).OfType<ContentPickerRow>().Count();
                if (window.Title == "Emojis" && rows > 20) throw new Exception("Unicode rows are not virtualized.");
                if (window.Title == "Emojis")
                {
                    var recent = Descendants(window).OfType<StackPanel>().Single(p => p.Name == "RecentEmojiSection");
                    var filters = Descendants(window).OfType<Grid>().Single(p => p.Name == "PickerCategoryFilters");
                    if (recent.TransformToAncestor(window).Transform(new Point()).Y >= filters.TransformToAncestor(window).Transform(new Point()).Y) throw new Exception("Recent emojis must appear above category filters.");
                    var recentRow = Descendants(recent).OfType<ContentPickerRow>().Single();
                    var recentData = (PickerRowModel)recentRow.DataContext;
                    if (recentData.Items.Count != 10 || recentData.Items.Any(c => c.Favorite != null)) throw new Exception("Recent emoji row must show10 items with no favorites action.");
                    if (Descendants(window).OfType<Button>().Any(b => b.ContextMenu != null)) throw new Exception("Emoji favorite context menu still exists.");
                    if (Descendants(window).OfType<ComboBox>().Single().Items.Cast<object>().Any(c => c.ToString()!.Contains("Favoritos"))) throw new Exception("Emoji favorites category still exists.");
                    Console.WriteLine("Emoji UI: recent10 above category filters; no favorites menu, category or action.");
                }
                var image = new RenderTargetBitmap((int)window.ActualWidth, (int)window.ActualHeight, 96, 96, PixelFormats.Pbgra32); image.Render(window);
                var png = new PngBitmapEncoder(); png.Frames.Add(BitmapFrame.Create(image));
                using var output = File.Create(Path.Combine(folder, window.Title == "Emojis" ? "picker-emoji-preview.png" : "picker-gif-preview.png")); png.Save(output);
                Console.WriteLine(window.Title + ": " + rows + " realized rows, screenshot saved; synthetic data only."); captures++;
            }
            finally { window.Close(); }
        };
        timer.Start();
        var demo = new AppSnapshot { Demo = true, CurrentUser = new("demo", "Demo"), Guilds = new[] { new NavItem("123", "Servidor de ejemplo", "guild") } };
        ContentPicker.ShowEmoji(owner, demo, (_, _) => Task.FromResult<IReadOnlyList<CustomEmoji>>(Array.Empty<CustomEmoji>()), _ => { }, local);
        ContentPicker.ShowGif(owner, demo, (_, _) => Task.FromResult(new GifPage(new[] { new GifItem("1", "https://tenor.com/view/demo-123", "", "GIF de ejemplo"), new GifItem("2", "https://tenor.com/view/demo-124", "", "Otro GIF") })), _ => { }, local);
        timer.Stop(); owner.Close(); app.Shutdown();
        if (Directory.Exists(local)) Directory.Delete(local, true);
        if (captures != 2) throw new Exception("Missing picker preview.");
    }
    private static IEnumerable<DependencyObject> Descendants(DependencyObject parent)
    {
        for (int i = 0; i < VisualTreeHelper.GetChildrenCount(parent); i++)
        {
            var child = VisualTreeHelper.GetChild(parent, i); yield return child;
            foreach (var descendant in Descendants(child)) yield return descendant;
        }
    }
}
