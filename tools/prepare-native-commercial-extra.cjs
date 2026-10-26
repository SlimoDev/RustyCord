'use strict';
// Staging only: this preparer never writes the active policy, vendor files or app configuration.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),vm=require('node:vm'),assert=require('node:assert/strict');
const ROOT=path.resolve(__dirname,'..'),NATIVE=path.join(ROOT,'native-candidate'),STAGE=path.join(NATIVE,'prepared-commercial');
const INPUTS={
 web:{path:path.join(ROOT,'evidence/public-web/web.4d4b2f75fdea5470.js'),sha256:'667a771c61ff709e2f0400d395a324f0a5b1b5aa89e11e263289144a12d084ae'},
 policy:{path:path.join(NATIVE,'host/renderer-policy.js'),sha256:'0fa9e650e4b336a3e009ab2f0b7bd4096969adb31e992f085aea26a0aadda4b0'},
 renderer:{path:path.join(NATIVE,'vencord/dist/renderer.js'),sha256:'e1283ec8d13bbbbbd1b2fbc6d3ea5e60eb7a9fbf314dd077644dcdb41f285f08'},
 preload:{path:path.join(NATIVE,'vencord/dist/preload.js'),sha256:'a05b8c9f6ef86d25ecf00cb6bf6e5fdc8ebde7798a76b97918ae0bb59edab1cb'}
};
const sha=x=>crypto.createHash('sha256').update(x).digest('hex');
const STATE='__notDiscordNativeCommercialExtraState';
const INSERT='            state.registered = true;';
const C_ANCHOR='ws.initialize(),Pn.initialize(),ip.initialize()';
const S_ENTRY='SocialLayerStorefrontAnnouncementManager:{actions:["LOGOUT","POST_CONNECTION_OPEN","SOCIAL_LAYER_STOREFRONT_CONFIG_FETCH_SUCCESS","SOCIAL_LAYER_STOREFRONT_LAUNCH_ANNOUNCEMENT_FETCH_SUCCESS"],inlineRequire:()=>n(889979).A,neverLoadBeforeConnectionOpen:!0},';
const S_REGISTER='(0,i.n)(r)';
function once(source,anchor){const i=source.indexOf(anchor);return i>=0&&source.indexOf(anchor,i+anchor.length)<0;}
function only(source,anchor,replacement){assert(once(source,anchor),'Expected unique anchor: '+anchor.slice(0,100));return source.replace(anchor,()=>replacement);}
function parsePolicy(bytes){
 const s=bytes.toString('utf8'),prefix='module.exports=Object.freeze(',start=s.indexOf(prefix);
 assert(start>=0&&s.trimEnd().endsWith(');'),'Policy format mismatch');
 return JSON.parse(s.slice(start+prefix.length,s.lastIndexOf(');')));
}
function verifiedInputs(overrides={}){
 const out={};
 for(const [key,item] of Object.entries(INPUTS)){
  const bytes=overrides[key]||fs.readFileSync(item.path);
  assert.equal(sha(bytes),item.sha256,key+' SHA mismatch: stop for source review');
  out[key]=bytes;
 }
 return out;
}
function marker(key){return '(()=>{try{window.'+STATE+'.'+key+'.applied=true}catch{}})()';}
function extractFactory(web,id,start,end,bodyHash){
 const raw=web.slice(start,end);
 assert(raw.startsWith(id+'(e,t,n){')&&raw.endsWith('}'),'Factory bounds mismatch '+id);
 assert.equal(sha(raw.slice(String(id).length)),bodyHash,'Factory body mismatch '+id);
 new vm.Script('0,function'+raw.slice(raw.indexOf('(')));
 return {id:String(id),raw,normalized:'0,function'+raw.slice(raw.indexOf('(')),start,end,bodyHash};
}
function specs(web){
 const c=extractFactory(web,926456,981009,2265906,'be99324fd6e1584bcc134f7dc072abb1d605adf86d2a16837562d16945d50a8d');
 const s=extractFactory(web,40876,2916046,2942074,'af6de50a2a33d8ba7496ee10f0221abbc76f3928670bf9e89f7e83b69e517ec0');
 const begin='class Pt extends nE.A{',end='let Pn=new Pt;';
 assert(once(c.raw,begin)&&once(c.raw,end),'Collectibles class boundaries mismatch');
 const classBlock=c.raw.slice(c.raw.indexOf(begin),c.raw.indexOf(end)+end.length);
 const result=[
  {key:'collectibles',factory:c,guards:[classBlock,'M8="CollectiblesMarketingManager"','Oy=n(815996)','nE=n(272355)'],edits:[[C_ANCHOR,'ws.initialize(),'+marker('collectibles')+',ip.initialize()']]},
  {key:'socialAnnouncement',factory:s,guards:['var i=n(928439);let r={',S_ENTRY,S_REGISTER],edits:[[S_ENTRY,''],[S_REGISTER,'('+S_REGISTER+','+marker('socialAnnouncement')+')']]}
 ];
 for(const target of result){
  for(const guard of target.guards)assert(once(target.factory.raw,guard),target.key+' guard mismatch');
  for(const edit of target.edits)assert(once(target.factory.raw,edit[0]),target.key+' edit mismatch');
 }
 return result;
}
function extraCode(targets){
 const definitions=targets.map(t=>({key:t.key,id:t.factory.id,guards:t.guards,edits:t.edits}));
 return [
 '            // Staged native marketing policy; no exports, chunks, manual routes or quest fetches removed.',
 '            const extraState = {version:1,registered:false,collectibles:{registered:false,matched:false,applied:false,error:false,late:false},socialAnnouncement:{registered:false,matched:false,applied:false,error:false,late:false}};',
 '            try {',
 '                if (Object.getOwnPropertyDescriptor(window, '+JSON.stringify(STATE)+')) throw new Error("Existing extra policy state");',
 '                Object.defineProperty(window, '+JSON.stringify(STATE)+', {value:extraState, configurable:true});',
 '            } catch { state.error=true; return; }',
 '            const extraTargets = '+JSON.stringify(definitions)+';',
 '            for (const target of extraTargets) {',
 '                const status = extraState[target.key];',
 '                try {',
 '                    if (vencord.Webpack?.wreq?.c && Object.prototype.hasOwnProperty.call(vencord.Webpack.wreq.c,target.id)) {status.late=true; continue;}',
 '                    vencord.Plugins.addPatch({',
 '                        find:new RegExp("^"+target.id+"\\\\(e,t,n\\\\)\\\\{"),',
 '                        replacement:{match:/^[\\s\\S]*$/,replace(source){',
 '                            // Exact local guards tolerate unrelated Vencord patches. On any drift, preserve source.',
 '                            const unique=anchor=>{const i=source.indexOf(anchor);return i>=0&&source.indexOf(anchor,i+anchor.length)<0;};',
 '                            if(!target.guards.every(unique)||!target.edits.every(edit=>unique(edit[0]))){status.error=true;return source;}',
 '                            let updated=source;',
 '                            for(const [from,to] of target.edits) updated=updated.replace(from,()=>to);',
 '                            status.matched=true;',
 '                            // applied is set by the replacement only when its original activation point executes.',
 '                            return updated;',
 '                        }}',
 '                    },"NotDiscordNativeCommercialExtra");',
 '                    status.registered=true;',
 '                } catch { status.error=true; }',
 '            }',
 '            extraState.registered=extraTargets.every(target=>extraState[target.key].registered);',
 ''
 ].join('\n');
}
function prepare(input){
 for(const [key,item]of Object.entries(INPUTS))assert.equal(sha(input[key]),item.sha256,key+' SHA mismatch');
 const base=parsePolicy(input.policy),targets=specs(input.web.toString('utf8'));
 assert.equal(base.version,1);assert.equal(typeof base.before,'string');
 const code=extraCode(targets),before=only(base.before,INSERT,code+INSERT);
 const policy='"use strict";\n// STAGED ONLY. Native commercial activation policy; review before promotion.\nmodule.exports=Object.freeze('+JSON.stringify({...base,commercialExtraVersion:1,before})+');\n';
 new vm.Script(before);new vm.Script(policy);
 assert.equal(before.replace(code,''),base.before,'Base policy changed outside insertion');
 return {targets,code,before,policy,base};
}
function nativeHelpers(renderer){
 const begin=renderer.indexOf('function Kt('),end=renderer.indexOf('var zr=',begin);
 const addStart=renderer.indexOf('function JS('),addEnd=renderer.indexOf('function Hd(',addStart);
 assert(begin>=0&&end>begin&&addStart>=0&&addEnd>addStart);
 return 'var gs=[];'+renderer.slice(begin,end)+renderer.slice(addStart,addEnd);
}
function fixture(prepared,input,{cache=[],throwAt=0,origin='https://discord.com',frame=false,skipAssign=false}={}){
 const styles=[],listeners=new Map(),timers=new Map();let id=0;
 const document={readyState:'loading',head:{appendChild(x){styles.push(x)}},getElementById(name){return styles.find(x=>x.id===name)||null},createElement(){return{}},addEventListener(name,fn){listeners.set(name,[...(listeners.get(name)||[]),fn])},removeEventListener(name,fn){listeners.set(name,(listeners.get(name)||[]).filter(x=>x!==fn))}};
 const context=vm.createContext({location:{origin},document,setTimeout(fn){timers.set(++id,fn);return id},clearTimeout(key){timers.delete(key)}});
 vm.runInContext('window=globalThis;self=window;top=window;',context);
 if(frame)context.top={};
 vm.runInContext(nativeHelpers(input.renderer.toString('utf8')),context);
 context.cache=Object.fromEntries(cache.map(key=>[key,{}]));context.throwAt=throwAt;
 vm.runInContext('var addCount=0;var testVencord={Plugins:{addPatch(p,n){addCount++;if(addCount===throwAt)throw Error("fixture registration failure");return JS(p,n);}},Webpack:{wreq:{c:cache}}};',context);
 vm.runInContext(prepared.before,context,{timeout:1000});
 if(!skipAssign)vm.runInContext('var Vencord=testVencord;',context);
 return {context,styles,listeners,timers,patches:context.gs,state:context[STATE]};
}
function apply(patches,raw,source){
 let updated=source;
 for(const patch of patches){
  if(!(typeof patch.find==='string'?raw.includes(patch.find):patch.find.test(raw)))continue;
  for(const replacement of patch.replacement)updated=updated.replace(replacement.match,replacement.replace);
 }
 return updated;
}
function tests(prepared,input){
 const checks=[],check=(name,fn)=>{fn();checks.push(name);};
 const c=prepared.targets[0],s=prepared.targets[1];
 check('native real addPatch normalizers register Nitro and two extras via one Vencord setter',()=>{
  const f=fixture(prepared,input);assert.equal(f.patches.length,3);assert.equal(f.state.registered,true);
  assert.equal(f.context.__notDiscordCommercialPreloadState.registered,true);
  assert.equal(Object.getOwnPropertyDescriptor(f.context,'Vencord').value,f.context.testVencord);assert.equal(f.timers.size,0);
  assert.equal((prepared.before.match(/set: assignedVencord/g)||[]).length,1);
 });
 check('whole main factory compiles; only Nitro effect and Collectibles activation differ',()=>{
  const f=fixture(prepared,input),actual=apply(f.patches,c.factory.raw,c.factory.normalized);
  let expected=c.factory.normalized.replace('eI.useEffect(()=>{(0,ra.t)(()=>re.preload())},[])','eI.useEffect(()=>{try{window.__notDiscordCommercialPreloadState.applied=true}catch{}},[])');
  expected=only(expected,...c.edits[0]);assert.equal(actual,expected);new vm.Script(actual);
  assert.equal(f.state.collectibles.matched,true);assert.equal(f.state.collectibles.applied,false);
  assert.equal(actual.includes(c.guards[0]),true);assert.equal(actual.includes('Pn.terminate()'),true);
 });
 check('registry compiles; only announcement registry entry and execution marker differ',()=>{
  const f=fixture(prepared,input),actual=apply(f.patches,s.factory.raw,s.factory.normalized);
  let expected=s.factory.normalized;for(const edit of s.edits)expected=only(expected,...edit);
  assert.equal(actual,expected);new vm.Script(actual);
  assert.equal(f.state.socialAnnouncement.matched,true);assert.equal(f.state.socialAnnouncement.applied,false);
  assert.equal(actual.includes('SocialLayerStorefrontConfigManager'),true);
 });
 for(const target of prepared.targets){
  check(target.key+' permits unrelated preceding plugin edits',()=>{
   const f=fixture(prepared,input),modified=target.factory.normalized+'/* unrelated Vencord patch */';
   assert.notEqual(apply(f.patches.slice(1),target.factory.raw,modified),modified);
   assert.equal(f.state[target.key].matched,true);
  });
  for(let i=0;i<target.guards.length;i++){
   check(target.key+' fails unchanged on drift in guard '+i,()=>{
    const f=fixture(prepared,input);
    // For block guards insert drift inside the block, not after it.
    const altered=target.factory.normalized.replace(target.guards[i],()=>target.guards[i].slice(0,1)+'/* drift */'+target.guards[i].slice(1));
    assert.equal(apply(f.patches.filter(p=>p.plugin==='NotDiscordNativeCommercialExtra'),target.factory.raw,altered),altered);
    assert.equal(f.state[target.key].error,true);assert.equal(f.state[target.key].matched,false);
   });
  }
  check(target.key+' rejects duplicate anchor and double application unchanged',()=>{
   const f=fixture(prepared,input),patches=f.patches.slice(1),dup=target.factory.normalized+'/*'+target.edits[0][0]+'*/';
   assert.equal(apply(patches,target.factory.raw,dup),dup);
   const fresh=fixture(prepared,input),oncePatched=apply(fresh.patches.slice(1),target.factory.raw,target.factory.normalized);
   assert.equal(apply(fresh.patches.slice(1),target.factory.raw,oncePatched),oncePatched);
  });
  check(target.key+' rejects different raw module identity',()=>{
   const f=fixture(prepared,input);assert.equal(apply(f.patches.slice(1),target.factory.raw.replace(target.factory.id,'999999'),target.factory.normalized),target.factory.normalized);
   assert.equal(f.state[target.key].matched,false);
  });
 }
 check('late main factory preserves original policy late guard and registers no patches',()=>{
  const f=fixture(prepared,input,{cache:['926456']});assert.equal(f.patches.length,0);assert.equal(f.context.__notDiscordCommercialPreloadState.late,true);assert.equal(f.state,undefined);
 });
 check('late announcement factory skips only that extra and reports incomplete registration',()=>{
  const f=fixture(prepared,input,{cache:['40876']});assert.equal(f.patches.length,2);assert.equal(f.state.socialAnnouncement.late,true);assert.equal(f.state.registered,false);assert.equal(f.state.collectibles.registered,true);
 });
 for(const throwAt of [2,3])check('registration exception '+throwAt+' does not falsely claim both registered',()=>{
  const f=fixture(prepared,input,{throwAt});assert.equal(f.state.registered,false);assert.equal(f.state[throwAt===2?'collectibles':'socialAnnouncement'].error,true);
 });
 check('Collectibles fixture executes neighbors, skips initialize and marks execution',()=>{
  const f=fixture(prepared,input);f.context.calls=[];
  vm.runInContext('var ws={initialize(){calls.push("ws")}},Pn={initialize(){calls.push("Pn")}},ip={initialize(){calls.push("ip")}};'+c.edits[0][1],f.context);
  assert.deepEqual(Array.from(f.context.calls),['ws','ip']);assert.equal(f.state.collectibles.applied,true);
 });
 check('registry fixture preserves remaining entry and marks actual registrar execution',()=>{
  const f=fixture(prepared,input);let isolated='var r={'+S_ENTRY+'Other:{actions:["MANUAL"],inlineRequire:()=>42}},i={n(value){globalThis.registeredKeys=Object.keys(value)}};'+S_REGISTER+';';
  for(const edit of s.edits)isolated=only(isolated,...edit);
  vm.runInContext(isolated,f.context);assert.deepEqual(Array.from(f.context.registeredKeys),['Other']);assert.equal(f.state.socialAnnouncement.applied,true);
 });
 check('announcement registration failure does not mark activation as applied',()=>{
  const f=fixture(prepared,input);f.context.i={n(){throw Error('fixture registrar failure')}};f.context.r={};
  assert.throws(()=>vm.runInContext(s.edits[1][1],f.context),/fixture registrar failure/);
  assert.equal(f.state.socialAnnouncement.applied,false);
 });
 check('policy repeat is idempotent',()=>{
  const f=fixture(prepared,input);vm.runInContext(prepared.before,f.context);assert.equal(f.patches.length,3);
 });
 check('foreign origins and subframes do not install policy',()=>{
  for(const options of [{origin:'https://example.invalid'},{frame:true}]){const f=fixture(prepared,input,options);assert.equal(f.patches.length,0);assert.equal(f.state,undefined);}
 });
 check('existing setter timeout cleanup remains intact',()=>{
  const f=fixture(prepared,input,{skipAssign:true});for(const fn of [...f.timers.values()])fn();
  assert.equal(f.context.__notDiscordCommercialPreloadState.expired,true);assert.equal(f.timers.size,0);assert.equal(f.state,undefined);
 });
 for(const key of Object.keys(INPUTS))check(key+' changed full static input fails preparation',()=>{
  assert.throws(()=>prepare({...input,[key]:Buffer.concat([input[key],Buffer.from('\n')])}),/SHA mismatch/);
 });
 check('no new setter, timer, network, chunk block, credential or storage operations in extras',()=>{
  assert(!/\b(setTimeout|setInterval|fetch|XMLHttpRequest|localStorage|sessionStorage)\b/.test(prepared.code));
  assert(!prepared.code.includes('set:'));assert(!prepared.code.includes('n.e(')||prepared.code.includes('class Pt'));
  assert.equal(prepared.before.replace(prepared.code,''),prepared.base.before);
 });
 return {passed:true,checks:checks.length,names:checks,mode:'isolated fixtures and compile-only public factories; no client or factory execution'};
}
function safeWrite(name,data){
 assert.equal(path.basename(name),name);assert(!fs.existsSync(STAGE)||fs.realpathSync(STAGE)===STAGE,'Staging path must not be redirected');
 fs.mkdirSync(STAGE,{recursive:true});assert.equal(fs.realpathSync(STAGE),STAGE);
 const file=path.join(STAGE,name);assert(!fs.existsSync(file)||!fs.lstatSync(file).isSymbolicLink(),'No output symlinks');
 fs.writeFileSync(file,data);
}
function main(){
 assert(process.argv.slice(2).every(x=>x==='--self-test'),'Only --self-test is accepted; no apply mode exists');
 const input=verifiedInputs(),prepared=prepare(input),report=tests(prepared,input);
 for(const [key,item]of Object.entries(INPUTS))assert.equal(sha(fs.readFileSync(item.path)),item.sha256,key+' changed during preparation');
 if(!process.argv.includes('--self-test')){
  const manifest={
   schemaVersion:1,stageOnly:true,activated:false,measured:false,target25PercentVerified:false,
   inputs:INPUTS,preparedPolicySha256:sha(prepared.policy),rollbackPolicySha256:sha(input.policy),
   guards:{staticFullInputHashes:true,runtimeWholeFactoryHash:false,runtimeExactLocalBlocksAndUniqueAnchors:true,runtimeRawFactoryIdPattern:true,cacheGuard:true,driftAction:'return source unchanged'},
   statusGlobal:STATE,statusFields:['registered','matched','applied','error','late'],
   targets:prepared.targets.map(t=>({key:t.key,factoryId:t.factory.id,utf16Start:t.factory.start,utf16End:t.factory.end,bodySha256:t.factory.bodyHash,guards:t.guards.map(g=>({length:g.length,sha256:sha(g)})),edits:t.edits.map(([from,to])=>({from,to}))})),
   preserved:['one existing Vencord setter','native preload and IPC','existing Nitro policy','manager classes and exports','manual storefront routes and action modules','chat, calls, screen share and FakeNitro code','all chunks and other registry entries','quests code'],
   tradeoffs:['Collectibles reconnect marketing refresh, badges and automatic announcement suppressed. Existing module, object and persisted cache remain.','Social storefront launch-announcement dispatcher registration suppressed. The manager is also eagerly imported and remains allocated; internal announcement methods are not promised equivalent after automatic connection events stop.'],
   limitations:['Static evidence supports avoiding automatic fetch/model/modal work, not a measured RAM saving.','Manual route modules and exports remain byte-identical, but interactive shop behavior needs a live check before claiming compatibility.','No extra-runtime activation or side-by-side benchmark has been performed.','registered is only queued, matched is transformed source, applied requires execution of the replaced activation point. Collectibles applied does not certify that the entire main factory later completed; Vencord may fall back to the original after a later failure. Social applied is recorded after successful registry registration.'],
   promotion:'Review staged renderer-policy.js; this tool never activates it. If promoted later, keep renderer-policy.base.js to restore the exact prior policy and restart the candidate.',
   tests:report
  };
  safeWrite('renderer-policy.base.js',input.policy);safeWrite('renderer-policy.js',prepared.policy);
  safeWrite('manifest.json',JSON.stringify(manifest,null,2)+'\n');safeWrite('self-check.json',JSON.stringify(report,null,2)+'\n');
 }
 console.log(JSON.stringify({stageOnly:true,activated:false,output:process.argv.includes('--self-test')?null:STAGE,preparedPolicySha256:sha(prepared.policy),...report},null,2));
}
if(require.main===module)main();
module.exports={prepare,tests,verifiedInputs,INPUTS,STATE};
