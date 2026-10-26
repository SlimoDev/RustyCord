const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const root='E:/DESCARGAS/REA/NotDiscord',base='C:/Users/sssli/AppData/Local/Discord/app-1.0.9262';
const queries=['BrowserWindow','backgroundThrottling','loadURL','discord.com','mainScreenPreload','DiscordNative','discord_voice','discord_krisp','discord_rpc','discord_desktop_overlay','hardwareAcceleration','disableHardwareAcceleration'];
const result=[];
for (const [name,file] of [['bootstrap',base+'/resources/_app.asar'],['core',base+'/modules/discord_desktop_core-1/discord_desktop_core/core.asar']]) {
 const b=fs.readFileSync(file),h=JSON.parse(b.subarray(16,16+b.readUInt32LE(12))),offset=8+b.readUInt32LE(4);
 for(const [entry,v] of Object.entries(h.files)){if(!/\.js$/.test(entry)||v.offset===undefined)continue;const code=b.subarray(offset+Number(v.offset),offset+Number(v.offset)+v.size).toString('utf8');for(const q of queries){let start=0,count=0;while((start=code.indexOf(q,start))!==-1){count++;if(count<=3)result.push({artifact:name,file:entry,query:q,utf16Offset:start,excerpt:code.slice(Math.max(0,start-200),start+q.length+450)});start+=q.length;}} }
}
fs.writeFileSync(root+'/evidence/original/focused-excerpts.json',JSON.stringify(result,null,2));
for(const q of ['BrowserWindow','backgroundThrottling','loadURL','DiscordNative','discord_voice','hardwareAcceleration']){console.log(JSON.stringify({q,matches:result.filter(x=>x.query===q)}));}
const modules=[];function walk(dir){for(const e of fs.readdirSync(dir,{withFileTypes:true})){const f=path.join(dir,e.name);if(e.isDirectory())walk(f);else if(/\.(node|dll)$/.test(e.name))modules.push({path:path.relative(base,f),bytes:fs.statSync(f).size});}}walk(base+'/modules');fs.writeFileSync(root+'/evidence/original/native-module-inventory.json',JSON.stringify(modules,null,2));console.log(JSON.stringify({nativeFiles:modules.length,totalMiB:modules.reduce((a,x)=>a+x.bytes,0)/1048576,largest:modules.sort((a,b)=>b.bytes-a.bytes).slice(0,8)}));
