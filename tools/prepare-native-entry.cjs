'use strict';
const fs=require('fs'), path=require('path'), assert=require('assert/strict'), crypto=require('crypto'), vm=require('vm');
const {unpack,repack}=require('./prepare-native-bootstrap.cjs');
const root=path.resolve(__dirname,'..'), native=path.join(root,'native-candidate'), resources=path.join(native,'runtime','resources');
function hash(b){return crypto.createHash('sha256').update(b).digest('hex')}
function safeWrite(file,bytes){
 const relative=path.relative(native,file);assert(relative&&!relative.startsWith('..')&&!path.isAbsolute(relative));
 for(let current=path.resolve(file);;current=path.dirname(current)){if(fs.existsSync(current))assert(!fs.lstatSync(current).isSymbolicLink(),'Symlink forbidden');if(current===path.dirname(current))break;}
 fs.mkdirSync(path.dirname(file),{recursive:true});fs.writeFileSync(file,bytes);
}
const configFile=path.join(resources,'build_info.json');
const config=JSON.parse(fs.readFileSync(configFile,'utf8'));
config.standaloneModules=true;config.disableUpdater=true;
safeWrite(configFile,JSON.stringify(config,null,2)+'\n');
const pluginNames=JSON.parse(fs.readFileSync(path.join(root,'config','vencord-plugins.json'),'utf8'));
assert.equal(pluginNames.length,26);assert(pluginNames.every(n=>/^[A-Za-z0-9]+$/.test(n)));
const plugins=Object.fromEntries(pluginNames.map(n=>[n,{enabled:true}]));plugins.NoTrack.disableAnalytics=true;
const settings={autoUpdate:false,autoUpdateNotification:false,enableReactDevtools:false,cloud:{authenticated:false,settingsSync:false},plugins};
const settingsPath=path.join(native,'vencord-data','settings','settings.json');
if(!fs.existsSync(settingsPath))safeWrite(settingsPath,JSON.stringify(settings,null,2)+'\n');
const nativeSettings=path.join(native,'vencord-data','settings','native-settings.json');
if(!fs.existsSync(nativeSettings))safeWrite(nativeSettings,JSON.stringify({plugins:{},customCspRules:{}},null,2)+'\n');
const entry=fs.readFileSync(path.join(native,'host','index.js'),'utf8').replace(/^\uFEFF/,'');
new vm.Script(entry,{filename:'NotDiscordNative-entry.syntax-only.js'});
const pkg=JSON.stringify({name:'notdiscord-native',productName:'NotDiscord',version:'0.1.0',main:'index.js',private:true});
const packed=repack({header:{files:{'package.json':{size:0,offset:'0'},'index.js':{size:0,offset:'0'}}},files:new Map([['package.json',{bytes:Buffer.from(pkg)}],['index.js',{bytes:Buffer.from(entry)}]])});
const check=unpack(packed);assert.equal(check.files.get('index.js').bytes.toString('utf8'),entry);assert.equal(JSON.parse(check.files.get('package.json').bytes).main,'index.js');
safeWrite(path.join(resources,'app.asar'),packed);
const executable=path.join(native,'runtime','NotDiscord.exe'), originalName=path.join(native,'runtime','Discord.exe');
if(!fs.existsSync(executable)){assert(fs.existsSync(originalName));assert.equal(hash(fs.readFileSync(originalName)),'a3b85524ac280f887ea77bc3d25f53143e1521f891d6863442c74f38a4564bfa');fs.renameSync(originalName,executable);}
assert.equal(hash(fs.readFileSync(executable)),'f92e752e394cfb28901e67321f7aa5205113643c11c0737c8e71b0c97adc671a','Expected reviewed NotDiscord VERSIONINFO branding; see prepare-native-branding.py');
safeWrite(path.join(native,'ABRIR_NOTDISCORD_NATIVO.cmd'),'@echo off\r\nstart "" "%~dp0runtime\\NotDiscord.exe"\r\n');
const evidence={preparedUtc:new Date().toISOString(),entrySha256:hash(Buffer.from(entry)),entryAsarSha256:hash(packed),executableSha256:hash(fs.readFileSync(executable)),pluginNames,profileCopied:false,standaloneModules:config.standaloneModules,disableUpdater:config.disableUpdater,executed:false,featureParityVerified:false,targetSavingsVerified:false};
fs.mkdirSync(path.join(root,'evidence','native'),{recursive:true});fs.writeFileSync(path.join(root,'evidence','native','entry-policy.json'),JSON.stringify(evidence,null,2)+'\n');
console.log(JSON.stringify({prepared:true,executable,entryAsarSha256:evidence.entryAsarSha256,seedPlugins:pluginNames.length,executed:false}));
