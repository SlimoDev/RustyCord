'use strict';
const fs=require('fs'),path=require('path'),assert=require('assert/strict'),crypto=require('crypto'),vm=require('vm');
const {unpack,repack}=require('./prepare-native-bootstrap.cjs'),{parse,entryReach}=require('./prepare-native-sentry-imports.cjs');
const root=path.resolve(__dirname,'..'),native=path.join(root,'native-candidate'),hash=x=>crypto.createHash('sha256').update(x).digest('hex');
const input=fs.readFileSync(path.join(native,'runtime/resources/_app.asar'));
assert.equal(hash(input),'f160d80b01eea8cbd4206dc40f6739e6ca71e2748efdda5a36ef4cd17f1b1d3b');
const archive=unpack(input),originals=new Map([...archive.files].map(([n,f])=>[n,Buffer.from(f.bytes)]));
const source=archive.files.get('bundle.js').bytes.toString('utf8');
function walk(n,fn,p=null){if(!n||typeof n!=='object')return;if(n.type)fn(n,p);for(const[k,v]of Object.entries(n)){if(k==='start'||k==='end')continue;if(Array.isArray(v))v.forEach(x=>walk(x,fn,n));else if(v&&typeof v==='object')walk(v,fn,n)}}
function graph(s){
 const g=parse(s);
 for(const id of [24836,71804,80345]){
  const m=g.modules.get(id),text=s.slice(m.node.start,m.node.end);
  const match=/var map=(\{[^;]+\});function __rspack_context\(req\)/.exec(text);assert(match,'Context map changed');
  assert(text.includes('return __webpack_require__(__rspack_context_resolve(req))'));
  assert(text.includes('if(!__webpack_require__.o(map,req))'));
  assert(text.includes('return map[req]'));
  const vals=Object.values(JSON.parse(match[1]));assert(vals.every(x=>/^\d+$/.test(x)&&g.modules.has(Number(x))));
  m.deps.push(...vals.map(Number));
 }
 let bound=0;
 walk(g.ast,(n,p)=>{
  if(n.type!=='Identifier'||n.name!=='__webpack_require__')return;
  if(p.type==='FunctionExpression'){assert(p.params.includes(n));return}
  if(p.type==='FunctionDeclaration'){assert.equal(p.id,n);return}
  if(p.type==='MemberExpression'){assert(['o','d','r','nmd','t','c','s','n'].includes(p.property.name));return}
  assert.equal(p.type,'CallExpression','Unknown loader escape');
  if(p.callee===n)return;
  const text=s.slice(p.start,p.end);
  if(text==='__webpack_modules__[moduleId].call(module1.exports,module1,module1.exports,__webpack_require__)')return;
  assert.equal(text,'__webpack_require__.t.bind(__webpack_require__,86592,23)');
  const m=[...g.modules.values()].find(x=>p.start>=x.node.start&&p.end<=x.node.end);assert(m);m.deps.push(86592);bound++;
 });
 const allowed=new Set([24836,71804,80345,33121]);assert(g.dynamic.every(x=>allowed.has(x.from)));
 if(g.modules.has(33121)){
  const n=g.dynamic.find(x=>x.from===33121);assert(n);
  assert(s.slice(n.utf16Start,n.utf16Start+180).includes('throw e.code="MODULE_NOT_FOUND",e}())'));
 }
 return g;
}
const before=graph(source),pattern='__webpack_require__(65681).captureException(err)';
assert.equal(source.split(pattern).length,3);
const modified=source.replaceAll(pattern,'void 0'),afterCaptures=graph(modified);
const beforeReach=entryReach(before),afterReach=entryReach(afterCaptures);
const removed=new Set([...beforeReach].filter(x=>!afterReach.has(x)));
assert(removed.has(65681)&&removed.size>=650&&removed.size<=660);
const kept=[...afterCaptures.modules.values()].filter(x=>!removed.has(x.id));
const output=modified.slice(0,afterCaptures.table.start+1)+kept.map(x=>modified.slice(x.node.start,x.node.end)).join(',')+modified.slice(afterCaptures.table.end-1);
new vm.Script(output);
const after=graph(output);assert.deepEqual([...entryReach(after)].sort(),[...afterReach].sort());
for(const m of kept)if(m.id!==37079)assert.equal(after.modules.get(m.id).sha256,before.modules.get(m.id).sha256);
for(const m of after.modules.values())assert(!m.deps.some(x=>removed.has(x)));
assert(after.dynamic.every(x=>[24836,71804,80345].includes(x.from)));
// Exercise the preserved local error UI and quit path with mocks only.
const errorFactory=output.slice(after.modules.get(37079).node.start,after.modules.get(37079).node.end);
let shown=0,quit=0,warning=0;const listeners={},exports1={};
const context=vm.createContext({console:{warn(){warning++},error(){}},global:{releaseChannel:'canary'}});
const fn=vm.runInContext('({'+errorFactory+'})[37079]',context);
fn({},exports1,id=>{if(id===932)return{env:{},on:(k,f)=>listeners[k]=f,exit(){throw Error('Unexpected exit')}};if(id===91288)return{app:{quit(){quit++}},dialog:{showErrorBox(){shown++},showMessageBox(){shown++;return Promise.resolve()}}};throw Error('Unexpected dependency '+id)});
exports1.init();assert.equal(typeof listeners.uncaughtException,'function');listeners.uncaughtException(new Error('local test'));exports1.fatal(new Error('local fatal test'));exports1.handled(new Error('local nonfatal test'));
assert.equal(shown,2);assert(warning>=1);
(async()=>{
 await Promise.resolve();assert.equal(quit,1);
 archive.files.get('bundle.js').bytes=Buffer.from(output);
 const packed=repack(archive,output),round=unpack(packed);
 for(const[n,b]of originals)assert(round.files.get(n).bytes.equals(n==='bundle.js'?Buffer.from(output):b));
 const dst=path.join(native,'prepared-memory/_app.asar');fs.mkdirSync(path.dirname(dst),{recursive:true});fs.writeFileSync(dst,packed);
 const report={utc:new Date().toISOString(),staged:true,applied:false,checksPassed:true,sourceSha256:hash(input),outputSha256:hash(packed),beforeSourceBytes:Buffer.byteLength(source),afterSourceBytes:Buffer.byteLength(output),factoriesBefore:before.modules.size,factoriesAfter:after.modules.size,removedFactories:[...removed],resolvedContextMaps:[24836,71804,80345],preservedReachableFactories:afterReach.size,removedCaptureCalls:2,localErrorDialogAndQuitMockPassed:true,remainingFactoriesByteIdenticalExceptErrorCapture:true,otherArchiveMembersUnchanged:true};
 fs.writeFileSync(path.join(root,'evidence/native/bootstrap-sdk-pruned-prepared.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({...report,removedFactories:removed.size}));
})().catch(e=>{console.error(e);process.exitCode=1});
