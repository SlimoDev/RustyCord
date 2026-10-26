'use strict';
// Static-only preparation. Run: node --expose-internals tools/prepare-native-sentry-imports.cjs
// No installed Discord files, profiles, host entry, active runtime, or SDK exports are modified.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),assert=require('node:assert/strict'),vm=require('node:vm');
let acorn;try{acorn=require('internal/deps/acorn/acorn/dist/acorn');}catch{throw Error('Run this preparer with node --expose-internals; the flag is only for the static parser, never the app.');}
const {unpack,repack}=require('./prepare-native-bootstrap.cjs');
const ROOT=path.resolve(__dirname,'..'),RESOURCES=path.join(ROOT,'native-candidate/runtime/resources'),STAGED=path.join(ROOT,'native-candidate/prepared-sentry-imports');
const hash=x=>crypto.createHash('sha256').update(x).digest('hex');
const MAIN_INIT='crashReporterSetup.init(buildInfo,{sentry,getTransport:dsnFunc=>browser.makeMultiplexedTransport(sentry.makeElectronOfflineTransport(),dsnFunc)})';
const PRELOAD_INIT='crashReporterSetup.init(buildInfo,{sentry,getTransport:dsnFunc=>browser.makeMultiplexedTransport(browser.makeFetchTransport,dsnFunc)})';
const ARTIFACTS=[
 {name:'bootstrap',source:path.join(RESOURCES,'_app.asar'),output:'resources/_app.asar',sha256:'3c760e25272011b8da6f606b7bb7ac21b0e6ee8aa0acb2134736760e1a194039',members:[
  {name:'bundle.js',sha256:'2e615240b83f233fba9875e926a4e0d4d44a866be45bbee3886d4214e46003cd',scope:23151,sdk:[65681,24865],setup:62457,edits:[
   {before:',sentry=__webpack_require__(65681)',after:''},
   {before:',browser=__webpack_require__(24865);crashReporterSetup.init(',after:';crashReporterSetup.init('},
   {before:MAIN_INIT,after:'crashReporterSetup.init(buildInfo)'}]}]},
 {name:'core',source:path.join(RESOURCES,'standalone_modules/discord_desktop_core/core.asar'),output:'resources/standalone_modules/discord_desktop_core/core.asar',sha256:'f5fa959bb770042495ba02ab3fa62e9e3fa9f5251150eab4c4f107f6ccde52f1',members:[
  {name:'mainScreenPreload.js',sha256:'a669dabbb7ccefba825042d3e25c5f90fece411e2ab7ba42bc168dbd63b4c501',scope:'entry',sdk:[9772,5768],setup:2457,edits:[
   {before:',sentry=__webpack_require__(9772),browser=__webpack_require__(5768)',after:''},
   {before:PRELOAD_INIT,after:'crashReporterSetup.init(buildInfo)'}]}]},
 {name:'overlay-preload',kind:'javascript',source:path.join(RESOURCES,'standalone_modules/discord_overlay2/overlayPreload.js'),output:'resources/standalone_modules/discord_overlay2/overlayPreload.js',sha256:'1cf60c58ec379cf740f0b6f548f83e86ad70b17a55808e2d4b7b1e9246f800f1',members:[
  {name:'overlayPreload.js',sha256:'1cf60c58ec379cf740f0b6f548f83e86ad70b17a55808e2d4b7b1e9246f800f1',scope:2787,sdk:[9420,9464],setup:924,edits:[
   {before:',sentry=__webpack_require__(9420),browser=__webpack_require__(9464)',after:''},
   {before:PRELOAD_INIT,after:'crashReporterSetup.init(buildInfo)'}]}]}
];
function walk(node,fn,parent=null){if(!node||typeof node!=='object')return;if(node.type)fn(node,parent);for(const value of Object.values(node)){if(Array.isArray(value))for(const v of value)walk(v,fn,node);else if(value&&typeof value==='object')walk(value,fn,node);}}
function parse(source){
 const ast=acorn.parse(source,{ecmaVersion:'latest',sourceType:'script'});let table;
 walk(ast,n=>{if(n.type==='VariableDeclarator'&&n.id.name==='__webpack_modules__'){assert(!table,'Ambiguous factory table');table=n.init;}});
 assert.equal(table?.type,'ObjectExpression');const modules=new Map();for(const p of table.properties){assert.equal(p.key.type,'Literal');assert(!modules.has(p.key.value));modules.set(p.key.value,{id:p.key.value,node:p,sha256:hash(source.slice(p.start,p.end)),bytes:Buffer.byteLength(source.slice(p.start,p.end)),deps:[],dynamic:[]});}
 const edges=[],dynamic=[];
 // Scan each factory separately. Includes calls in deferred functions: deliberately conservative.
 function calls(root,from,skipTable=false){walk(root,n=>{if(skipTable&&n.start>=table.start&&n.end<=table.end)return;if(n.type!=='CallExpression'||n.callee.name!=='__webpack_require__')return;let arg=n.arguments[0];if(arg?.type==='AssignmentExpression'&&arg.operator==='=')arg=arg.right;const item={from,to:arg?.type==='Literal'?arg.value:null,utf16Start:n.start};if(item.to===null){dynamic.push(item);if(from!=='entry')modules.get(from).dynamic.push(item);}else{edges.push(item);if(from!=='entry')modules.get(from).deps.push(item.to);}});}
 for(const[id,m]of modules)calls(m.node,id);calls(ast,'entry',true);
 const identifiers=[];walk(ast,(n,p)=>{if(n.type==='Identifier'&&['sentry','browser'].includes(n.name))identifiers.push({name:n.name,start:n.start,end:n.end,parent:p?.type});});
 return{ast,table,modules,edges,dynamic,identifiers};
}
function reach(graph,roots){const result=new Set(),pending=[...roots];while(pending.length){const id=pending.pop();if(result.has(id)||!graph.modules.has(id))continue;result.add(id);pending.push(...graph.modules.get(id).deps);}return result;}
function entryReach(graph){return reach(graph,graph.edges.filter(e=>e.from==='entry').map(e=>e.to));}
function sourcePatch(source,spec){
 assert.equal(hash(source),spec.sha256,`Unexpected member ${spec.name}`);const before=parse(source),edits=[];
 // These setup factories must already be the reviewed telemetry no-ops; no SDK is needed by init.
 const setup=source.slice(before.modules.get(spec.setup).node.start,before.modules.get(spec.setup).node.end);
 assert(setup.includes('exports.init=function(buildInfo,_sentry)'));
 assert(setup.includes('exports.getGlobalSentry=function(){return null}'));
 assert(!setup.includes('__webpack_require__('));
 for(const wanted of spec.edits){const start=source.indexOf(wanted.before);assert(start>=0,`Missing root edit ${wanted.before}`);assert.equal(source.indexOf(wanted.before,start+1),-1,'Ambiguous root edit');edits.push({...wanted,start,end:start+wanted.before.length});}
 // The browser declaration edit includes a short unique context shared with the following init edit.
 // Shorten it to the actual deleted declarator so edits cannot overlap.
 for(const e of edits)if(e.after===';crashReporterSetup.init('){e.before=',browser=__webpack_require__(24865)';e.after='';e.end=e.start+e.before.length;}
 edits.sort((a,b)=>a.start-b.start);for(let i=1;i<edits.length;i++)assert(edits[i-1].end<=edits[i].start);
 const inScope=n=>spec.scope==='entry'?!(n.start>=before.table.start&&n.end<=before.table.end):n.start>=before.modules.get(spec.scope).node.start&&n.end<=before.modules.get(spec.scope).node.end;
 for(const id of before.identifiers.filter(inScope))assert(edits.some(e=>id.start>=e.start&&id.end<=e.end),`Unexpected surviving ${id.name} reference in changed scope`);
 let output=source;for(const e of [...edits].reverse())output=output.slice(0,e.start)+e.after+output.slice(e.end);
 new vm.Script(output,{filename:`NotDiscord-SDK-imports-${spec.name}.syntax-only.js`});const after=parse(output);
 let srcAt=0,outAt=0;for(const e of edits){const len=e.start-srcAt;assert.equal(output.slice(outAt,outAt+len),source.slice(srcAt,e.start),'Unselected code changed');srcAt=e.end;outAt+=len+e.after.length;}assert.equal(output.slice(outAt),source.slice(srcAt));
 assert.deepEqual([...before.modules.keys()],[...after.modules.keys()],'Factory inventory changed');
 for(const[id,m]of before.modules)if(id!==spec.scope)assert.equal(after.modules.get(id).sha256,m.sha256,`Untouched factory ${id} changed`);
 for(const id of spec.sdk)assert.equal(before.modules.get(id).sha256,after.modules.get(id).sha256,'SDK contract changed');
 assert.equal(after.edges.filter(e=>e.from===spec.scope&&spec.sdk.includes(e.to)).length,0,'Root SDK import remains');
 assert.equal(before.edges.filter(e=>e.from===spec.scope&&spec.sdk.includes(e.to)).length,2);
 const bReach=entryReach(before),aReach=entryReach(after),removed=[...bReach].filter(id=>!aReach.has(id)).sort((a,b)=>a-b),sdkClosure=reach(before,spec.sdk);
 const analysis={member:spec.name,changedScope:spec.scope,setupFactory:spec.setup,sdkRoots:spec.sdk,parseOnly:true,moduleCount:before.modules.size,incomingSdkCallsBefore:before.edges.filter(e=>spec.sdk.includes(e.to)),incomingSdkCallsAfter:after.edges.filter(e=>spec.sdk.includes(e.to)),sdkDependencyClosure:{factoryCount:sdkClosure.size,factorySourceBytes:[...sdkClosure].reduce((n,id)=>n+before.modules.get(id).bytes,0)},allLiteralDependencyReachability:{before:bReach.size,after:aReach.size,noLongerReachableFactoryCount:removed.length,noLongerReachableSourceBytes:removed.reduce((n,id)=>n+before.modules.get(id).bytes,0),noLongerReachableFactoryIds:removed},dynamicRequireSites:before.dynamic,interpretation:'All literal calls in nested/deferred functions count as graph edges. This is a conservative static dependency comparison, not an execution trace or RAM estimate. SDK factories and their exports remain byte-identical; only obsolete root import calls and ignored transport closures are removed.'};
 return{source:output,edits,analysis,before,after};
}
function prepareArtifact(spec,input){
 assert.equal(hash(input),spec.sha256,`Unexpected source ${spec.name}`);const patches=[];
 if(spec.kind==='javascript'){const p=sourcePatch(input.toString('utf8'),spec.members[0]);patches.push(p);return{spec,input,output:Buffer.from(p.source),patches};}
 const archive=unpack(input),originals=new Map([...archive.files].map(([n,f])=>[n,Buffer.from(f.bytes)])),updates=new Map();
 for(const member of spec.members){assert(archive.files.has(member.name));const p=sourcePatch(archive.files.get(member.name).bytes.toString('utf8'),member);patches.push(p);updates.set(member.name,Buffer.from(p.source));archive.files.get(member.name).bytes=Buffer.from(p.source);}
 const output=repack(archive,archive.files.get('bundle.js').bytes.toString('utf8')),round=unpack(output);assert.deepEqual([...round.files.keys()],[...originals.keys()]);for(const[n,b]of originals)assert(round.files.get(n).bytes.equals(updates.get(n)||b),`Unexpected archive change: ${n}`);
 return{spec,input,output,patches};
}
function selfTest(prepared){const checks=[];function check(name,fn){fn();checks.push({name,passed:true});}
 for(const item of prepared){
  check(`${item.spec.name}: exact source guard`,()=>assert.equal(hash(item.input),item.spec.sha256));
  check(`${item.spec.name}: changed source rejected`,()=>{const bad=Buffer.from(item.input);bad[bad.length-1]^=1;assert.throws(()=>prepareArtifact(item.spec,bad),/Unexpected source/);});
  check(`${item.spec.name}: output differs`,()=>assert.notEqual(hash(item.output),item.spec.sha256));
  check(`${item.spec.name}: second application rejected`,()=>assert.throws(()=>prepareArtifact(item.spec,item.output),/Unexpected source/));
  for(const p of item.patches){
   check(`${item.spec.name}: no setup/Sentry export change`,()=>{for(const id of [...p.analysis.sdkRoots,p.analysis.setupFactory])assert.equal(p.before.modules.get(id).sha256,p.after.modules.get(id).sha256);});
   check(`${item.spec.name}: no remaining root SDK edges`,()=>assert(!p.after.edges.some(e=>e.from===p.analysis.changedScope&&p.analysis.sdkRoots.includes(e.to))));
   check(`${item.spec.name}: init retains buildInfo argument`,()=>{let found=[];walk(p.after.ast,n=>{if(n.type==='CallExpression'&&n.callee.type==='MemberExpression'&&n.callee.object.name==='crashReporterSetup'&&n.callee.property.name==='init')found.push(n);});assert.equal(found.length,1);assert.equal(found[0].arguments.length,1);assert.equal(found[0].arguments[0].name,'buildInfo');});
  }
 }
 const bp=prepared.find(p=>p.spec.name==='bootstrap').patches[0];
 check('bootstrap: local error handling and deferred Sentry captures preserved',()=>{assert.equal(bp.before.modules.get(37079).sha256,bp.after.modules.get(37079).sha256);assert.equal(bp.after.edges.filter(e=>e.from===37079&&e.to===65681).length,2);});
 for(const item of prepared.filter(p=>p.spec.name!=='bootstrap'))check(`${item.spec.name}: SDK roots unreachable through all literal entry calls`,()=>{const p=item.patches[0],r=entryReach(p.after);for(const id of p.analysis.sdkRoots)assert(!r.has(id));});
 check('exactly seven focused edits',()=>assert.equal(prepared.reduce((n,p)=>n+p.patches.reduce((s,x)=>s+x.edits.length,0),0),7));
 return checks;
}
function inside(base,file){const relative=path.relative(base,file);return relative!==''&&relative!=='..'&&!relative.startsWith('..'+path.sep)&&!path.isAbsolute(relative);}
function noLinks(file){for(let p=path.resolve(file);;p=path.dirname(p)){if(fs.existsSync(p))assert(!fs.lstatSync(p).isSymbolicLink(),'Reparse path forbidden');if(p===path.dirname(p))break;}}
function write(base,file,bytes){assert(inside(base,file));noLinks(file);fs.mkdirSync(path.dirname(file),{recursive:true});noLinks(file);fs.writeFileSync(file,bytes);}
function main(argv){assert(argv.length===0||argv.length===1&&argv[0]==='--check','Optional --check only; this tool cannot apply changes');const prepared=ARTIFACTS.map(spec=>prepareArtifact(spec,fs.readFileSync(spec.source))),checks=selfTest(prepared),stage=argv.length===0;
 if(stage)for(const p of prepared)write(STAGED,path.join(STAGED,p.spec.output),p.output);
 for(const p of prepared)assert.equal(hash(fs.readFileSync(p.spec.source)),p.spec.sha256,'Active runtime changed during preparation');
 const manifest={policyVersion:1,preparedUtc:new Date().toISOString(),staged:stage,applied:false,targetCodeExecuted:false,parser:{name:'Node bundled Acorn',version:acorn.version},checks,passed:checks.every(c=>c.passed),artifacts:prepared.map(p=>({name:p.spec.name,source:p.spec.source,sourceSha256:p.spec.sha256,output:path.join(STAGED,p.spec.output),outputSha256:hash(p.output),edits:p.patches.flatMap(x=>x.edits.map(e=>({member:x.analysis.member,utf16Start:e.start,utf16End:e.end,beforeSha256:hash(e.before),afterSha256:hash(e.after)})))})),analysis:prepared.flatMap(p=>p.patches.map(x=>x.analysis)),shippedEvidence:{bootstrapReaEvidence:'ev_0fdf5de72f17b667671e44494bcb887dfd79af0edc639506088e43b5e15f4af3',originalBootstrapSha256:'cbd26748b01c1b4847dc5471c4d2e1e46d69b0eb6f6c3347aa66ca992267e9b8',note:'REA analyzed shipped bundle.js; semantic budget was partial. The complete local AST audit in this proposal operates on the exact pinned candidate copies.'},scope:['Remove obsolete root imports of Sentry Electron/Browser in bootstrap, and Sentry Renderer/Browser in main/overlay preloads','Pass buildInfo directly to the previously reviewed telemetry opt-out init','Preserve all SDK module factories, exports, setup factories, error-handler functions and other archive members','Never read account profiles or run the target application'],prerequisites:['Apply only after closing the native candidate gracefully','Update the reviewed host entry bootstrap SHA pin to the staged bootstrap hash and repack entry app.asar','Copy staged artifacts only to native-candidate runtime; never original Discord','Runtime startup, chat, voice, capture, plugins and matched RAM still require validation'],limits:['No RAM saving is proven by source bytes or module counts; measure actual total private working set','The bootstrap SDK remains callable by preserved errorHandler.fatal and non-stable errorHandler.handled; these imports are inside the deferred functions','Static graph includes nested calls but cannot resolve dynamic requires, native modules or runtime reflection','Bundled library text and SDK debug ID trailers remain; the intended saving is avoided factory evaluation/objects, not removal of all parse cost','This is not a guarantee of network silence or removal of native telemetry','Overlay preloader saving matters only when that preload actually runs']};
 const evidenceRoot=path.join(ROOT,'evidence/native');write(evidenceRoot,path.join(evidenceRoot,'sentry-imports-policy.json'),JSON.stringify(manifest,null,2)+'\n');console.log(JSON.stringify({staged:stage,applied:false,passed:manifest.passed,checks:checks.length,outputs:manifest.artifacts.map(x=>({name:x.name,sha256:x.outputSha256})),analysis:manifest.analysis.map(x=>({member:x.member,sdkDependencyClosure:x.sdkDependencyClosure,graph:x.allLiteralDependencyReachability,dynamicRequires:x.dynamicRequireSites.length})),evidence:path.join(evidenceRoot,'sentry-imports-policy.json')},null,2));
}
module.exports={ARTIFACTS,sourcePatch,prepareArtifact,selfTest,parse,reach,entryReach};if(require.main===module)try{main(process.argv.slice(2));}catch(error){console.error(error.stack||String(error));process.exitCode=1;}
