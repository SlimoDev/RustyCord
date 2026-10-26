const fs = require('node:fs');
const path = require('node:path');
const root = 'E:/DESCARGAS/REA/NotDiscord';
const file = root + '/src/NotDiscord/Program.cs';
let s = fs.readFileSync(file, 'utf8').replace(/\n/g, '\n');
fs.mkdirSync(root+'/build/v01-backup', {recursive:true});
if (!fs.existsSync(root+'/build/v01-backup/Program.cs')) fs.copyFileSync(file,root+'/build/v01-backup/Program.cs');
function replace(a,b) { if(!s.includes(a)) throw Error('Missing anchor: '+a.slice(0,80)); s=s.replace(a,b); }
replace('new MainWindow(paths, smoke, benchmark)', 'new MainWindow(paths, smoke, benchmark, args.Contains("--baseline-engine"), args.Contains("--measure-session"))');
replace('public bool CloseToTray { get; set; }', 'public bool CloseToTray { get; set; }\n    public bool CompactMemory { get; set; } = true;\n    public bool LeanInterface { get; set; } = true;\n    public bool BlockTelemetry { get; set; } = true;');
replace('private readonly bool smoke, benchmark;', 'private readonly bool smoke, benchmark, baselineEngine, measureSession;\n    private readonly Dictionary<string, int> blockedTelemetry = new();\n    private int sessionProbeCount;\n    private bool sessionProbeBusy;\n    private readonly System.Windows.Forms.Timer sessionProbeTimer = new() { Interval = 10000 };');
replace('public MainWindow(AppPaths paths, bool smoke, bool benchmark)', 'public MainWindow(AppPaths paths, bool smoke, bool benchmark, bool baselineEngine, bool measureSession)');
replace('this.paths = paths; this.smoke = smoke; this.benchmark = benchmark;', 'this.paths = paths; this.smoke = smoke; this.benchmark = benchmark;\n        this.baselineEngine = baselineEngine; this.measureSession = measureSession;');
replace('(_, _) => SaveDiagnostics()', 'async (_, _) => await SaveDiagnosticsAsync()');
replace('Resize += (_, _) => AdjustMemoryTarget();', 'Resize += (_, _) => AdjustMemoryTarget();\n        sessionProbeTimer.Tick += async (_, _) => { if (++sessionProbeCount >= 18) sessionProbeTimer.Stop(); await SaveDiagnosticsAsync(); };');
replace('var options = new CoreWebView2EnvironmentOptions { AreBrowserExtensionsEnabled = true };', 'var options = new CoreWebView2EnvironmentOptions { AreBrowserExtensionsEnabled = true };\n            if (preferences.CompactMemory && !baselineEngine) options.AdditionalBrowserArguments = "--js-flags=--optimize-for-size";');
replace('core.Settings.AreDevToolsEnabled = !smoke;', 'core.Settings.AreDevToolsEnabled = !smoke;\n            if (preferences.BlockTelemetry && !baselineEngine) ConfigureTelemetryBlocking(core);\n            if (preferences.LeanInterface && !baselineEngine) await core.AddScriptToExecuteOnDocumentCreatedAsync(LeanInterface.Script);');
replace('if (!smoke) { await ApplyVisualSavingAsync(); await ImportPluginsAsync(false); }', 'if (!smoke) { await ApplyVisualSavingAsync(); await ImportPluginsAsync(false); }\n                if (measureSession && !smoke) sessionProbeTimer.Start();');
replace('var items = new List<object>();\n        var ids', 'var items = new List<object>();\n        bool enumerationComplete = environment != null; int readFailures = 0;\n        var ids');
replace('kinds[info.ProcessId] = info.Kind.ToString(); } } catch { }', 'kinds[info.ProcessId] = info.Kind.ToString(); } } catch { enumerationComplete = false; }');
replace('catch { allPrivateWorkingSetAvailable = false; }', 'catch { allPrivateWorkingSetAvailable = false; readFailures++; }');
replace('privateWorkingSetBytes = allPrivateWorkingSetAvailable ? (long?)privateWorkingSet : null, workingSetBytes', 'privateWorkingSetBytes = allPrivateWorkingSetAvailable && enumerationComplete ? (long?)privateWorkingSet : null, coverageComplete = enumerationComplete && readFailures == 0, enumeratedProcessCount = ids.Count, readFailureCount = readFailures, workingSetBytes');
const from = s.indexOf('    private void SaveDiagnostics()');
const to = s.indexOf('    private async Task RunSmokeAsync()',from);
if(from<0||to<0)throw Error('Missing diagnostics');
s=s.slice(0,from)+`    private void ConfigureTelemetryBlocking(CoreWebView2 core)
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
            string[] requested = JsonSerializer.Deserialize<string[]>(File.ReadAllText(paths.Inside("config/vencord-plugins.json"))) ?? [];
            string probe = "(() => {if(location.origin!=='https://discord.com')return {discord:false};const v=globalThis.Vencord;const names=" + JsonSerializer.Serialize(requested) + ";const enabled=names.filter(n=>v?.Settings?.plugins?.[n]?.enabled===true);return {discord:true,ready:document.readyState==='complete',chatSurface:!!document.querySelector('[role=textbox][contenteditable=true],textarea'),loginForm:!!document.querySelector('input[type=password]'),vencord:!!v,getUserMedia:typeof navigator.mediaDevices?.getUserMedia==='function',getDisplayMedia:typeof navigator.mediaDevices?.getDisplayMedia==='function',rtc:typeof RTCPeerConnection==='function',requestedCount:names.length,enabledPluginNames:enabled,noTrack:v?.Settings?.plugins?.NoTrack?.disableAnalytics===true};})()";
            using var state = JsonDocument.Parse(await view.CoreWebView2.ExecuteScriptAsync(probe));
            var report = new { kind = "aggregate-only-no-account-content", runtime = environment?.BrowserVersionString, compactMemoryRequested = preferences.CompactMemory && !baselineEngine, leanInterfaceRequested = preferences.LeanInterface && !baselineEngine, extension = extensionStatus, metrics = CaptureMetrics(), rendererHeap = heap, domCounters = dom, capabilities = state.RootElement.Clone(), blockedTelemetry };
            paths.WriteJson("evidence/diagnostics-latest.json", report);
            if (measureSession) paths.WriteJson("evidence/session-probe-" + (baselineEngine ? "baseline" : "compact") + ".json", report);
            if (!measureSession) SetStatus("Diagnóstico total guardado en evidence\\\\diagnostics-latest.json (sin mensajes ni credenciales).");
        }
        catch { if (!measureSession) SetStatus("No se pudo guardar el diagnóstico."); }
        finally { sessionProbeBusy = false; }
    }
`+s.slice(to);
replace('exiting = true; watchdog.Stop(); tray.Visible = false; tray.Dispose();', 'exiting = true; watchdog.Stop(); sessionProbeTimer.Stop(); sessionProbeTimer.Dispose(); tray.Visible = false; tray.Dispose();');
fs.writeFileSync(file,s);
const csproj=root+'/src/NotDiscord/NotDiscord.csproj';
fs.writeFileSync(csproj,fs.readFileSync(csproj,'utf8').replace('<Version>0.1.0</Version>','<Version>0.2.0</Version>'));
console.log('Updated host and version; waiting on new policy/interface classes before building.');
