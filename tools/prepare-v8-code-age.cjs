'use strict';
const fs=require('fs'),path=require('path'),assert=require('assert/strict'),vm=require('vm'),crypto=require('crypto');
const root=path.resolve(__dirname,'..'),native=path.join(root,'native-candidate'),backup=path.join(native,'backups','before-code-age-8');
const edits=[];
for(const rel of ['host/index.js','host/media-selftest.cjs']){
 const p=path.join(native,rel),before=fs.readFileSync(p,'utf8');
 assert.equal(before.split('--bytecode-old-time=60').length,2);
 const after=before.replace('--bytecode-old-time=60','--bytecode-old-time=8');
 new vm.Script(after);edits.push({rel,p,before,after});
}
for(const e of edits){const p=path.join(backup,e.rel);assert(!fs.existsSync(p));fs.mkdirSync(path.dirname(p),{recursive:true});fs.writeFileSync(p,e.before);fs.writeFileSync(e.p,e.after)}
fs.writeFileSync(path.join(root,'evidence/native/v8-code-age-experiment.json'),JSON.stringify({
 utc:new Date().toISOString(),flag:'--bytecode-old-time=8',before:60,
 meaning:'Eligible unused bytecode and baseline code may be discarded at ordinary V8 collections earlier. No forced GC or memory-pressure notification.',
 sourceVersion:'14.8.178.38',sourceFiles:['evidence/public-web/v8-flags-14.8.178.38.h','evidence/public-web/v8-marking-visitor-14.8.178.38.h'],
 potentialCost:'Inactive functions may need recompilation on later use; measure CPU and functionality.',
 artifacts:edits.map(e=>({path:e.rel,sha256:crypto.createHash('sha256').update(e.after).digest('hex')})),benchmarkPending:true
},null,2));
console.log('Prepared code age 8-second trial');
