const fs=require('node:fs');
const root='E:/DESCARGAS/REA/NotDiscord';
const file=root+'/src/NotDiscord/Program.cs';
let s=fs.readFileSync(file,'utf8').replace(/\r\n/g,'\n');
fs.mkdirSync(root+'/build/v04-backup',{recursive:true});
fs.copyFileSync(file,root+'/build/v04-backup/Program.cs');
function replace(a,b){if(!s.includes(a))throw Error('Missing '+a.slice(0,100));s=s.replace(a,b);}
replace('if (preferences.LeanInterface && !baselineEngine) await core.AddScriptToExecuteOnDocumentCreatedAsync(LeanInterface.Script);', `if (preferences.LeanInterface && !baselineEngine) {
                await core.AddScriptToExecuteOnDocumentCreatedAsync(CommercialPreload.Script);
                await core.AddScriptToExecuteOnDocumentCreatedAsync(LeanInterface.Script);
            }`);
replace('using var leanState = JsonDocument.Parse(await view.CoreWebView2.ExecuteScriptAsync(LeanInterface.StateScript));', 'using var leanState = JsonDocument.Parse(await view.CoreWebView2.ExecuteScriptAsync(LeanInterface.StateScript));\n            using var commercialState = JsonDocument.Parse(await view.CoreWebView2.ExecuteScriptAsync(CommercialPreload.StateScript));');
replace('var report = new { leanInterface =', 'var report = new { version = Application.ProductVersion, commercialPreload = commercialState.RootElement.Clone(), leanInterface =');
replace('FinishSmoke(success, new { extendedSmoke,', 'using var commercialState = JsonDocument.Parse(await view.CoreWebView2.ExecuteScriptAsync(CommercialPreload.StateScript));\n        FinishSmoke(success, new { commercialPreload = commercialState.RootElement.Clone(), extendedSmoke,');
fs.writeFileSync(file,s);
const proj=root+'/src/NotDiscord/NotDiscord.csproj';fs.writeFileSync(proj,fs.readFileSync(proj,'utf8').replace('<Version>0.4.0</Version>','<Version>0.5.0</Version>'));
console.log('Integrated guarded Nitro idle-preload patch and aggregate diagnostics.');
