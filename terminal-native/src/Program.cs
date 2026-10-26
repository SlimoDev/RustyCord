using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Threading;

namespace NotDiscord.Terminal;

internal static class Program
{
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] private static extern int SetCurrentProcessExplicitAppUserModelID(string appId);
    [STAThread]
    public static int Main(string[] args)
    {
        SetCurrentProcessExplicitAppUserModelID("com.notdiscord.Terminal.Native");
        string root = FindRoot();
        string data = Path.Combine(root, "data"); Directory.CreateDirectory(data);
        var app = new Application { ShutdownMode = ShutdownMode.OnMainWindowClose };
        var window = new TerminalWindow();
        app.MainWindow = window;
        bool demo = args.Contains("--demo", StringComparer.Ordinal);
        if (!demo)
        {
            try { NotificationIdentity.EnsureRegistered(Environment.ProcessPath!, Path.Combine(AppContext.BaseDirectory, "discord.ico")); }
            catch (Exception error) when (error is IOException or UnauthorizedAccessException or System.Runtime.InteropServices.COMException or System.ComponentModel.Win32Exception) { }
        }
        string? preview = GetArgument(args, "--preview");
        bool diagnostics = args.Contains("--diagnostics", StringComparer.Ordinal);
        DispatcherTimer? diagnosticsTimer = null;
        bool samplingMedia = false;
        Controller? controller = null;
        bool closed = false;
        app.DispatcherUnhandledException += (_, e) =>
        {
            // No exception dumps: native message state and credentials can appear in them.
            e.Handled = true;
            MessageBox.Show(window, "La operación no pudo completarse. La versión es experimental; no se guardaron datos privados en un informe de error.", "NotDiscord Terminal", MessageBoxButton.OK, MessageBoxImage.Warning);
        };
        window.Loaded += async (_, _) =>
        {
            if (demo) window.Apply(DemoState());
            else
            {
                controller = new Controller(window, data);
                if (diagnostics)
                {
                    Directory.CreateDirectory(Path.Combine(root, "evidence"));
                    diagnosticsTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(3) };
                    diagnosticsTimer.Tick += async (_, _) =>
                    {
                        try { File.WriteAllText(Path.Combine(root, "evidence", "readiness.json"), System.Text.Json.JsonSerializer.Serialize(controller.Readiness())); } catch (IOException) { }
                        string request = Path.Combine(root, "evidence", "media-stats.request");
                        if (samplingMedia || !File.Exists(request)) return;
                        samplingMedia = true;
                        try
                        {
                            File.Delete(request);
                            var statistics = await controller.MediaStatisticsAsync();
                            File.WriteAllText(Path.Combine(root, "evidence", "media-stats.json"), System.Text.Json.JsonSerializer.Serialize(statistics));
                        }
                        catch (Exception error) when (error is IOException or ServiceException or OperationCanceledException)
                        {
                            try { File.WriteAllText(Path.Combine(root, "evidence", "media-stats.json"), "{\"sanitized\":true,\"available\":false}"); } catch (IOException) { }
                        }
                        finally { samplingMedia = false; }
                    };
                    diagnosticsTimer.Start();
                }
                await controller.InitializeAsync();
            }
            if (preview != null)
            {
                await Task.Delay(700);
                await window.Dispatcher.InvokeAsync(() => { window.SavePreview(preview); window.Close(); }, DispatcherPriority.ApplicationIdle);
            }
        };
        window.Closing += async (_, e) =>
        {
            if (closed || controller == null) return;
            e.Cancel = true; closed = true;
            diagnosticsTimer?.Stop();
            await controller.DisposeAsync();
            window.Close();
        };
        return app.Run(window);
    }
    private static string? GetArgument(string[] args, string name)
    {
        int i = Array.IndexOf(args, name); return i >= 0 && i + 1 < args.Length ? args[i + 1] : null;
    }
    private static string FindRoot()
    {
        var directory = new DirectoryInfo(AppContext.BaseDirectory);
        while (directory != null)
        {
            if (directory.Name.Equals("terminal-native", StringComparison.OrdinalIgnoreCase)) return directory.FullName;
            directory = directory.Parent;
        }
        return AppContext.BaseDirectory;
    }
    private static AppSnapshot DemoState()
    {
        var me = new Person("100", "tú"); var ana = new Person("101", "ana"); var leo = new Person("102", "leo");
        return new AppSnapshot
        {
            Demo = true, SignedIn = false, CurrentUser = me, View = "chat", Connection = "DEMO SIN CONEXIÓN",
            Status = "Datos ficticios para verificar el diseño · no es una prueba de ahorro con Discord",
            SelectedGuildId = "10", SelectedChannelId = "20", ChannelName = "general",
            Guilds = new[] { new NavItem("10", "Amigos", "guild"), new NavItem("11", "Juegos", "guild"), new NavItem("12", "Desarrollo", "guild") },
            Channels = new[] { new NavItem("19", "CONVERSACIONES", "category"), new NavItem("20", "general", "text"), new NavItem("21", "proyectos", "text"), new NavItem("22", "Sala de voz", "voice") },
            DirectMessages = new[] { new NavItem("23", "ana", "dm"), new NavItem("24", "leo", "dm") },
            Relationships = new[] { new Relationship(ana, 1), new Relationship(leo, 3) },
            Messages = new[]
            {
                new ChatMessage("1001", "20", ana, "La interfaz ya se dibuja con controles nativos de Windows. El chat no necesita un navegador.", DateTimeOffset.Now.AddMinutes(-4), Array.Empty<ChatAttachment>()),
                new ChatMessage("1002", "20", leo, "Los mensajes se cargan por páginas y la caché tiene un límite. Las imágenes sólo se solicitan cuando aparecen en pantalla.", DateTimeOffset.Now.AddMinutes(-3), Array.Empty<ChatAttachment>()),
                new ChatMessage("1003", "20", me, "FakeNitro, enlaces limpios y privados fijados están integrados en el código propio. Esta conversación es una demostración local.", DateTimeOffset.Now.AddMinutes(-2), Array.Empty<ChatAttachment>()),
                new ChatMessage("1004", "20", ana, "Solicitudes e invitaciones siguen dentro del alcance. Add Server, Discover y stickers quedan fuera.", DateTimeOffset.Now.AddMinutes(-1), Array.Empty<ChatAttachment>())
            }
        };
    }
}
