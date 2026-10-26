'use strict';
const fs=require('fs'),path=require('path'),crypto=require('crypto'),assert=require('assert/strict');
const {unpack}=require('./prepare-native-bootstrap.cjs');
const root=path.resolve(__dirname,'..'),native=path.join(root,'native-candidate');
const hash=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const expected={
'runtime/NotDiscord.exe':'f92e752e394cfb28901e67321f7aa5205113643c11c0737c8e71b0c97adc671a',
'runtime/resources/app.asar':'97055df1d64007b2e8343b516eb2fcdad3fa443a8bc27df3056d8a5cb6411ef6',
'runtime/resources/_app.asar':'7fcb82aa46e8b713bf2087d04fdbd4d39d5d45fdcfd6f7f0ef49a768c9fa6e75',
'runtime/resources/standalone_modules/discord_desktop_core/core.asar':'5d34b937dd8c5227f700729a6df2785b8636d9aee6edb352b319534439617ac3',
'host/index.js':'c3c33da1689fb947016fb7018a2eb22ac51f4e39172d7176fdb24d793986e073',
'vencord/dist/renderer.js':'e1283ec8d13bbbbbd1b2fbc6d3ea5e60eb7a9fbf314dd077644dcdb41f285f08'};
const actual={};
for(const [rel,digest] of Object.entries(expected)){actual[rel]=hash(path.join(native,rel));assert.equal(actual[rel],digest,rel)}
const entry=fs.readFileSync(path.join(native,'host/index.js'),'utf8');
assert.equal(unpack(fs.readFileSync(path.join(native,'runtime/resources/app.asar'))).files.get('index.js').bytes.toString('utf8'),entry);
const preload=fs.readFileSync(path.join(native,'vencord/dist/preload.js'),'utf8');
assert(preload.includes('renderer-resource-diagnostics.cjs'));assert(!preload.includes('renderer-cache-policy.cjs'));
const diag=fs.readFileSync(path.join(native,'host/renderer-resource-diagnostics.cjs'),'utf8');
assert(!/clearCache\s*\(|setInterval\s*\(|setTimeout\s*\(|notifyMemoryPressure\s*\(/.test(diag));
const plugins=JSON.parse(fs.readFileSync(path.join(root,'config/vencord-plugins.json'),'utf8'));
const probe=JSON.parse(fs.readFileSync(path.join(root,'evidence/native/memory-code-age8-matched-probe.json'),'utf8'));
assert.equal(plugins.length,26);assert(plugins.every(n=>probe.result.enabledPlugins.includes(n)));
const modules=fs.readdirSync(path.join(native,'runtime/resources/standalone_modules')).filter(n=>n.startsWith('discord_'));
assert.equal(modules.length,17);
assert.equal(hash('C:/Users/sssli/AppData/Local/Discord/app-1.0.9262/Discord.exe'),'a3b85524ac280f887ea77bc3d25f53143e1521f891d6863442c74f38a4564bfa');
const report={utc:new Date().toISOString(),passed:true,actual,originalExecutableUnchanged:true,modules,requestedPlugins:plugins.length,allRequestedPluginsEnabled:true,extraCrashHandlerEnabled:probe.result.enabledPlugins.includes('CrashHandler'),autoResourceClearing:false,forcedGC:false,workingSetTrimming:false,realCallAfterLatestPatchConfirmed:false};
fs.writeFileSync(path.join(root,'evidence/native/memory-current-build-audit.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({passed:true,modules:modules.length,requestedPlugins:plugins.length,hashesVerified:Object.keys(actual).length,originalUnchanged:true}));
