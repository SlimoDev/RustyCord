'use strict';
const fs=require('fs'),path=require('path'),crypto=require('crypto'),assert=require('assert/strict'),vm=require('vm');
const root=path.resolve(__dirname,'..'),native=path.join(root,'native-candidate');
const stamp=new Date().toISOString().replace(/[-:.]/g,'');
const backup=path.join(native,'backups','pre-heap-growth-'+stamp);
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
const paths=['host/index.js','host/media-selftest.cjs','runtime/resources/app.asar'];
const originals=paths.map(p=>({path:p,bytes:fs.readFileSync(path.join(native,p))}));
let entry=originals[0].bytes.toString('utf8'),media=originals[1].bytes.toString('utf8');
function one(source,from,to){assert.equal(source.split(from).length,2,'Anchor is not unique');return source.replace(from,()=>to);}
entry=one(entry,"'--flush-code-based-on-time','--bytecode-old-time=60'];","'--flush-code-based-on-time','--bytecode-old-time=60','--heap-growing-percent=10'];");
entry=one(entry,'flush-code-based-on-time|bytecode-old-time)(?:=|$)','flush-code-based-on-time|bytecode-old-time|heap-growing-percent)(?:=|$)');
media=one(media,'--flush-code-based-on-time --bytecode-old-time=60\'','--flush-code-based-on-time --bytecode-old-time=60 --heap-growing-percent=10\'');
new vm.Script(entry);new vm.Script(media);
fs.mkdirSync(backup,{recursive:true});
for(const item of originals){const output=path.join(backup,item.path);fs.mkdirSync(path.dirname(output),{recursive:true});fs.writeFileSync(output,item.bytes);}
fs.writeFileSync(path.join(native,paths[0]),entry);fs.writeFileSync(path.join(native,paths[1]),media);
const report={utc:new Date().toISOString(),setting:'--heap-growing-percent=10',previousConservativeGrowthMax:1.3,growthFactor:1.1,forcedGc:false,hardHeapLimit:false,runtimeApplied:false,notes:'Natural GC allocation threshold; a smaller growth margin may increase CPU/pauses. Does not guarantee resident savings. V8 14.8.178.38 exact source checked.',sources:['https://raw.githubusercontent.com/v8/v8/14.8.178.38/src/heap/heap-controller.cc','https://raw.githubusercontent.com/v8/v8/14.8.178.38/src/execution/isolate.h'],backupRelative:path.relative(root,backup),artifacts:originals.map(item=>({path:item.path,beforeSha256:hash(item.bytes),afterSha256:hash(fs.readFileSync(path.join(native,item.path)))}))};
fs.writeFileSync(path.join(root,'evidence/native/heap-growth-policy.json'),JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({prepared:true,syntaxChecked:true,runtimeApplied:false,backup:report.backupRelative}));
