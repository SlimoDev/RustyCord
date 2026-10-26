'use strict';
const fs=require('fs'),path=require('path'),assert=require('assert/strict'),crypto=require('crypto'),vm=require('vm');
const root=path.resolve(__dirname,'..'),native=path.join(root,'native-candidate'),backup=path.join(native,'backups','memory-cache-20261018');
fs.mkdirSync(backup,{recursive:true});
function edit(rel,fn){const file=path.join(native,rel),old=fs.readFileSync(file,'utf8'),next=fn(old);new vm.Script(next);const b=path.join(backup,rel);fs.mkdirSync(path.dirname(b),{recursive:true});if(!fs.existsSync(b))fs.writeFileSync(b,old);fs.writeFileSync(file,next);return{path:rel,before:sha(old),after:sha(next)}}
function sha(s){return crypto.createHash('sha256').update(s).digest('hex')}
const records=[];
records.push(edit('host/index.js',s=>{
 const a="'PartitionAllocAdjustSizeWhenInForeground']";assert.equal(s.split(a).length,3);
 return s.split(a).join("'PartitionAllocAdjustSizeWhenInForeground','PartitionAllocLargeThreadCacheSize']");
}));
records.push(edit('host/media-selftest.cjs',s=>{const a='PartitionAllocAdjustSizeWhenInForeground';assert.equal(s.split(a).length,2);return s.replace(a,a+',PartitionAllocLargeThreadCacheSize')}));
records.push(edit('vencord/dist/preload.js',s=>{
 assert(!s.includes('notdiscord:resource-usage-request'));
 return s+"\n;(() => {\n const {ipcRenderer,webFrame}=require('electron/renderer');\n if(location.origin!=='https://discord.com'||window.top!==window.self)return;\n ipcRenderer.on('notdiscord:resource-usage-request',()=>{\n  const raw=webFrame.getResourceUsage(),result={};\n  for(const kind of ['images','cssStyleSheets','xslStyleSheets','fonts','other','scripts']){\n   const v=raw[kind];if(!v)continue;result[kind]={};\n   for(const key of ['count','size','liveSize'])if(Number.isFinite(v[key])&&v[key]>=0)result[kind][key]=v[key];\n  }\n  ipcRenderer.send('notdiscord:resource-usage-response',result);\n });\n})();\n";
}));
records.push(edit('host/memory-diagnostics.cjs',s=>{
 const a="data.aggregate=await wc.executeJavaScript(expression);";
 assert.equal(s.split(a).length,2);
 s=s.replace(a,a+"data.resourceCache=await resourceUsage(electron,wc);");
 return s+"\nasync function resourceUsage(electron,wc){\n return new Promise(resolve=>{\n  const channel='notdiscord:resource-usage-response';\n  const finish=value=>{clearTimeout(timer);electron.ipcMain.removeListener(channel,onResult);resolve(value)};\n  const onResult=(event,raw)=>{\n   if(event.sender!==wc)return;\n   const data={};\n   for(const kind of ['images','cssStyleSheets','xslStyleSheets','fonts','other','scripts']){\n    const item=raw?.[kind];if(!item)continue;data[kind]={};\n    for(const key of ['count','size','liveSize'])if(Number.isFinite(item[key])&&item[key]>=0)data[kind][key]=item[key];\n   }\n   finish(data);\n  };\n  const timer=setTimeout(()=>finish({unavailable:true}),4000);\n  electron.ipcMain.on(channel,onResult);wc.send('notdiscord:resource-usage-request');\n });\n}\n";
}));
const binary=fs.readFileSync(path.join(native,'runtime','NotDiscord.exe'));
assert(binary.includes(Buffer.from('PartitionAllocLargeThreadCacheSize\0')));
fs.writeFileSync(path.join(root,'evidence/native/thread-cache-experiment.json'),JSON.stringify({utc:new Date().toISOString(),feature:'PartitionAllocLargeThreadCacheSize',binaryContainsFeatureName:true,change:'Disable larger renderer thread allocation cache; live objects, WebRTC, JIT and sandbox retained',hypothesis:'Reduce unused native memory retained per renderer thread; may increase allocation CPU cost',records,benchmarkPending:true},null,2));
console.log(JSON.stringify({prepared:true,records}));
