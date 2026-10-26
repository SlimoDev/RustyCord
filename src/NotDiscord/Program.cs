using System.Diagnostics;
using System.Security.Cryptography;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Microsoft.Web.WebView2.Core;
using Microsoft.Web.WebView2.WinForms;

namespace NotDiscord;

internal static class Program
{
    [STAThread]
    static void Main(string[] args)
    {
        ApplicationConfiguration.Initialize();
        try
        {
            var paths = new AppPaths(AppContext.BaseDirectory);
            if (args.Contains("--self-test")) { Environment.ExitCode = SelfTests.Run(paths); return; }
            bool benchmark = args.Contains("--benchmark-smoke");
            bool smoke = args.Contains("--smoke-test") || args.Contains("--long-smoke") || benchmark;
            int waitIndex = Array.IndexOf(args, "--wait-for-parent");
            if (waitIndex >= 0 && waitIndex + 1 < args.Length && int.TryParse(args[waitIndex + 1], out int parentId) && parentId != Environment.ProcessId)
            {
                try { using var parent = Process.GetProcessById(parentId); parent.WaitForExit(8000); } catch { }
            }
            string key = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(paths.Root))).Substring(0, 20);
            using var mutex = new Mutex(true, "Local\\NotDiscord-" + key + (smoke ? "-smoke" : ""), out bool first);
            if (!first)
            {
                if (!smoke) MessageBox.Show("NotDiscord ya está abierto. Revisa también la bandeja.", "NotDiscord");
                Environment.ExitCode = 2; return;
            }
            Application.Run(new MainWindow(paths, smoke, benchmark, args.Contains("--baseline-engine"), args.Contains("--measure-session"), args.Contains("--long-smoke")));
        }
        catch (Exception ex)
        {
            Environment.ExitCode = 1;
            if (!args.Contains("--smoke-test") && !args.Contains("--benchmark-smoke") && !args.Contains("--long-smoke")) MessageBox.Show("No se pudo iniciar NotDiscord: " + ex.GetType().Name, "NotDiscord", MessageBoxButtons.OK, MessageBoxIcon.Error);
        }
    }
}

