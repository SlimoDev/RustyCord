'use strict';
const fs=require('fs'),path=require('path'),assert=require('assert/strict'),crypto=require('crypto'),vm=require('vm');
const {unpack,repack}=require('./prepare-native-bootstrap.cjs'),{parse,entryReach}=require('./prepare-native-sentry-imports.cjs');
const root=path.resolve(__dirname,'..'),native=path.join(root,'native-candidate');
const file=path.join(native,'runtime/resources/standalone_modules/discord_desktop_core/core.asar'),input=fs.readFileSync(file);
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
assert.equal(hash(input),'aebff02e69167ea6f50cf777cbd4dac7d60431fa25444c93d233779115a75e1b');
const archive=unpack(input),originals=new Map([...archive.files].map(([n,f])=>[n,Buffer.from(f.bytes)]));
const member='mainScreenPreload.js',source=archive.files.get(member).bytes.toString('utf8');
assert.equal(hash(source),'e27a027c93ccebb24e1cd97b4205b0041ef3e31bb17a3fa44f73fdc2af3696ff');
const g=parse(source),reachable=entryReach(g);
assert.equal(g.dynamic.length,0);
const previous=JSON.parse(fs.readFileSync(path.join(root,'evidence/native/sentry-imports-policy.json'),'utf8')).analysis.find(x=>x.member===member);
const remove=new Set(previous.allLiteralDependencyReachability.noLongerReachableFactoryIds);
assert.equal(remove.size,300);for(const id of remove){assert(g.modules.has(id));assert(!reachable.has(id))}
let argumentPasses=0;
function walk(n,p){
 if(!n||typeof n!=='object')return;
 if(n.type==='Identifier'&&n.name==='__webpack_require__'){
  if(p.type==='FunctionExpression')assert(p.params.includes(n));
  else if(p.type==='FunctionDeclaration')assert.equal(p.id,n);
  else if(p.type==='MemberExpression')assert.equal(p.property.name,'nmd');
  else if(p.type==='CallExpression'){
   if(p.callee!==n){
    argumentPasses++;assert.equal(source.slice(p.start,p.end),'__webpack_modules__[moduleId].call(module1.exports,module1,module1.exports,__webpack_require__)');
   }
  }else throw Error('Unreviewed loader escape: '+p.type);
 }
 for(const[k,v]of Object.entries(n)){if(k==='start'||k==='end')continue;if(Array.isArray(v))v.forEach(x=>walk(x,n));else if(v&&typeof v==='object')walk(v,n)}
}
walk(g.ast,null);assert.equal(argumentPasses,1);
const kept=[...g.modules.values()].filter(x=>!remove.has(x.id));
const output=source.slice(0,g.table.start+1)+kept.map(x=>source.slice(x.node.start,x.node.end)).join(',')+source.slice(g.table.end-1);
new vm.Script(output,{filename:member});
const after=parse(output);assert.deepEqual([...entryReach(after)].sort(),[...reachable].sort());assert.equal(after.dynamic.length,0);
for(const m of kept)assert.equal(after.modules.get(m.id).sha256,m.sha256);
for(const e of after.edges)assert(!remove.has(e.to));
archive.files.get(member).bytes=Buffer.from(output);
const packed=repack(archive,archive.files.get('bundle.js').bytes.toString('utf8')),round=unpack(packed);
for(const[name,bytes]of originals)assert(round.files.get(name).bytes.equals(name===member?Buffer.from(output):bytes),'Other archive member changed: '+name);
const dest=path.join(native,'prepared-memory/core.asar');fs.mkdirSync(path.dirname(dest),{recursive:true});fs.writeFileSync(dest,packed);
const report={utc:new Date().toISOString(),staged:true,applied:false,checksPassed:true,sourceSha256:hash(input),outputSha256:hash(packed),member,beforeSourceBytes:Buffer.byteLength(source),afterSourceBytes:Buffer.byteLength(output),factoriesBefore:g.modules.size,factoriesAfter:after.modules.size,removedFactories:[...remove],reachableFactories:reachable.size,dynamicCalls:0,loaderEscapes:0,unchangedMembers:[...originals.keys()].filter(x=>x!==member).length,explanation:'Remove only the previously documented 300 Sentry dependency factories after root imports were disabled. All remaining factories, reachable graph and all other ASAR members preserved.',limits:'Static proof for the pinned artifact only. Runtime, native voice, screen sharing, Vencord and total resident RAM require checks.'};
fs.writeFileSync(path.join(root,'evidence/native/preload-dead-code-prepared.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({...report,removedFactories:remove.size}));
