// REJECTED: this experiment sends critical memory pressure. Keep for evidence only.
throw new Error("Rejected experiment: see evidence/native/resource-cache-experiment-rejected.json");
'use strict';
const fs=require('fs'),path=require('path'),assert=require('assert/strict'),vm=require('vm');
const root=path.resolve(__dirname,'..'),native=path.join(root,'native-candidate');
for(const rel of ['host/index.js','host/media-selftest.cjs']){
 const p=path.join(native,rel);let s=fs.readFileSync(p,'utf8');
 s=rel.endsWith('index.js')?s.replaceAll(",'PartitionAllocLargeThreadCacheSize'",''):s.replaceAll(',PartitionAllocLargeThreadCacheSize','');
 new vm.Script(s);fs.writeFileSync(p,s);
}
const pre=path.join(native,'vencord/dist/preload.js');
const original=fs.readFileSync(path.join(native,'backups/memory-cache-20261018/vencord/dist/preload.js'),'utf8');
const s=original+"\n;require(require('node:path').join(__dirname,'..','..','host','renderer-cache-policy.cjs'))({webFrame:t.webFrame,ipcRenderer:t.ipcRenderer,origin:location.origin,isTop:window.top===window.self,enabled:true});\n";
new vm.Script(s);fs.writeFileSync(pre,s);
const diag=path.join(native,'host/memory-diagnostics.cjs');let d=fs.readFileSync(diag,'utf8');
const anchor="electron.app.on('browser-window-created',(_e,win)=>watch(win));";
assert.equal(d.split(anchor).length,2);
d=d.replace(anchor,anchor+"\n electron.ipcMain.on('notdiscord:resource-sweep-result',(event,data)=>{\n  if(!states.has(event.sender.id)||event.sender.isDestroyed())return;\n  const clean={};\n  for(const k of ['enabled','attempted','cleared'])clean[k]=data?.[k]===true;\n  clean.reason=['not-attempted','below-threshold','not-ready-or-voice-active','unused-resource-cache','error'].includes(data?.reason)?data.reason:'invalid';\n  for(const phase of ['before','after']){\n   const v=data?.[phase];if(!v)continue;\n   clean[phase]={unusedBytes:Number.isFinite(v.unusedBytes)&&v.unusedBytes>=0?v.unusedBytes:null,resources:{}};\n   for(const kind of ['images','cssStyleSheets','xslStyleSheets','fonts','other','scripts']){\n    const item=v.resources?.[kind];if(!item)continue;clean[phase].resources[kind]={};\n    for(const k of ['count','size','liveSize'])if(Number.isFinite(item[k])&&item[k]>=0)clean[phase].resources[kind][k]=item[k];\n   }\n  }\n  save('resource-cache-sweep-'+process.pid+'.json',clean);\n });\n");new vm.Script(d);fs.writeFileSync(diag,d);
fs.writeFileSync(path.join(root,'evidence/native/resource-cache-experiment.json'),JSON.stringify({utc:new Date().toISOString(),threadCacheReductionReverted:true,threadCacheResult:'No demonstrated saving: before 375.89 MiB, after 381.13 MiB; unequal startup age, inconclusive',resourceSweep:{enabled:true,oncePerStartup:true,settleSeconds:120,unusedThresholdMiB:16,skipIfVoiceActive:true,skipIfVoiceApiUnavailable:true},manualGc:false,workingSetTrim:false,moduleRemoval:false,benchmarkPending:true},null,2));
console.log('Resource cache experiment installed; thread-cache trial reverted');
