const fs=require('node:fs');
const root='E:/DESCARGAS/REA/NotDiscord';
const file=root+'/src/NotDiscord/Program.cs';
let s=fs.readFileSync(file,'utf8').replace(/\r\n/g,'\n');
function replace(a,b){if(!s.includes(a))throw Error('Missing '+a.slice(0,100));s=s.replace(a,b);}
replace('public bool ReleaseUnusedResources { get; set; } = true;', 'public bool ReleaseUnusedResources { get; set; } = true;\n    public bool CompressSources { get; set; } = true;');
let lines=s.split('\n');let idx=lines.findIndex(line=>line.includes('if (preferences.CompactMemory && !baselineEngine) options.AdditionalBrowserArguments'));
if(idx<0||!lines[idx+1].includes('MemoryCacheStrongReference'))throw Error('Missing flags');
lines.splice(idx,2,'            options.AdditionalBrowserArguments = EnginePolicy.BuildArguments(preferences.CompactMemory, preferences.ReleaseUnusedResources, preferences.CompactMemory && preferences.CompressSources, baselineEngine);');s=lines.join('\n');
replace('unusedResourceCacheDisabled = preferences.ReleaseUnusedResources && !baselineEngine,', 'unusedResourceCacheDisabled = preferences.ReleaseUnusedResources && !baselineEngine, sourceCompressionRequested = preferences.CompactMemory && preferences.CompressSources && !baselineEngine, sourceCacheLimitMiBPerRenderer = EnginePolicy.SourceCacheLimitMiBPerRenderer,');
replace('foreach (var check in TelemetryPolicy.SelfTestResults()) checks[check.Key] = check.Value;', 'foreach (var check in TelemetryPolicy.SelfTestResults()) checks[check.Key] = check.Value;\n        foreach (var check in EnginePolicy.SelfTestResults()) checks[check.Key] = check.Value;');
fs.writeFileSync(file,s);
const proj=root+'/src/NotDiscord/NotDiscord.csproj';fs.writeFileSync(proj,fs.readFileSync(proj,'utf8').replace('<Version>0.3.0</Version>','<Version>0.4.0</Version>'));
console.log('Integrated bounded source compression.');
