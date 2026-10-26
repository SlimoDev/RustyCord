'use strict';
// Stage a single automatic advertising fetch omission. No apply mode, client launch or profile reads.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),vm=require('node:vm'),assert=require('node:assert/strict');
const ROOT=path.resolve(__dirname,'..'),NATIVE=path.join(ROOT,'native-candidate'),STAGE=path.join(NATIVE,'prepared-quest-hero');
const INPUTS={
 web:{path:path.join(ROOT,'evidence/public-web/web.4d4b2f75fdea5470.js'),sha256:'667a771c61ff709e2f0400d395a324f0a5b1b5aa89e11e263289144a12d084ae'},
 policy:{path:path.join(NATIVE,'host/renderer-policy.js'),sha256:'14bc1c015406af209f2d5534ffedab644376e8b8b5642e76b23227cfa169eedf'},
 renderer:{path:path.join(NATIVE,'vencord/dist/renderer.js'),sha256:'e1283ec8d13bbbbbd1b2fbc6d3ea5e60eb7a9fbf314dd077644dcdb41f285f08'},
 preload:{path:path.join(NATIVE,'vencord/dist/preload.js'),sha256:'a05b8c9f6ef86d25ecf00cb6bf6e5fdc8ebde7798a76b97918ae0bb59edab1cb'}
};
const sha=x=>crypto.createHash('sha256').update(x).digest('hex'),STATE='__notDiscordNativeQuestHeroState';
const INSERT='            state.registered = true;';
const INITIAL='this.initialFetchTimerId=window.setTimeout(()=>{Date.now()-d.A.lastFetchedCurrentQuests<=p||this._fetch("post_connect_initial")},r)';
const ANCHOR='if('+INITIAL+',e){let e=r+Math.floor(Math.random()*i);this.initialQuestHomeHeroFetchTimerId=window.setTimeout(()=>{try{(0,o.Yf)()}catch(e){}},e)}';
const REPLACEMENT=INITIAL+',e&&(()=>{try{window.'+STATE+'.applied=true}catch{}})()';
function unique(source,anchor){const i=source.indexOf(anchor);return i>=0&&source.indexOf(anchor,i+anchor.length)<0;}
function only(source,anchor,to){assert(unique(source,anchor),'Unique anchor mismatch: '+anchor.slice(0,90));return source.replace(anchor,()=>to);}
function readInputs(){return Object.fromEntries(Object.entries(INPUTS).map(([key,item])=>{const b=fs.readFileSync(item.path);assert.equal(sha(b),item.sha256,key+' SHA mismatch');return[key,b];}));}
function parsePolicy(bytes){const s=bytes.toString('utf8'),prefix='module.exports=Object.freeze(';assert(unique(s,prefix)&&s.trimEnd().endsWith(');'));return JSON.parse(s.slice(s.indexOf(prefix)+prefix.length,s.lastIndexOf(');')));}
function prepare(input){
 for(const[key,item]of Object.entries(INPUTS))assert.equal(sha(input[key]),item.sha256,key+' SHA mismatch');
 const web=input.web.toString('utf8'),raw=web.slice(7183512,7186714);
 assert.equal(sha(raw),'766b4a6078735141f91f0df70670a127f515e6b3f3b9881bff30a15fab25cda1','Quest factory digest mismatch');
 assert(raw.startsWith('645582(e,t,n){'));assert(unique(raw,ANCHOR));
 const handlerStart='handlePostConnectionOpen=()=>{',handlerEnd=';handleRunningGamesChange=()=>{';
 assert(unique(raw,handlerStart)&&unique(raw,handlerEnd));
 const handler=raw.slice(raw.indexOf(handlerStart),raw.indexOf(handlerEnd));
 const guards=[handler,'o=n(396813),d=n(859703),c=n(945810)','E=n(971276)','initialQuestHomeHeroFetchTimerId=null'];
 for(const guard of guards)assert(unique(raw,guard),'Guard mismatch');
 const normalized='0,function'+raw.slice(raw.indexOf('(')),transformed=only(normalized,ANCHOR,REPLACEMENT);
 new vm.Script(normalized);new vm.Script(transformed);
 const base=parsePolicy(input.policy);assert.equal(base.version,1);assert.equal(base.commercialExtraVersion,1);
 const code=[
 '            // Omit only the automatic Quest Home banner timer; current quests and progression stay intact.',
 '            const questHeroState={installed:true,registered:false,matched:false,applied:false,error:false,late:false};',
 '            try {',
 '                if(Object.getOwnPropertyDescriptor(window,'+JSON.stringify(STATE)+')) throw new Error("Existing Quest hero state");',
 '                Object.defineProperty(window,'+JSON.stringify(STATE)+',{value:questHeroState,configurable:true});',
 '                if(vencord.Webpack?.wreq?.c && Object.prototype.hasOwnProperty.call(vencord.Webpack.wreq.c,"645582")) {questHeroState.late=true;}',
 '                else {',
 '                    const questHeroGuards='+JSON.stringify(guards)+';',
 '                    const questHeroOriginal='+JSON.stringify(ANCHOR)+',questHeroReplacement='+JSON.stringify(REPLACEMENT)+';',
 '                    vencord.Plugins.addPatch({find:/^645582\\(e,t,n\\)\\{/,replacement:{match:/^[\\s\\S]*$/,replace(source){',
 '                        const unique=anchor=>{const i=source.indexOf(anchor);return i>=0&&source.indexOf(anchor,i+anchor.length)<0;};',
 '                        if(!questHeroGuards.every(unique)||!unique(questHeroOriginal)){questHeroState.error=true;return source;}',
 '                        questHeroState.matched=true;',
 '                        return source.replace(questHeroOriginal,()=>questHeroReplacement);',
 '                    }}},"NotDiscordNativeQuestHero");',
 '                    questHeroState.registered=true;',
 '                }',
 '            } catch {questHeroState.error=true;}',
 ''
 ].join('\n');
 const before=only(base.before,INSERT,code+INSERT);
 assert.equal(before.replace(code,''),base.before);
 const policy='"use strict";\n// STAGED ONLY. Defer the automatic Quest Home advertising hero request.\nmodule.exports=Object.freeze('+JSON.stringify({...base,questHeroVersion:1,before})+');\n';
 new vm.Script(before);new vm.Script(policy);
 return {raw,normalized,transformed,handler,guards,base,code,before,policy};
}
function nativeHelpers(renderer){
 const begin=renderer.indexOf('function Kt('),end=renderer.indexOf('var zr=',begin),a=renderer.indexOf('function JS('),b=renderer.indexOf('function Hd(',a);
 assert(begin>=0&&end>begin&&a>=0&&b>a);return'var gs=[];'+renderer.slice(begin,end)+renderer.slice(a,b);
}
function fixture(p,input,{cache=[],throwAt=0,origin='https://discord.com',frame=false,skipAssign=false,existingState=false}={}){
 const listeners=new Map(),styles=[],timers=new Map();let seq=0;
 const document={readyState:'loading',head:{appendChild(x){styles.push(x)}},getElementById(id){return styles.find(x=>x.id===id)||null},createElement(){return{}},addEventListener(k,fn){listeners.set(k,[...(listeners.get(k)||[]),fn])},removeEventListener(k,fn){listeners.set(k,(listeners.get(k)||[]).filter(x=>x!==fn))}};
 const context=vm.createContext({location:{origin},document,setTimeout(fn){timers.set(++seq,fn);return seq},clearTimeout(id){timers.delete(id)}});
 vm.runInContext('window=globalThis;self=window;top=window;',context);if(frame)context.top={};
 vm.runInContext(nativeHelpers(input.renderer.toString('utf8')),context);
 context.cache=Object.fromEntries(cache.map(id=>[id,{}]));context.throwAt=throwAt;
 if(existingState)Object.defineProperty(context,STATE,{value:{foreign:true},configurable:false});
 vm.runInContext('var calls=0;var testVencord={Plugins:{addPatch(p,n){if(++calls===throwAt)throw Error("fixture addPatch");return JS(p,n)}},Webpack:{wreq:{c:cache}}};',context);
 vm.runInContext(p.before,context,{timeout:1000});if(!skipAssign)vm.runInContext('var Vencord=testVencord;',context);
 return{context,patches:context.gs,state:context[STATE],timers};
}
function apply(patches,raw,source){let out=source;for(const patch of patches){if(!(typeof patch.find==='string'?raw.includes(patch.find):patch.find.test(raw)))continue;for(const r of patch.replacement)out=out.replace(r.match,r.replace);}return out;}
function handlerFixture(p,{eligible=true,reconnect=false,recent=false}={}){
 const ctx=vm.createContext({eligible,reconnect,recent});ctx[STATE]={applied:false};
 vm.runInContext('window=globalThis;var cleared=[],timeouts=[],intervals=[],fetches=[],heroCalls=0,configCalls=0,seq=100;window.clearTimeout=id=>cleared.push(id);window.setTimeout=(fn,ms)=>{timeouts.push({fn,ms});return ++seq};window.setInterval=(fn,ms)=>{intervals.push({fn,ms});return ++seq};var E={s:()=>eligible},d={A:{lastFetchedCurrentQuests:recent?Date.now():0}},u={questFetchJitterMs:5000,questHomeHeroJitterMs:5000},_={getConfig(){configCalls++;return{questFetchJitterMs:60000,questHomeHeroJitterMs:60000}}},h=1800000,p=300000,A=86400000,o={Yf(){heroCalls++}};',ctx);
 const handler=only(p.handler,ANCHOR,REPLACEMENT).replace('handlePostConnectionOpen=','');
 vm.runInContext('var manager={initialFetchTimerId:11,initialQuestHomeHeroFetchTimerId:12,recurringFetchTimerId:13,lastFetchAttemptedAt:0,hasHandledConnectionOpen:reconnect,_fetch(caller){fetches.push(caller)}};manager.run=function(){return ('+handler+')()};manager.run();',ctx);
 return ctx;
}
function tests(p,input){
 const names=[],check=(name,fn)=>{fn();names.push(name);};
 check('real native addPatch registers four patches through one existing setter',()=>{
  const f=fixture(p,input);assert.equal(f.patches.length,4);assert(f.state.registered);assert(!f.state.matched&&!f.state.applied);
  assert.equal(Object.getOwnPropertyDescriptor(f.context,'Vencord').value,f.context.testVencord);
  assert.equal((p.before.match(/set: assignedVencord/g)||[]).length,1);assert.equal(f.timers.size,0);
 });
 check('complete factory transforms exactly one advertising timer branch and compiles',()=>{
  const f=fixture(p,input),result=apply(f.patches,p.raw,p.normalized);assert.equal(result,p.transformed);new vm.Script(result);
  assert.equal(f.state.matched,true);assert.equal(f.state.applied,false);
 });
 check('existing Nitro, Collectibles and Social transformations unchanged',()=>{
  const web=input.web.toString('utf8'),expanded=fixture(p,input),base=fixture({...p,before:p.base.before},input);
  for(const [start,end]of [[981009,2265906],[2916046,2942074]]){
   const raw=web.slice(start,end),normalized='0,function'+raw.slice(raw.indexOf('('));
   assert.equal(apply(expanded.patches,raw,normalized),apply(base.patches,raw,normalized));
  }
 });
 check('unrelated Vencord source modifications remain compatible',()=>{
  const f=fixture(p,input),source=p.normalized+'/* another plugin */';assert.equal(apply(f.patches,p.raw,source),p.transformed+'/* another plugin */');
 });
 for(let i=0;i<p.guards.length;i++)check('guard '+i+' drift returns original source',()=>{
  const f=fixture(p,input),g=p.guards[i],changed=p.normalized.replace(g,()=>g.slice(0,1)+'/*drift*/'+g.slice(1));
  assert.equal(apply(f.patches,p.raw,changed),changed);assert.equal(f.state.error,true);assert.equal(f.state.matched,false);
 });
 check('duplicate target anchor returns original source',()=>{
  const f=fixture(p,input),source=p.normalized+'/*'+ANCHOR+'*/';assert.equal(apply(f.patches,p.raw,source),source);assert.equal(f.state.error,true);
 });
 check('different original factory identity does not match',()=>{
  const f=fixture(p,input);assert.equal(apply(f.patches,p.raw.replace('645582','123456'),p.normalized),p.normalized);assert.equal(f.state.matched,false);
 });
 check('repeat replacement fails unchanged',()=>{
  const f=fixture(p,input),first=apply(f.patches,p.raw,p.normalized);assert.equal(apply(f.patches,p.raw,first),first);assert.equal(f.state.error,true);
 });
 check('late Quest factory skips only Quest patch',()=>{
  const f=fixture(p,input,{cache:['645582']});assert.equal(f.patches.length,3);assert.equal(f.state.late,true);assert.equal(f.state.registered,false);assert.equal(f.context.__notDiscordNativeCommercialExtraState.registered,true);
 });
 check('Quest registration failure does not change prior patch registrations',()=>{
  const f=fixture(p,input,{throwAt:4});assert.equal(f.patches.length,3);assert.equal(f.state.error,true);assert.equal(f.state.registered,false);assert.equal(f.context.__notDiscordCommercialPreloadState.registered,true);
 });
 check('existing foreign status is not overwritten',()=>{
  const f=fixture(p,input,{existingState:true});assert.equal(f.patches.length,3);assert.equal(f.state.foreign,true);
 });
 for(const reconnect of [false,true])check('connection handler preserves initial and recurring quest work, reconnect='+reconnect,()=>{
  const f=handlerFixture(p,{reconnect});assert.equal(f.timeouts.length,1);assert.equal(f.intervals.length,1);assert.equal(f.intervals[0].ms,1800000);
  assert.deepEqual(Array.from(f.cleared),[11,12,13]);assert.equal(f.manager.initialQuestHomeHeroFetchTimerId,12);assert.equal(f.manager.hasHandledConnectionOpen,true);
  assert.equal(f[STATE].applied,true);assert.equal(f.configCalls,reconnect?1:0);
  f.timeouts[0].fn();f.intervals[0].fn();assert.deepEqual(Array.from(f.fetches),['post_connect_initial','post_connect_recurring']);assert.equal(f.heroCalls,0);
 });
 check('recent current quests still avoid redundant initial fetch',()=>{
  const f=handlerFixture(p,{recent:true});f.timeouts[0].fn();assert.equal(f.fetches.length,0);assert.equal(f[STATE].applied,true);
 });
 check('ineligible case preserves quest timer and does not claim advertising suppression executed',()=>{
  const f=handlerFixture(p,{eligible:false});assert.equal(f.timeouts.length,1);assert.equal(f.intervals.length,1);assert.equal(f[STATE].applied,false);assert.equal(f.heroCalls,0);
 });
 check('policy idempotence preserves four registrations',()=>{
  const f=fixture(p,input);vm.runInContext(p.before,f.context);assert.equal(f.patches.length,4);
 });
 check('foreign origins and child frames remain unaffected',()=>{
  for(const options of [{origin:'https://example.invalid'},{frame:true}]){const f=fixture(p,input,options);assert.equal(f.patches.length,0);assert.equal(f.state,undefined);}
 });
 check('existing assignment timeout still cleans up',()=>{
  const f=fixture(p,input,{skipAssign:true});for(const fn of [...f.timers.values()])fn();assert.equal(f.context.__notDiscordCommercialPreloadState.expired,true);assert.equal(f.state,undefined);assert.equal(f.timers.size,0);
 });
 for(const key of Object.keys(INPUTS))check(key+' full input hash mismatch stops preparation',()=>{
  assert.throws(()=>prepare({...input,[key]:Buffer.concat([input[key],Buffer.from('\n')])}),/SHA mismatch/);
 });
 check('manual hero loader, quest actions, progress manager, chunks and exports remain outside the only factory edit',()=>{
  assert.equal(p.normalized.replace(ANCHOR,'<ONLY_EDIT>'),p.transformed.replace(REPLACEMENT,'<ONLY_EDIT>'));
  assert(p.normalized.includes('n.d(t,{A:()=>m})'));
  assert.equal(p.before.replace(p.code,''),p.base.before);
 });
 return{passed:true,checks:names.length,names,mode:'isolated native addPatch and handler fixtures; complete factories compiled but not executed; no client launch'};
}
function safeWrite(name,data){
 assert.equal(path.basename(name),name);assert(!fs.existsSync(STAGE)||fs.realpathSync(STAGE)===STAGE,'Staging path redirected');
 fs.mkdirSync(STAGE,{recursive:true});assert.equal(fs.realpathSync(STAGE),STAGE);
 const file=path.join(STAGE,name);assert(!fs.existsSync(file)||!fs.lstatSync(file).isSymbolicLink(),'Output symlink rejected');fs.writeFileSync(file,data);
}
function main(){
 assert(process.argv.slice(2).every(x=>x==='--self-test'),'Only --self-test accepted; no apply mode');
 const input=readInputs(),p=prepare(input),report=tests(p,input);
 for(const[key,item]of Object.entries(INPUTS))assert.equal(sha(fs.readFileSync(item.path)),item.sha256,key+' changed during preparation');
 if(!process.argv.includes('--self-test')){
  const manifest={schemaVersion:1,stageOnly:true,activated:false,measured:false,target25PercentVerified:false,inputs:INPUTS,
   preparedPolicySha256:sha(p.policy),rollbackPolicySha256:sha(input.policy),statusGlobal:STATE,
   target:{factoryId:'645582',utf16Start:7183512,utf16End:7186714,rawFactorySha256:sha(p.raw),anchorOffset:7185354,anchor:ANCHOR,replacement:REPLACEMENT},
   guards:{staticFullInputHashes:true,runtimeWholeFactoryHash:false,runtimeExactConnectionHandlerAndDependencies:true,uniqueAnchor:true,originalFactoryIdPattern:true,cacheGuard:true,driftAction:'return source unchanged'},
   preserved:['existing Nitro and commercial extras','single Vencord setter','native IPC and preload','current quest fetch and recurring interval','progress and heartbeats','manual hero API and view loader','manager exports and cleanup','all chunks','chat, voice, capture and plugin code'],
   stateMeaning:{registered:'queued with Vencord',matched:'source transformed',applied:'patched connection handler reached the eligible advertising branch; not proof the entire application later completed without fallback',error:'guard or registration failure',late:'Quest factory already cached before registration'},
   expectedEffect:'Skip automatic QUEST_HOME_BANNER_DESKTOP decision request and its retained creative/metadata model until manual loading.',
   limitations:['RAM savings unknown; metadata-only evidence does not establish an 8.44 MiB reduction.','Manual Quests banner may appear later because it is no longer prefetched.','Manual route loader and action APIs remain unchanged, but have not been interactively tested for this staged policy.','Other automatic advertisement paths are unchanged. No full QuestFetchManager disable, fake progress or shared chunk removal.'],
   promotion:'Review and explicitly promote renderer-policy.js later. This preparer never activates it. renderer-policy.base.js restores the prior commercial policy after a candidate restart.',
   tests:report};
  safeWrite('renderer-policy.base.js',input.policy);safeWrite('renderer-policy.js',p.policy);safeWrite('manifest.json',JSON.stringify(manifest,null,2)+'\n');safeWrite('self-check.json',JSON.stringify(report,null,2)+'\n');
 }
 console.log(JSON.stringify({stageOnly:true,activated:false,preparedPolicySha256:sha(p.policy),output:process.argv.includes('--self-test')?null:STAGE,...report},null,2));
}
if(require.main===module)main();
module.exports={prepare,tests,readInputs,INPUTS,STATE};