internal sealed class AppPaths
{
    public string Root { get; }
    public AppPaths(string start)
    {
        var directory = new DirectoryInfo(Path.GetFullPath(start));
        for (int depth = 0; directory != null && depth < 8; depth++, directory = directory.Parent)
        {
            if (File.Exists(Path.Combine(directory.FullName, ".notdiscord-root")))
            {
                Root = directory.FullName;
                foreach (string name in new[] { "data", "config", "evidence" }) Directory.CreateDirectory(Inside(name));
                return;
            }
        }
        throw new DirectoryNotFoundException("Conserva bin junto al marcador .notdiscord-root.");
    }
    public string Inside(string relative)
    {
        if (Path.IsPathRooted(relative)) throw new ArgumentException("Ruta absoluta no permitida.");
        string resolved = Path.GetFullPath(Path.Combine(Root, relative));
        if (!resolved.StartsWith(Root.TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
            throw new ArgumentException("La ruta sale de NotDiscord.");
        return resolved;
    }
    public void WriteJson(string relative, object value) => File.WriteAllText(Inside(relative), JsonSerializer.Serialize(value, new JsonSerializerOptions { WriteIndented = true }));
}

internal static class NavigationPolicy
{
    public static bool IsHttp(string? value) => Uri.TryCreate(value, UriKind.Absolute, out var uri)
        && (uri.Scheme == Uri.UriSchemeHttps || uri.Scheme == Uri.UriSchemeHttp)
        && string.IsNullOrEmpty(uri.UserInfo) && !string.IsNullOrEmpty(uri.Host);
    public static bool IsDiscord(string? value) => IsHttp(value) && Uri.TryCreate(value, UriKind.Absolute, out var uri)
        && uri.Scheme == "https" && uri.Host.Equals("discord.com", StringComparison.OrdinalIgnoreCase) && uri.Port == 443;
    public static bool CanOpenExternal(string? value, bool userInitiated) => userInitiated && IsHttp(value);
    public static bool IsRestorableChannel(string? value) => IsDiscord(value) && Uri.TryCreate(value, UriKind.Absolute, out var uri)
        && string.IsNullOrEmpty(uri.Query) && string.IsNullOrEmpty(uri.Fragment)
        && System.Text.RegularExpressions.Regex.IsMatch(uri.AbsolutePath, @"^/channels/(@me|[0-9]+)(/[0-9]+)?$");
}

internal sealed class LauncherPreferences
{
    public bool VisualSaving { get; set; }
    public bool CloseToTray { get; set; }
    public bool CompactMemory { get; set; } = true;
    public bool LeanInterface { get; set; } = true;
    public bool BlockTelemetry { get; set; } = true;
    public bool ReleaseUnusedResources { get; set; } = true;
    public bool CompressSources { get; set; } = true;
}

internal sealed class MainWindow : Form
{
    private readonly AppPaths paths;
    private readonly bool smoke, benchmark, baselineEngine, measureSession, extendedSmoke;
    private readonly Dictionary<string, int> blockedTelemetry = new();
    private int sessionProbeCount;
    private bool sessionProbeBusy;
    private readonly System.Windows.Forms.Timer sessionProbeTimer = new() { Interval = 10000 };
    private readonly List<Form> popups = new();
    private readonly ToolStripMenuItem reloadItem;
    private bool browserFailed, youtubeEmbedCompatibility, optimizationInitialized;
    private readonly WebView2 view = new() { Dock = DockStyle.Fill };
    private readonly ToolStripStatusLabel status = new() { Text = "Iniciando…", Spring = true, TextAlign = ContentAlignment.MiddleLeft };
    private readonly NotifyIcon tray = new() { Icon = SystemIcons.Application, Text = "NotDiscord" };
    private readonly Stopwatch lifetime = Stopwatch.StartNew();
    private readonly System.Windows.Forms.Timer watchdog = new() { Interval = 58000 };
    private readonly LauncherPreferences preferences;
    private CoreWebView2Environment? environment;
    private bool initialized, exiting, importing, smokeFinished;
    private string extensionStatus = "pendiente";
    private string? visualScriptId;
    private string? lastSavedChannel;
    private readonly string profilePath;
    private readonly ToolStripMenuItem visualItem;
    private readonly ToolStripMenuItem trayItem;
    private readonly ToolStripMenuItem importItem;

    public MainWindow(AppPaths paths, bool smoke, bool benchmark, bool baselineEngine, bool measureSession, bool extendedSmoke)
    {
        this.paths = paths; this.smoke = smoke; this.benchmark = benchmark;
        this.baselineEngine = baselineEngine; this.measureSession = measureSession; this.extendedSmoke = extendedSmoke;
        preferences = ReadPreferences();
        profilePath = paths.Inside(smoke ? Path.Combine("data", "smoke-profile", DateTime.UtcNow.ToString("yyyyMMdd-HHmmss-fff")) : Path.Combine("data", "profile"));
        Directory.CreateDirectory(profilePath);
        Text = smoke ? "NotDiscord — prueba sin sesión" : "NotDiscord";
        Size = new Size(1180, 800); MinimumSize = new Size(780, 540);
        StartPosition = FormStartPosition.CenterScreen;
        Icon = SystemIcons.Application;
        var menu = new MenuStrip();
        reloadItem = new ToolStripMenuItem("Recargar", null, (_, _) =>
        {
            if (browserFailed) { RestartApplication(); return; }
            if (initialized) { try { view.Reload(); } catch { OnBrowserFailed(); } }
        });
        menu.Items.Add(reloadItem);
        importItem = new ToolStripMenuItem("Importar plugins", null, async (_, _) => await ImportPluginsAsync(true));
        menu.Items.Add(importItem);
        visualItem = new ToolStripMenuItem("Ahorro visual") { CheckOnClick = true, Checked = preferences.VisualSaving };
        visualItem.CheckedChanged += async (_, _) => { preferences.VisualSaving = visualItem.Checked; SavePreferences(); await ApplyVisualSavingAsync(); };
        menu.Items.Add(visualItem);
        var compactItem = new ToolStripMenuItem("Memoria compacta") { CheckOnClick = true, Checked = preferences.CompactMemory };
        compactItem.CheckedChanged += (_, _) => { preferences.CompactMemory = compactItem.Checked; SavePreferences(); SetStatus("El cambio de memoria se aplicará al cerrar y volver a abrir NotDiscord."); };
        menu.Items.Add(compactItem);
        trayItem = new ToolStripMenuItem("Cerrar a bandeja") { CheckOnClick = true, Checked = preferences.CloseToTray };
        trayItem.CheckedChanged += (_, _) => { preferences.CloseToTray = trayItem.Checked; SavePreferences(); };
        menu.Items.Add(trayItem);
        menu.Items.Add(new ToolStripMenuItem("Diagnóstico", null, async (_, _) => await SaveDiagnosticsAsync()));
        menu.Items.Add(new ToolStripMenuItem("Salir", null, (_, _) => { exiting = true; Close(); }));
        var strip = new StatusStrip(); strip.Items.Add(status);
        Controls.Add(view); Controls.Add(strip); Controls.Add(menu); MainMenuStrip = menu;
        var trayMenu = new ContextMenuStrip();
        trayMenu.Items.Add("Mostrar", null, (_, _) => ShowFromTray());
        trayMenu.Items.Add("Salir", null, (_, _) => { exiting = true; Close(); });
        tray.ContextMenuStrip = trayMenu;
        tray.DoubleClick += (_, _) => ShowFromTray();
        Resize += (_, _) => AdjustMemoryTarget();
        sessionProbeTimer.Tick += async (_, _) => { if (++sessionProbeCount >= 18) sessionProbeTimer.Stop(); await SaveDiagnosticsAsync(); };
        if (smoke) { ShowInTaskbar = false; Opacity = 0; }
        watchdog.Interval = (benchmark || extendedSmoke) ? 160000 : 58000;
        watchdog.Tick += (_, _) => FinishSmoke(false, new { error = "Tiempo máximo de prueba alcanzado", limitSeconds = (benchmark || extendedSmoke) ? 160 : 58 });
    }

    protected override async void OnShown(EventArgs e)
    {
        base.OnShown(e);
        if (smoke) watchdog.Start();
        try
        {
            var options = new CoreWebView2EnvironmentOptions { AreBrowserExtensionsEnabled = true };
            options.AdditionalBrowserArguments = EnginePolicy.BuildArguments(preferences.CompactMemory, preferences.ReleaseUnusedResources, preferences.CompactMemory && preferences.CompressSources, baselineEngine);
            environment = await CoreWebView2Environment.CreateAsync(null, profilePath, options);
            await view.EnsureCoreWebView2Async(environment);
            if (IsDisposed) return;
            var core = view.CoreWebView2;
            core.Settings.AreHostObjectsAllowed = false;
            core.Settings.IsWebMessageEnabled = false;
            core.Settings.IsStatusBarEnabled = false;
            core.Settings.AreDevToolsEnabled = !smoke;
            if (preferences.BlockTelemetry && !baselineEngine) ConfigureTelemetryBlocking(core);
            if (preferences.LeanInterface && !baselineEngine) {
                await core.AddScriptToExecuteOnDocumentCreatedAsync(CommercialPreload.Script);
                await core.AddScriptToExecuteOnDocumentCreatedAsync(LeanInterface.Script);
            }
            core.NavigationStarting += OnNavigationStarting;
            core.SourceChanged += (_, _) => RememberChannel(core.Source);
            core.NewWindowRequested += OnNewWindowRequested;
            core.LaunchingExternalUriScheme += (_, args) => args.Cancel = true;
            core.PermissionRequested += OnPermissionRequested;
            core.DownloadStarting += OnDownloadStarting;
            core.NavigationCompleted += async (_, args) =>
            {
                if (!args.IsSuccess) { SetStatus("No se pudo cargar Discord. Comprueba tu conexión y pulsa Recargar."); return; }
                if (!smoke) {
                    if (!optimizationInitialized && preferences.LeanInterface && !baselineEngine) {
                        optimizationInitialized = true;
                        try {
                        for (int i = 0; i < 34 && !IsDisposed; i++) {
                            using var state = JsonDocument.Parse(await core.ExecuteScriptAsync(LeanInterface.StateScript));
                            if (state.RootElement.ValueKind == JsonValueKind.Object && state.RootElement.TryGetProperty("settingsReady", out var ready) && ready.GetBoolean()) {
                                if (state.RootElement.GetProperty("restartNeeded").GetBoolean()) { SetStatus("Aplicando ajustes de privacidad y promociones…"); await Task.Delay(750); core.Reload(); return; }
                                break;
                            }
                            await Task.Delay(500);
                        }
                        } catch { SetStatus("No se pudieron confirmar todos los ajustes de promociones."); }
                    }
                    await ApplyVisualSavingAsync(); await ImportPluginsAsync(false);
                }
                if (measureSession && !smoke && sessionProbeCount < 18) sessionProbeTimer.Start();
            };
            core.ProcessFailed += (_, args) =>
            {
                if (args.ProcessFailedKind == CoreWebView2ProcessFailedKind.BrowserProcessExited) OnBrowserFailed();
                else SetStatus("Un proceso del navegador falló. Puedes pulsar Recargar.");
            };
            string extensionPath = paths.Inside(Path.Combine("vendor", "vencord", "current"));
            if (File.Exists(Path.Combine(extensionPath, "manifest.json")))
            {
                try
                {
                    var extension = await core.Profile.AddBrowserExtensionAsync(extensionPath);
                    if (!extension.IsEnabled) await extension.EnableAsync(true);
                    extensionStatus = extension.IsEnabled ? "cargada" : "deshabilitada";
                }
                catch (Exception ex) { extensionStatus = "error: " + ex.GetType().Name; }
            }
            else extensionStatus = "no encontrada";
            initialized = true;
            await ApplyVisualSavingAsync();
            try
            {
                string source = paths.Inside("config/vencord-plugins.json");
                if (File.Exists(source) && (JsonSerializer.Deserialize<string[]>(await File.ReadAllTextAsync(source)) ?? Array.Empty<string>()).Contains("FixYoutubeEmbeds", StringComparer.Ordinal))
                {
                    await core.AddScriptToExecuteOnDocumentCreatedAsync(YoutubeEmbedCompatibility.Script);
                    youtubeEmbedCompatibility = true;
                }
            }
            catch { SetStatus("No se pudo cargar la compatibilidad experimental de YouTube."); }
            core.Navigate(ReadStartupChannel());
            SetStatus("Vencord: " + extensionStatus + ". Perfil independiente; inicia sesión en Discord.");
            if (smoke) await RunSmokeAsync();
        }
        catch (Exception ex)
        {
            if (smoke) FinishSmoke(false, new { error = ex.GetType().Name });
            else SetStatus("Error al iniciar: " + ex.GetType().Name + ". Revisa WebView2 y la carpeta vendor.");
        }
    }

    private void SetStatus(string text) { if (!IsDisposed) status.Text = text; }
    private LauncherPreferences ReadPreferences()
    {
        try { return JsonSerializer.Deserialize<LauncherPreferences>(File.ReadAllText(paths.Inside("config/launcher.json"))) ?? new(); }
        catch { return new(); }
    }
    private void SavePreferences() { if (!smoke) paths.WriteJson("config/launcher.json", preferences); }
    private void ShowFromTray() { Show(); WindowState = FormWindowState.Normal; Activate(); tray.Visible = false; AdjustMemoryTarget(); }
    private void AdjustMemoryTarget()
    {
        if (!initialized) return;
        try { view.CoreWebView2.MemoryUsageTargetLevel = WindowState == FormWindowState.Minimized || !Visible ? CoreWebView2MemoryUsageTargetLevel.Low : CoreWebView2MemoryUsageTargetLevel.Normal; }
        catch (NotImplementedException) { }
        catch (System.Runtime.InteropServices.COMException) { }
    }

    private string ReadStartupChannel()
    {
        if (smoke) return "https://discord.com/app";
        try {
            string? saved = JsonSerializer.Deserialize<string>(File.ReadAllText(paths.Inside("data/last-channel.json")));
            if (NavigationPolicy.IsRestorableChannel(saved)) { lastSavedChannel = saved; return saved!; }
        } catch { }
        return "https://discord.com/app";
    }
    private void RememberChannel(string url)
    {
        if (smoke || url == lastSavedChannel || !NavigationPolicy.IsRestorableChannel(url)) return;
        try {
            // Private navigation state, never copied into diagnostics or evidence.
            string target = paths.Inside("data/last-channel.json");
            File.WriteAllText(target + ".tmp", JsonSerializer.Serialize(url));
            File.Move(target + ".tmp", target, true);
            lastSavedChannel = url;
        } catch { }
    }
    private void OnNavigationStarting(object? sender, CoreWebView2NavigationStartingEventArgs e)
    {
        if (NavigationPolicy.IsDiscord(e.Uri)) return;
        e.Cancel = true;
        if (!smoke && NavigationPolicy.CanOpenExternal(e.Uri, e.IsUserInitiated)) OpenExternal(e.Uri);
    }
    private async void OnNewWindowRequested(object? sender, CoreWebView2NewWindowRequestedEventArgs e)
    {
        e.Handled = true;
        if (!e.IsUserInitiated || smoke) return;
        if (e.Uri == "about:blank" && e.Name == "VencordQuickCss" && NavigationPolicy.IsDiscord(e.OriginalSourceFrameInfo.Source) && NavigationPolicy.IsDiscord(view.CoreWebView2.Source))
        {
            using var deferral = e.GetDeferral();
            Form? popup = null;
            try
            {
                popup = new Form { Text = "NotDiscord — QuickCSS", Width = 960, Height = 700, StartPosition = FormStartPosition.CenterParent, Icon = SystemIcons.Application };
                var editor = new WebView2 { Dock = DockStyle.Fill };
                popup.Controls.Add(editor); popups.Add(popup);
                popup.FormClosed += (_, _) => { popups.Remove(popup); editor.Dispose(); };
                await editor.EnsureCoreWebView2Async(environment);
                if (IsDisposed || popup.IsDisposed) return;
                var popupCore = editor.CoreWebView2;
                popupCore.Settings.AreHostObjectsAllowed = false;
                popupCore.Settings.IsWebMessageEnabled = false;
                popupCore.Settings.IsStatusBarEnabled = false;
                bool initialBlankAllowed = true;
                popupCore.NavigationStarting += (_, nav) =>
                {
                    if (initialBlankAllowed && nav.Uri == "about:blank") { initialBlankAllowed = false; return; }
                    initialBlankAllowed = false;
                    if (!NavigationPolicy.IsDiscord(nav.Uri)) { nav.Cancel = true; if (NavigationPolicy.CanOpenExternal(nav.Uri, nav.IsUserInitiated)) OpenExternal(nav.Uri); }
                };
                popupCore.NewWindowRequested += (_, requested) => requested.Handled = true;
                popupCore.LaunchingExternalUriScheme += (_, request) => request.Cancel = true;
                popupCore.PermissionRequested += (_, request) => request.State = CoreWebView2PermissionState.Deny;
                popupCore.DownloadStarting += (_, download) => download.Cancel = true;
                popupCore.WindowCloseRequested += (_, _) => popup.Close();
                e.NewWindow = popupCore; popup.Show(this);
            }
            catch { popup?.Dispose(); SetStatus("No se pudo abrir el editor QuickCSS."); }
            return;
        }
        if (NavigationPolicy.IsDiscord(e.Uri)) view.CoreWebView2.Navigate(e.Uri);
        else if (NavigationPolicy.CanOpenExternal(e.Uri, true)) OpenExternal(e.Uri);
    }
    private void OnBrowserFailed()
    {
        initialized = false; browserFailed = true;
        if (smoke) { FinishSmoke(false, new { error = "BrowserProcessExited" }); return; }
        reloadItem.Text = "Reiniciar";
        importItem.Enabled = false; visualItem.Enabled = false;
        SetStatus("El navegador se cerró. Pulsa Reiniciar para abrir una nueva instancia.");
    }
    private void RestartApplication()
    {
        try
        {
            Process.Start(new ProcessStartInfo(Application.ExecutablePath) { Arguments = "--wait-for-parent " + Environment.ProcessId, WorkingDirectory = paths.Root, UseShellExecute = false });
            exiting = true; Close();
        }
        catch { SetStatus("No se pudo reiniciar. Cierra y abre NotDiscord manualmente."); }
    }
    private static void OpenExternal(string value)
    {
        if (!NavigationPolicy.IsHttp(value)) return;
        try { Process.Start(new ProcessStartInfo(value) { UseShellExecute = true }); }
        catch { MessageBox.Show("No se pudo abrir el navegador predeterminado.", "NotDiscord"); }
    }
    private void OnPermissionRequested(object? sender, CoreWebView2PermissionRequestedEventArgs e)
    {
        e.State = CoreWebView2PermissionState.Deny;
        if (smoke || !NavigationPolicy.IsDiscord(e.Uri)) return;
        string? label = e.PermissionKind switch
        {
            CoreWebView2PermissionKind.Microphone => "usar el micrófono",
            CoreWebView2PermissionKind.Camera => "usar la cámara",
            CoreWebView2PermissionKind.Notifications => "mostrar notificaciones",
            CoreWebView2PermissionKind.ClipboardRead => "leer el portapapeles",
            _ => null
        };
        if (label == null) return;
        using var deferral = e.GetDeferral();
        if (MessageBox.Show(this, "https://discord.com solicita " + label + ".\n¿Permitirlo?", "Permiso de Discord", MessageBoxButtons.YesNo, MessageBoxIcon.Question) == DialogResult.Yes)
            e.State = CoreWebView2PermissionState.Allow;
        e.SavesInProfile = false;
    }
    private void OnDownloadStarting(object? sender, CoreWebView2DownloadStartingEventArgs e)
    {
        using var deferral = e.GetDeferral();
        e.Cancel = true; e.Handled = true;
        if (smoke || !NavigationPolicy.IsDiscord(view.CoreWebView2.Source)) return;
        using var dialog = new SaveFileDialog { Title = "Guardar descarga", FileName = Path.GetFileName(e.ResultFilePath), InitialDirectory = paths.Root, Filter = "Todos los archivos|*.*", OverwritePrompt = true };
        if (dialog.ShowDialog(this) == DialogResult.OK) { e.ResultFilePath = dialog.FileName; e.Cancel = false; SetStatus("Descargando el archivo seleccionado…"); }
    }

    private async Task ApplyVisualSavingAsync()
    {
        if (!initialized) return;
        try
        {
            if (visualScriptId != null) { view.CoreWebView2.RemoveScriptToExecuteOnDocumentCreated(visualScriptId); visualScriptId = null; }
            string code = VisualScript(preferences.VisualSaving);
            if (preferences.VisualSaving) visualScriptId = await view.CoreWebView2.AddScriptToExecuteOnDocumentCreatedAsync(code);
            await view.CoreWebView2.ExecuteScriptAsync(code);
            string features = preferences.VisualSaving ? "{\"features\":[{\"name\":\"prefers-reduced-motion\",\"value\":\"reduce\"}]}" : "{\"features\":[]}";
            await view.CoreWebView2.CallDevToolsProtocolMethodAsync("Emulation.setEmulatedMedia", features);
        }
        catch { SetStatus("No se pudo aplicar el ahorro visual en esta página; prueba Recargar."); }
    }
    internal static string VisualScript(bool enabled) => "(() => { if(location.origin !== 'https://discord.com') return; const apply=()=>{document.getElementById('notdiscord-visual-saving')?.remove(); " + (enabled ? "const style=document.createElement('style'); style.id='notdiscord-visual-saving'; style.textContent='*,*::before,*::after{animation-duration:0.001ms!important;animation-iteration-count:1!important;transition-duration:0.001ms!important;scroll-behavior:auto!important}'; (document.head||document.documentElement).appendChild(style);" : "") + "}; if(document.documentElement)apply();else document.addEventListener('DOMContentLoaded',apply,{once:true}); })();";

    private async Task ImportPluginsAsync(bool explicitRequest)
    {
        if (!initialized || smoke || importing) return;
        string marker = paths.Inside("data/plugins-imported.json");
        string source = paths.Inside("config/vencord-plugins.json");
        if (!File.Exists(source) || (!explicitRequest && File.Exists(marker))) return;
        importing = true; importItem.Enabled = false;
        try
        {
            string[] requested = (JsonSerializer.Deserialize<string[]>(await File.ReadAllTextAsync(source)) ?? Array.Empty<string>())
                .Where(s => !string.IsNullOrWhiteSpace(s) && s.Length <= 128 && s != "__proto__" && s != "constructor" && s != "prototype")
                .Distinct(StringComparer.Ordinal).Take(512).ToArray();
            string names = JsonSerializer.Serialize(requested);
            string script = "(() => { if(location.origin!=='https://discord.com')return {ready:false}; const v=globalThis.Vencord; const available=v?.Plugins?.plugins; if(!v?.Settings?.plugins||!available)return {ready:false}; const requested=" + names + "; const applied=[], unavailable=[]; for(const name of requested){if(Object.prototype.hasOwnProperty.call(available,name)){v.Settings.plugins[name].enabled=true;applied.push(name);}else unavailable.push(name);} return {ready:true,applied,unavailable};})()";
            for (int attempt = 0; attempt < 30 && !IsDisposed; attempt++)
            {
                string result = await view.CoreWebView2.ExecuteScriptAsync(script);
                using var parsed = JsonDocument.Parse(result);
                if (parsed.RootElement.ValueKind == JsonValueKind.Object && parsed.RootElement.TryGetProperty("ready", out var ready) && ready.GetBoolean())
                {
                    await File.WriteAllTextAsync(marker, result);
                    int applied = parsed.RootElement.GetProperty("applied").GetArrayLength();
                    int skipped = parsed.RootElement.GetProperty("unavailable").GetArrayLength();
                    SetStatus($"Plugins importados: {applied}; no disponibles en web: {skipped}. Pulsa Recargar para activarlos.");
                    return;
                }
                await Task.Delay(500);
            }
            if (explicitRequest) SetStatus("Vencord todavía no está listo. Entra a Discord y vuelve a importar.");
        }
        catch (Exception ex) { SetStatus("No se pudieron importar los plugins: " + ex.GetType().Name); }
        finally { importing = false; if (!IsDisposed) importItem.Enabled = true; }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ProcessMemoryCountersEx2
    {
        public uint cb, PageFaultCount;
        public nuint PeakWorkingSetSize, WorkingSetSize, QuotaPeakPagedPoolUsage, QuotaPagedPoolUsage;
        public nuint QuotaPeakNonPagedPoolUsage, QuotaNonPagedPoolUsage, PagefileUsage, PeakPagefileUsage;
        public nuint PrivateUsage, PrivateWorkingSetSize;
        public ulong SharedCommitUsage;
    }
    [DllImport("psapi.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetProcessMemoryInfo(IntPtr process, out ProcessMemoryCountersEx2 counters, uint size);

    private JsonElement CaptureMetrics()
    {
        var items = new List<object>();
        bool enumerationComplete = environment != null; int readFailures = 0;
        var ids = new HashSet<int> { Environment.ProcessId };
        var kinds = new Dictionary<int, string> { [Environment.ProcessId] = "Host" };
        try { if (environment != null) foreach (var info in environment.GetProcessInfos()) { ids.Add(info.ProcessId); kinds[info.ProcessId] = info.Kind.ToString(); } } catch { enumerationComplete = false; }
        long privateBytes = 0, workingSet = 0, privateWorkingSet = 0; double cpuSeconds = 0;
        bool allPrivateWorkingSetAvailable = true;
        foreach (int id in ids)
        {
            try
            {
                using var process = Process.GetProcessById(id); process.Refresh();
                long? residentPrivate = null;
                if (GetProcessMemoryInfo(process.Handle, out var native, (uint)Marshal.SizeOf<ProcessMemoryCountersEx2>())) residentPrivate = (long)native.PrivateWorkingSetSize;
                if (residentPrivate.HasValue) privateWorkingSet += residentPrivate.Value; else allPrivateWorkingSetAvailable = false;
                privateBytes += process.PrivateMemorySize64; workingSet += process.WorkingSet64; cpuSeconds += process.TotalProcessorTime.TotalSeconds;
                items.Add(new { pid = id, kind = kinds.GetValueOrDefault(id, "WebView2"), privateBytes = process.PrivateMemorySize64, privateWorkingSetBytes = residentPrivate, workingSetBytes = process.WorkingSet64, cpuSeconds = process.TotalProcessorTime.TotalSeconds });
            }
            catch { allPrivateWorkingSetAvailable = false; readFailures++; }
        }
        return JsonSerializer.SerializeToElement(new { utc = DateTime.UtcNow, elapsedSeconds = lifetime.Elapsed.TotalSeconds, logicalProcessors = Environment.ProcessorCount, privateBytes, privateWorkingSetBytes = allPrivateWorkingSetAvailable && enumerationComplete ? (long?)privateWorkingSet : null, coverageComplete = enumerationComplete && readFailures == 0, enumeratedProcessCount = ids.Count, readFailureCount = readFailures, workingSetBytes = workingSet, cpuSeconds, processCount = items.Count, processes = items });
    }
    private void ConfigureTelemetryBlocking(CoreWebView2 core)
    {
        foreach (string filter in TelemetryPolicy.Filters)
            core.AddWebResourceRequestedFilter(filter, CoreWebView2WebResourceContext.All, CoreWebView2WebResourceRequestSourceKinds.All);
        core.WebResourceRequested += (_, e) =>
        {
            if (!TelemetryPolicy.IsBlocked(e.Request.Uri, e.Request.Method)) return;
            blockedTelemetry["analytics"] = blockedTelemetry.GetValueOrDefault("analytics") + 1;
            e.Response = environment!.CreateWebResourceResponse(null, 204, "No Content", "Cache-Control: no-store");
        };
    }
    private async Task SaveDiagnosticsAsync()
    {
        if (sessionProbeBusy || !initialized || IsDisposed) return;
        sessionProbeBusy = true;
        try
        {
            var heap = new Dictionary<string, double>(); var dom = new Dictionary<string, double>();
            foreach (var spec in new[] {
                ("Runtime.getHeapUsage", new[] { "usedSize", "totalSize", "embedderHeapUsedSize", "backingStorageSize" }, heap),
                ("Memory.getDOMCounters", new[] { "documents", "nodes", "jsEventListeners" }, dom) })
            {
                try
                {
                    using var parsed = JsonDocument.Parse(await view.CoreWebView2.CallDevToolsProtocolMethodAsync(spec.Item1, "{}"));
                    foreach (string key in spec.Item2)
                        if (parsed.RootElement.TryGetProperty(key, out var n) && n.TryGetDouble(out double value)) spec.Item3[key] = value;
                }
                catch { }
            }
            string configPath = paths.Inside("config/vencord-plugins.json");
            string[] requested = File.Exists(configPath) ? JsonSerializer.Deserialize<string[]>(File.ReadAllText(configPath)) ?? [] : [];
            string probe = "(() => {if(location.origin!=='https://discord.com')return {discord:false};const v=globalThis.Vencord;const names=" + JsonSerializer.Serialize(requested) + ";const enabled=names.filter(n=>v?.PlainSettings?.plugins?.[n]?.enabled===true);return {discord:true,ready:document.readyState==='complete',chatSurface:!!document.querySelector('[role=textbox][contenteditable=true],textarea'),loginForm:!!document.querySelector('input[type=password]'),vencord:!!v,getUserMedia:typeof navigator.mediaDevices?.getUserMedia==='function',getDisplayMedia:typeof navigator.mediaDevices?.getDisplayMedia==='function',rtc:typeof RTCPeerConnection==='function',requestedCount:names.length,enabledPluginNames:enabled,noTrack:v?.PlainSettings?.plugins?.NoTrack?.disableAnalytics===true};})()";
            using var state = JsonDocument.Parse(await view.CoreWebView2.ExecuteScriptAsync(probe));
            using var leanState = JsonDocument.Parse(await view.CoreWebView2.ExecuteScriptAsync(LeanInterface.StateScript));
            using var commercialState = JsonDocument.Parse(await view.CoreWebView2.ExecuteScriptAsync(CommercialPreload.StateScript));
            var report = new { version = Application.ProductVersion, commercialPreload = commercialState.RootElement.Clone(), leanInterface = leanState.RootElement.Clone(), kind = "aggregate-only-no-account-content", runtime = environment?.BrowserVersionString, compactMemoryRequested = preferences.CompactMemory && !baselineEngine, unusedResourceCacheDisabled = preferences.ReleaseUnusedResources && !baselineEngine, sourceCompressionRequested = preferences.CompactMemory && preferences.CompressSources && !baselineEngine, sourceCacheLimitMiBPerRenderer = EnginePolicy.SourceCacheLimitMiBPerRenderer, leanInterfaceRequested = preferences.LeanInterface && !baselineEngine, extension = extensionStatus, metrics = CaptureMetrics(), rendererHeap = heap, domCounters = dom, capabilities = state.RootElement.Clone(), blockedTelemetry };
            paths.WriteJson("evidence/diagnostics-latest.json", report);
            if (measureSession) paths.WriteJson("evidence/session-probe-" + (baselineEngine ? "baseline" : "compact") + ".json", report);
            if (!measureSession) SetStatus("Diagnóstico total guardado en evidence\\diagnostics-latest.json (sin mensajes ni credenciales).");
        }
        catch { if (!measureSession) SetStatus("No se pudo guardar el diagnóstico."); }
        finally { sessionProbeBusy = false; }
    }
    private async Task RunSmokeAsync()
    {
        JsonElement observed = default;
        object? initialMetrics = null;
        while (lifetime.Elapsed.TotalSeconds < 48 && !smokeFinished && !IsDisposed)
        {
            try
            {
                string state = await view.CoreWebView2.ExecuteScriptAsync("(() => ({origin:location.origin,title:document.title,ready:document.readyState,vencord:typeof globalThis.Vencord!=='undefined',plugins:Object.keys(globalThis.Vencord?.Plugins?.plugins||{}),getUserMedia:typeof navigator.mediaDevices?.getUserMedia==='function',getDisplayMedia:typeof navigator.mediaDevices?.getDisplayMedia==='function',loginDocument:!!document.querySelector('input[type=password]')}))()");
                using var json = JsonDocument.Parse(state);
                observed = json.RootElement.Clone();
                if (observed.ValueKind == JsonValueKind.Object && observed.GetProperty("origin").GetString() == "https://discord.com" && observed.GetProperty("ready").GetString() == "complete")
                {
                    initialMetrics ??= CaptureMetrics();
                    if (observed.GetProperty("vencord").GetBoolean() && observed.GetProperty("plugins").GetArrayLength() > 0 && observed.GetProperty("loginDocument").GetBoolean()) break;
                }
            }
            catch { }
            await Task.Delay(600);
        }
        if (smokeFinished || IsDisposed) return;
        object? pluginImport = null; object? pluginPersistence = null;
        try
        {
            string source = paths.Inside("config/vencord-plugins.json");
            if (File.Exists(source))
            {
                string[] names = (JsonSerializer.Deserialize<string[]>(await File.ReadAllTextAsync(source)) ?? Array.Empty<string>())
                    .Where(s => !string.IsNullOrWhiteSpace(s) && s.Length <= 128 && s != "__proto__" && s != "constructor" && s != "prototype")
                    .Distinct(StringComparer.Ordinal).Take(512).ToArray();
                string script = "(() => { if(location.origin!=='https://discord.com')return {ready:false}; const v=globalThis.Vencord; const available=v?.Plugins?.plugins;if(!v?.Settings?.plugins||!available)return {ready:false};const applied=[],unavailable=[];for(const name of " + JsonSerializer.Serialize(names) + "){if(Object.prototype.hasOwnProperty.call(available,name)){v.Settings.plugins[name].enabled=true;applied.push(name);}else unavailable.push(name);}return {ready:true,applied,unavailable,enabledConfirmed:applied.every(name=>v.Settings.plugins[name].enabled===true)};})()";
                string result = await view.CoreWebView2.ExecuteScriptAsync(script);
                using var json = JsonDocument.Parse(result);
                pluginImport = json.RootElement.Clone();
                await File.WriteAllTextAsync(Path.Combine(profilePath, "plugins-imported.json"), result);
                if (json.RootElement.ValueKind == JsonValueKind.Object && json.RootElement.TryGetProperty("ready", out var ready) && ready.GetBoolean() && lifetime.Elapsed.TotalSeconds < 40)
                {
                    await Task.Delay(1500);
                    view.Reload();
                    string verificationScript = "(() => {if(location.origin!=='https://discord.com'||document.readyState!=='complete')return {ready:false};const v=globalThis.Vencord;const available=v?.Plugins?.plugins;if(!v?.Settings?.plugins||!available)return {ready:false};const requested=" + JsonSerializer.Serialize(names) + ";const supported=requested.filter(name=>Object.prototype.hasOwnProperty.call(available,name));return {ready:true,supported,missing:requested.filter(name=>!Object.prototype.hasOwnProperty.call(available,name)),enabled:supported.filter(name=>v.Settings.plugins[name].enabled===true),allEnabled:supported.every(name=>v.Settings.plugins[name].enabled===true)};})()";
                    await Task.Delay(1000);
                    for (int i = 0; i < 20 && lifetime.Elapsed.TotalSeconds < 53 && !IsDisposed; i++)
                    {
                        string verification = await view.CoreWebView2.ExecuteScriptAsync(verificationScript);
                        using var persisted = JsonDocument.Parse(verification);
                        if (persisted.RootElement.ValueKind == JsonValueKind.Object && persisted.RootElement.TryGetProperty("ready", out var persistedReady) && persistedReady.GetBoolean())
                        {
                            pluginPersistence = persisted.RootElement.Clone(); break;
                        }
                        await Task.Delay(500);
                    }
                }
            }
            else pluginImport = new { skipped = "No existe config/vencord-plugins.json" };
        }
        catch (Exception ex) { pluginImport = new { error = ex.GetType().Name }; }
        if (benchmark)
        {
            await RunBenchmarkAsync(pluginImport, pluginPersistence, observed);
            return;
        }
        if (extendedSmoke) {
            paths.WriteJson("evidence/long-smoke-progress.json", new { phase = "waiting-for-source-compression", waitSeconds = 90, utc = DateTime.UtcNow });
            await Task.Delay(90000);
            if (IsDisposed || smokeFinished) return;
        }
        JsonElement mediaSelfTest = lifetime.Elapsed.TotalSeconds < (extendedSmoke ? 145 : 43) ? await MediaSelfTest.RunAsync(view.CoreWebView2) : JsonSerializer.SerializeToElement(new { success = false, error = "noTimeBudget" });
        // A complete HTML document can precede React's login paint after the reload.
        while (lifetime.Elapsed.TotalSeconds < 54 && !IsDisposed)
        {
            string painted = await view.CoreWebView2.ExecuteScriptAsync("!!document.querySelector('input[type=password]')");
            if (painted == "true") { await Task.Delay(700); break; }
            await Task.Delay(400);
        }
        bool captured = false;
        try
        {
            if (observed.ValueKind == JsonValueKind.Object && observed.GetProperty("loginDocument").GetBoolean())
            {
                // Do not retain a login QR, input contents, or other transient authentication material.
                await view.CoreWebView2.ExecuteScriptAsync("(() => { const s=document.createElement('style'); s.textContent='canvas,svg,img,input,[class*=qrCode],[class*=qrLogin],[class*=QRCode]{visibility:hidden!important}'; document.head.append(s); })()");
                await Task.Delay(200);
                string capturePath = paths.Inside("evidence/smoke-latest.png");
                using (var image = File.Create(capturePath)) await view.CoreWebView2.CapturePreviewAsync(CoreWebView2CapturePreviewImageFormat.Png, image);
                captured = HasVisibleContent(capturePath);
                if (!captured)
                {
                    string capture = await view.CoreWebView2.CallDevToolsProtocolMethodAsync("Page.captureScreenshot", "{\"format\":\"png\",\"captureBeyondViewport\":true,\"fromSurface\":true}");
                    using var captureJson = JsonDocument.Parse(capture);
                    await File.WriteAllBytesAsync(capturePath, Convert.FromBase64String(captureJson.RootElement.GetProperty("data").GetString()!));
                    captured = HasVisibleContent(capturePath);
                }
            }
        }
        catch { }
        await Task.Delay(1500);
        bool success = observed.ValueKind == JsonValueKind.Object && observed.GetProperty("origin").GetString() == "https://discord.com"
            && !string.IsNullOrWhiteSpace(observed.GetProperty("title").GetString()) && observed.GetProperty("loginDocument").GetBoolean()
            && observed.GetProperty("vencord").GetBoolean() && observed.GetProperty("plugins").GetArrayLength() > 0
            && observed.GetProperty("getUserMedia").GetBoolean() && observed.GetProperty("getDisplayMedia").GetBoolean();
        success = success && captured && mediaSelfTest.TryGetProperty("success", out var mediaOk) && mediaOk.GetBoolean();
        if (File.Exists(paths.Inside("config/vencord-plugins.json"))) success = success && pluginPersistence is JsonElement persistedState && persistedState.TryGetProperty("allEnabled", out var allEnabled) && allEnabled.GetBoolean();
        using var commercialState = JsonDocument.Parse(await view.CoreWebView2.ExecuteScriptAsync(CommercialPreload.StateScript));
        FinishSmoke(success, new { commercialPreload = commercialState.RootElement.Clone(), extendedSmoke, postLoadWaitSeconds = extendedSmoke ? 90 : 0, mediaSelfTest, pluginImport, pluginPersistence, observed = observed.ValueKind == JsonValueKind.Undefined ? (object?)null : observed, screenshotCaptured = captured, initialMetrics, finalMetrics = CaptureMetrics(), notes = "Perfil nuevo sin credenciales. APIs presentes no prueban llamadas, captura real ni rendimiento autenticado." });
    }
    private static bool HasVisibleContent(string path)
    {
        using var bitmap = new Bitmap(path);
        var colors = new HashSet<int>();
        for (int y = 0; y < bitmap.Height; y += Math.Max(1, bitmap.Height / 50))
            for (int x = 0; x < bitmap.Width; x += Math.Max(1, bitmap.Width / 80)) colors.Add(bitmap.GetPixel(x, y).ToArgb());
        return colors.Count > 12;
    }
    private async Task RunBenchmarkAsync(object? import, object? persistence, JsonElement observed)
    {
        if (observed.ValueKind != JsonValueKind.Object || !observed.GetProperty("loginDocument").GetBoolean())
        {
            FinishSmoke(false, new { error = "No se confirmó la página de login sin sesión." }); return;
        }
        paths.WriteJson("evidence/benchmark-progress.json", new { phase = "estabilizando", startedUtc = DateTime.UtcNow });
        view.CoreWebView2.MemoryUsageTargetLevel = CoreWebView2MemoryUsageTargetLevel.Normal;
        await Task.Delay(60000);
        if (IsDisposed || smokeFinished) return;
        var samples = new List<object>();
        foreach (string phase in new[] { "oculto-normal", "minimizado-low" })
        {
            if (phase == "minimizado-low")
            {
                WindowState = FormWindowState.Minimized;
                view.CoreWebView2.MemoryUsageTargetLevel = CoreWebView2MemoryUsageTargetLevel.Low;
            }
            paths.WriteJson("evidence/benchmark-progress.json", new { phase, startedUtc = DateTime.UtcNow });
            JsonElement previous = CaptureMetrics();
            for (int i = 0; i < 15; i++)
            {
                await Task.Delay(2000);
                if (IsDisposed || smokeFinished) return;
                JsonElement current = CaptureMetrics();
                double seconds = current.GetProperty("elapsedSeconds").GetDouble() - previous.GetProperty("elapsedSeconds").GetDouble();
                double cpu = Math.Max(0, current.GetProperty("cpuSeconds").GetDouble() - previous.GetProperty("cpuSeconds").GetDouble());
                                var previousPids = previous.GetProperty("processes").EnumerateArray().Select(p => p.GetProperty("pid").GetInt32()).Order().ToArray();
                var currentPids = current.GetProperty("processes").EnumerateArray().Select(p => p.GetProperty("pid").GetInt32()).Order().ToArray();
                bool completeCpuInterval = previous.GetProperty("coverageComplete").GetBoolean() && current.GetProperty("coverageComplete").GetBoolean() && previousPids.SequenceEqual(currentPids);
                samples.Add(new { phase, index = i, intervalSeconds = seconds, completeCpuInterval, cpuPercentOfMachine = completeCpuInterval ? (double?)(100 * cpu / seconds / Environment.ProcessorCount) : null, cpuPercentOneCore = completeCpuInterval ? (double?)(100 * cpu / seconds) : null, metrics = current });
                previous = current;
            }
        }
        paths.WriteJson("evidence/benchmark-progress.json", new { phase = "completo", endedUtc = DateTime.UtcNow });
        FinishSmoke(true, new { pluginImport = import, pluginPersistence = persistence, origin = observed.GetProperty("origin").GetString(), title = observed.GetProperty("title").GetString(), warmupSeconds = 60, sampleSecondsPerPhase = 30, samples, notes = "Ventana invisible sin sesión, login web; no captura durante benchmark. Normal y Low no equivalen a actividad autenticada. PrivateWorkingSet excluye páginas compartidas; PrivateBytes es memoria comprometida. CPU normalizada por capacidad total del equipo." });
    }
    private void FinishSmoke(bool success, object details)
    {
        if (smokeFinished) return;
        smokeFinished = true; watchdog.Stop(); Environment.ExitCode = success ? 0 : 1;
        try { paths.WriteJson(benchmark ? "evidence/benchmark-latest.json" : "evidence/smoke-latest.json", new { success, runtime = environment?.BrowserVersionString, extension = extensionStatus, youtubeEmbedCompatibility, elapsedSeconds = lifetime.Elapsed.TotalSeconds, details }); } catch { }
        exiting = true; Close();
    }
    protected override void OnFormClosing(FormClosingEventArgs e)
    {
        if (!smoke && !exiting && preferences.CloseToTray && e.CloseReason == CloseReason.UserClosing)
        {
            e.Cancel = true; tray.Visible = true; Hide(); AdjustMemoryTarget(); return;
        }
        exiting = true; watchdog.Stop(); sessionProbeTimer.Stop(); sessionProbeTimer.Dispose(); tray.Visible = false; tray.Dispose();
        foreach (var popup in popups.ToArray()) popup.Close();
        view.Dispose(); watchdog.Dispose();
        base.OnFormClosing(e);
    }
}

internal static class SelfTests
{
    public static int Run(AppPaths paths)
    {
        var checks = new Dictionary<string, bool>
        {
            ["discordHttps"] = NavigationPolicy.IsDiscord("https://discord.com/app"),
            ["restoreChannel"] = NavigationPolicy.IsRestorableChannel("https://discord.com/channels/123/456"),
            ["restoreDm"] = NavigationPolicy.IsRestorableChannel("https://discord.com/channels/@me/456"),
            ["restoreFriends"] = NavigationPolicy.IsRestorableChannel("https://discord.com/channels/@me"),
            ["rejectRestoredScript"] = !NavigationPolicy.IsRestorableChannel("javascript:alert(1)"),
            ["rejectRestoredQuery"] = !NavigationPolicy.IsRestorableChannel("https://discord.com/channels/123/456?redirect=bad"),
            ["rejectRestoredSuffix"] = !NavigationPolicy.IsRestorableChannel("https://discord.com.evil.test/channels/123/456"),
            ["rejectRestoredCommercial"] = !NavigationPolicy.IsRestorableChannel("https://discord.com/shop"),
            ["rejectDiscordHttp"] = !NavigationPolicy.IsDiscord("http://discord.com/app"),
            ["rejectSuffix"] = !NavigationPolicy.IsDiscord("https://discord.com.evil.test"),
            ["rejectUserInfo"] = !NavigationPolicy.IsDiscord("https://name@discord.com/app"),
            ["rejectForeignPort"] = !NavigationPolicy.IsDiscord("https://discord.com:8443/app"),
            ["rejectFile"] = !NavigationPolicy.CanOpenExternal("file:///C:/Windows", true),
            ["rejectCustomScheme"] = !NavigationPolicy.CanOpenExternal("ms-settings:privacy", true),
            ["rejectJavascript"] = !NavigationPolicy.CanOpenExternal("javascript:alert(1)", true),
            ["rejectUnsolicitedExternal"] = !NavigationPolicy.CanOpenExternal("https://example.com", false),
            ["userExternalHttp"] = NavigationPolicy.CanOpenExternal("https://example.com", true),
            ["profileWithinRoot"] = paths.Inside("data/profile").StartsWith(paths.Root, StringComparison.OrdinalIgnoreCase),
            ["visualOriginGuard"] = MainWindow.VisualScript(true).Contains("location.origin !== 'https://discord.com'")
        };
        try { paths.Inside("../escape"); checks["rejectTraversal"] = false; } catch (ArgumentException) { checks["rejectTraversal"] = true; }
        try { paths.Inside("C:\\escape"); checks["rejectAbsolute"] = false; } catch (ArgumentException) { checks["rejectAbsolute"] = true; }
        foreach (var check in TelemetryPolicy.SelfTestResults()) checks[check.Key] = check.Value;
        foreach (var check in EnginePolicy.SelfTestResults()) checks[check.Key] = check.Value;
        bool success = checks.Values.All(x => x);
        paths.WriteJson("evidence/self-test.json", new { success, checks });
        return success ? 0 : 1;
    }
}





