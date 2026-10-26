'use strict';
const fs=require('fs'),path=require('path'),assert=require('assert/strict'),vm=require('vm'),crypto=require('crypto');
const root=path.resolve(__dirname,'..'),native=path.join(root,'native-candidate'),backup=path.join(native,'backups','before-v8-page-pool');
const edits=[];
for(const rel of ['host/index.js','host/media-selftest.cjs']){
 const p=path.join(native,rel),before=fs.readFileSync(p,'utf8');let after=before;
 assert(!before.includes('--no-memory-pool'));
 if(rel.endsWith('index.js')){
  const anchor="'--bytecode-old-time=60'];";assert.equal(after.split(anchor).length,2);
  after=after.replace(anchor,"'--bytecode-old-time=60','--no-memory-pool'];");
  const re='|bytecode-old-time)(?:=|$)';assert.equal(after.split(re).length,2);
  after=after.replace(re,'|bytecode-old-time|memory-pool)(?:=|$)');
 }else{
  const anchor="--bytecode-old-time=60');";assert.equal(after.split(anchor).length,2);
  after=after.replace(anchor,"--bytecode-old-time=60 --no-memory-pool');");
 }
 new vm.Script(after);edits.push({rel,p,before,after});
}
for(const e of edits){const p=path.join(backup,e.rel);fs.mkdirSync(path.dirname(p),{recursive:true});assert(!fs.existsSync(p));fs.writeFileSync(p,e.before);fs.writeFileSync(e.p,e.after)}
fs.writeFileSync(path.join(root,'evidence/native/v8-page-pool-experiment.json'),JSON.stringify({utc:new Date().toISOString(),flag:'--no-memory-pool',meaning:'Return already-free V8 pages to the allocator instead of retaining them in the reusable page pool. Does not cap live heap or force a GC.',sourceVersion:'14.8.178.38',sourceFiles:['evidence/public-web/v8-flags-14.8.178.38.h','evidence/public-web/v8-memory-allocator-14.8.178.38.cc','evidence/public-web/v8-memory-pool-14.8.178.38.cc'],potentialCost:'More allocation/deallocation calls during subsequent activity; requires CPU and functional validation',artifacts:edits.map(e=>({path:e.rel,sha256:crypto.createHash('sha256').update(e.after).digest('hex')})),benchmarkPending:true},null,2));
console.log('V8 free-page pool trial prepared');
