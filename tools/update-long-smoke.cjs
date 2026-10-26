const fs=require('node:fs');
const root='E:/DESCARGAS/REA/NotDiscord';
const file=root+'/src/NotDiscord/Program.cs';
let s=fs.readFileSync(file,'utf8').replace(/\r\n/g,'\n');
fs.mkdirSync(root+'/build/v03-backup',{recursive:true});
if(!fs.existsSync(root+'/build/v03-backup/Program.cs'))fs.copyFileSync(file,root+'/build/v03-backup/Program.cs');
function replace(a,b){if(!s.includes(a))throw Error('Missing '+a.slice(0,100));s=s.replace(a,b);}
replace('bool smoke = args.Contains("--smoke-test") || benchmark;', 'bool smoke = args.Contains("--smoke-test") || args.Contains("--long-smoke") || benchmark;');
replace('args.Contains("--measure-session")));', 'args.Contains("--measure-session"), args.Contains("--long-smoke")));');
replace('!args.Contains("--benchmark-smoke")) MessageBox', '!args.Contains("--benchmark-smoke") && !args.Contains("--long-smoke")) MessageBox');
replace('private readonly bool smoke, benchmark, baselineEngine, measureSession;', 'private readonly bool smoke, benchmark, baselineEngine, measureSession, extendedSmoke;');
replace('bool baselineEngine, bool measureSession)\n', 'bool baselineEngine, bool measureSession, bool extendedSmoke)\n');
replace('this.baselineEngine = baselineEngine; this.measureSession = measureSession;', 'this.baselineEngine = baselineEngine; this.measureSession = measureSession; this.extendedSmoke = extendedSmoke;');
replace('watchdog.Interval = benchmark ? 160000 : 58000;', 'watchdog.Interval = (benchmark || extendedSmoke) ? 160000 : 58000;');
replace('limitSeconds = benchmark ? 160 : 58', 'limitSeconds = (benchmark || extendedSmoke) ? 160 : 58');
replace('        JsonElement mediaSelfTest = lifetime.Elapsed.TotalSeconds < 43 ?', `        if (extendedSmoke) {
            paths.WriteJson("evidence/long-smoke-progress.json", new { phase = "waiting-for-source-compression", waitSeconds = 90, utc = DateTime.UtcNow });
            await Task.Delay(90000);
            if (IsDisposed || smokeFinished) return;
        }
        JsonElement mediaSelfTest = lifetime.Elapsed.TotalSeconds < (extendedSmoke ? 145 : 43) ?`);
replace('FinishSmoke(success, new { mediaSelfTest,', 'FinishSmoke(success, new { extendedSmoke, postLoadWaitSeconds = extendedSmoke ? 90 : 0, mediaSelfTest,');
fs.writeFileSync(file,s);
console.log('Added isolated delayed media validation; no authenticated session media capture.');
