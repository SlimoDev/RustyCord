const fs=require('node:fs');
const root='E:/DESCARGAS/REA/NotDiscord';
const file=root+'/src/NotDiscord/Program.cs';
let s=fs.readFileSync(file,'utf8').replace(/\r\n/g,'\n');
fs.mkdirSync(root+'/build/v02-backup',{recursive:true});
if(!fs.existsSync(root+'/build/v02-backup/Program.cs')) fs.copyFileSync(file,root+'/build/v02-backup/Program.cs');
function replace(a,b){if(!s.includes(a))throw Error('Missing '+a.slice(0,100));s=s.replace(a,b);}
replace('public bool BlockTelemetry { get; set; } = true;', 'public bool BlockTelemetry { get; set; } = true;\n    public bool ReleaseUnusedResources { get; set; } = true;');
replace('private string? visualScriptId;', 'private string? visualScriptId;\n    private string? lastSavedChannel;');
replace('public static bool CanOpenExternal(string? value, bool userInitiated) => userInitiated && IsHttp(value);', `public static bool CanOpenExternal(string? value, bool userInitiated) => userInitiated && IsHttp(value);
    public static bool IsRestorableChannel(string? value) => IsDiscord(value) && Uri.TryCreate(value, UriKind.Absolute, out var uri)
        && string.IsNullOrEmpty(uri.Query) && string.IsNullOrEmpty(uri.Fragment)
        && System.Text.RegularExpressions.Regex.IsMatch(uri.AbsolutePath, @"^/channels/(@me|[0-9]+)(/[0-9]+)?$");`);
const flagLine=s.split('\n').find(line=>line.includes('if (preferences.CompactMemory && !baselineEngine) options.AdditionalBrowserArguments'));
replace(flagLine,flagLine+'\n            if (preferences.ReleaseUnusedResources && !baselineEngine) options.AdditionalBrowserArguments += " --disable-features=MemoryCacheStrongReference";');
replace('core.NavigationStarting += OnNavigationStarting;', 'core.NavigationStarting += OnNavigationStarting;\n            core.SourceChanged += (_, _) => RememberChannel(core.Source);');
replace('core.Navigate("https://discord.com/app");','core.Navigate(ReadStartupChannel());');
replace('private void OnNavigationStarting(object? sender, CoreWebView2NavigationStartingEventArgs e)',`private string ReadStartupChannel()
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
    private void OnNavigationStarting(object? sender, CoreWebView2NavigationStartingEventArgs e)`);
replace('compactMemoryRequested = preferences.CompactMemory && !baselineEngine, leanInterfaceRequested', 'compactMemoryRequested = preferences.CompactMemory && !baselineEngine, unusedResourceCacheDisabled = preferences.ReleaseUnusedResources && !baselineEngine, leanInterfaceRequested');
replace('["discordHttps"] = NavigationPolicy.IsDiscord("https://discord.com/app"),', `["discordHttps"] = NavigationPolicy.IsDiscord("https://discord.com/app"),
            ["restoreChannel"] = NavigationPolicy.IsRestorableChannel("https://discord.com/channels/123/456"),
            ["restoreDm"] = NavigationPolicy.IsRestorableChannel("https://discord.com/channels/@me/456"),
            ["restoreFriends"] = NavigationPolicy.IsRestorableChannel("https://discord.com/channels/@me"),
            ["rejectRestoredScript"] = !NavigationPolicy.IsRestorableChannel("javascript:alert(1)"),
            ["rejectRestoredQuery"] = !NavigationPolicy.IsRestorableChannel("https://discord.com/channels/123/456?redirect=bad"),
            ["rejectRestoredSuffix"] = !NavigationPolicy.IsRestorableChannel("https://discord.com.evil.test/channels/123/456"),
            ["rejectRestoredCommercial"] = !NavigationPolicy.IsRestorableChannel("https://discord.com/shop"),`);
fs.writeFileSync(file,s);
let project=root+'/src/NotDiscord/NotDiscord.csproj';
fs.writeFileSync(project,fs.readFileSync(project,'utf8').replace('<Version>0.2.0</Version>','<Version>0.3.0</Version>'));
console.log('v0.3 cache policy and private channel restoration applied.');
