'use strict';
// Staging only. Defer ContentInventory automatic boot/focus/idle requests until an explicit feed action.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),vm=require('node:vm'),assert=require('node:assert/strict');
const ROOT=path.resolve(__dirname,'..'),NATIVE=path.join(ROOT,'native-candidate'),STAGE=path.join(NATIVE,'prepared-content-inventory');
const INPUTS={
 web:{path:path.join(ROOT,'evidence/public-web/web.4d4b2f75fdea5470.js'),sha256:'667a771c61ff709e2f0400d395a324f0a5b1b5aa89e11e263289144a12d084ae'},
 policy:{path:path.join(NATIVE,'host/renderer-policy.js'),sha256:'30dba1b858d66dd7561acbea0901701ab7ae47a1ce1cabb0bb041ded57705685'},
 renderer:{path:path.join(NATIVE,'vencord/dist/renderer.js'),sha256:'e1283ec8d13bbbbbd1b2fbc6d3ea5e60eb7a9fbf314dd077644dcdb41f285f08'},
 preload:{path:path.join(NATIVE,'vencord/dist/preload.js'),sha256:'a05b8c9f6ef86d25ecf00cb6bf6e5fdc8ebde7798a76b97918ae0bb59edab1cb'}
};
const sha=x=>crypto.createHash('sha256').update(x).digest('hex'),STATE='__notDiscordNativeContentInventoryState',INSERT='            state.registered = true;';
const CLASS='class F extends s.A{actions={';
const ACTIONS='POST_CONNECTION_OPEN:P,CONNECTION_CLOSED:U,WINDOW_FOCUS:M,IDLE:M,CONTENT_INVENTORY_TOGGLE_FEED_HIDDEN:M,CONTENT_INVENTORY_MANUAL_REFRESH:w,CONTENT_INVENTORY_INBOX_STALE:G,SPOTIFY_NEW_TRACK:x,GAME_PROFILE_OPEN:k';
const NEW_ACTIONS='POST_CONNECTION_OPEN:()=>notDiscordContentInventoryAuto(P),CONNECTION_CLOSED:U,WINDOW_FOCUS:()=>notDiscordContentInventoryAuto(M),IDLE:()=>notDiscordContentInventoryAuto(M),CONTENT_INVENTORY_TOGGLE_FEED_HIDDEN:()=>notDiscordContentInventoryManual(M),CONTENT_INVENTORY_MANUAL_REFRESH:e=>notDiscordContentInventoryManual(w,e),CONTENT_INVENTORY_INBOX_STALE:G,SPOTIFY_NEW_TRACK:x,GAME_PROFILE_OPEN:()=>notDiscordContentInventoryManual(k)';
const HELPERS='let notDiscordContentInventoryActivated=!1;try{window.'+STATE+'.applied=true}catch{}function notDiscordContentInventoryAuto(callback){if(notDiscordContentInventoryActivated)return callback();try{window.'+STATE+'.suppressedAutomatic=true}catch{}}function notDiscordContentInventoryManual(callback,event){notDiscordContentInventoryActivated=!0;try{window.'+STATE+'.activated=true}catch{}return callback(event)}';
const DIAGNOSTIC='(()=>{"use strict";const out={storeAvailable:false,feedCount:null,entryCount:null,feedsWithEntries:null,policyInstalled:false,registered:false,matched:false,applied:false,suppressedAutomatic:false,activated:false,policyError:false,late:false,diagnosticError:false};try{const status=window.'+STATE+';out.policyInstalled=status?.installed===true;for(const key of ["registered","matched","applied","suppressedAutomatic","activated","late"])out[key]=status?.[key]===true;out.policyError=status?.error===true;const store=window.Vencord?.Webpack?.wreq?.c?.["99753"]?.exports?.A;if(typeof store?.getFeeds!=="function")return out;const feeds=store.getFeeds();if(typeof feeds?.values!=="function"||typeof feeds?.size!=="number")return out;out.storeAvailable=true;out.feedCount=feeds.size;let entries=0,nonempty=0;for(const feed of feeds.values()){if(!Array.isArray(feed?.entries))continue;entries+=feed.entries.length;if(feed.entries.length>0)nonempty++}out.entryCount=entries;out.feedsWithEntries=nonempty}catch{out.diagnosticError=true}return out})()';
function unique(s,a){const i=s.indexOf(a);return i>=0&&s.indexOf(a,i+a.length)<0;}
function only(s,a,b){assert(unique(s,a),'Unique anchor mismatch: '+a.slice(0,100));return s.replace(a,()=>b);}
function readInputs(){return Object.fromEntries(Object.entries(INPUTS).map(([k,v])=>{const b=fs.readFileSync(v.path);assert.equal(sha(b),v.sha256,k+' SHA mismatch');return[k,b];}));}
function parsePolicy(bytes){const s=bytes.toString('utf8'),a='module.exports=Object.freeze(';assert(unique(s,a)&&s.trimEnd().endsWith(');'));return JSON.parse(s.slice(s.indexOf(a)+a.length,s.lastIndexOf(');')));}
function prepare(input){
 for(const[k,v]of Object.entries(INPUTS))assert.equal(sha(input[k]),v.sha256,k+' SHA mismatch');
 const raw=input.web.toString('utf8').slice(4065486,4068163);
 assert.equal(sha(raw),'069b4eb07e10a36bebd8ad9abc8945e5a120c39ee7ebb1c3b5a207362191877c','ContentInventory factory SHA mismatch');
 assert(raw.startsWith('819235(e,t,n){'));assert(unique(raw,ACTIONS)&&unique(raw,CLASS));
 const guards=[ACTIONS,CLASS,'E=n(86070),A=n(435738),h=n(99753),f=n(424994)','function M(){v()}function P(){M()}function U(){D(T)}function w(e){let{feedId:t,feature:n}=e;D(t),b({feedId:t,feature:n,force:!0})}','function k(){let e=h.A.getFeed(T);null!=e&&new Date(e.expired_at).getTime()>Date.now()||b({feedId:T,feature:r.M.GAME_PROFILE})}'];
 for(const g of guards)assert(unique(raw,g),'Local guard mismatch');
 const edits=[[CLASS,HELPERS+CLASS],[ACTIONS,NEW_ACTIONS]],normalized='0,function'+raw.slice(raw.indexOf('('));
 let transformed=normalized;for(const edit of edits)transformed=only(transformed,...edit);
 new vm.Script(normalized);new vm.Script(transformed);new vm.Script(DIAGNOSTIC);
 const base=parsePolicy(input.policy);assert.equal(base.version,1);assert.equal(base.questHeroVersion,1);
 const code=[
 '            // Optional activity/news feed: automatic events wait until an explicit feed action.',
 '            const inventoryState={installed:true,registered:false,matched:false,applied:false,suppressedAutomatic:false,activated:false,error:false,late:false};',
 '            try {',
 '                if(Object.getOwnPropertyDescriptor(window,'+JSON.stringify(STATE)+'))throw new Error("Existing ContentInventory state");',
 '                Object.defineProperty(window,'+JSON.stringify(STATE)+',{value:inventoryState,configurable:true});',
 '                if(vencord.Webpack?.wreq?.c&&Object.prototype.hasOwnProperty.call(vencord.Webpack.wreq.c,"819235"))inventoryState.late=true;',
 '                else{',
 '                    const inventoryGuards='+JSON.stringify(guards)+',inventoryEdits='+JSON.stringify(edits)+';',
 '                    vencord.Plugins.addPatch({find:/^819235\\(e,t,n\\)\\{/,replacement:{match:/^[\\s\\S]*$/,replace(source){',
 '                        const unique=a=>{const i=source.indexOf(a);return i>=0&&source.indexOf(a,i+a.length)<0;};',
 '                        if(!inventoryGuards.every(unique)||!inventoryEdits.every(e=>unique(e[0]))){inventoryState.error=true;return source;}',
 '                        let changed=source;for(const[from,to]of inventoryEdits)changed=changed.replace(from,()=>to);',
 '                        inventoryState.matched=true;return changed;',
 '                    }}},"NotDiscordNativeContentInventory");',
 '                    inventoryState.registered=true;',
 '                }',
 '            }catch{inventoryState.error=true;}',
 ''
 ].join('\n');
 const before=only(base.before,INSERT,code+INSERT);assert.equal(before.replace(code,''),base.before);
 const policy='"use strict";\n// STAGED ONLY. ContentInventory activity/news feed on explicit demand.\nmodule.exports=Object.freeze('+JSON.stringify({...base,contentInventoryVersion:1,before})+');\n';
 new vm.Script(before);new vm.Script(policy);return{raw,normalized,transformed,guards,edits,base,code,before,policy};
}
function nativeHelpers(s){const a=s.indexOf('function Kt('),b=s.indexOf('var zr=',a),c=s.indexOf('function JS('),d=s.indexOf('function Hd(',c);assert(a>=0&&b>a&&c>=0&&d>c);return'var gs=[];'+s.slice(a,b)+s.slice(c,d);}
function fixture(p,input,{cache=[],throwAt=0,origin='https://discord.com',frame=false,skipAssign=false}={}){
 const listeners=new Map(),styles=[],timers=new Map();let seq=0;
 const document={readyState:'loading',head:{appendChild(x){styles.push(x)}},getElementById(id){return styles.find(x=>x.id===id)||null},createElement(){return{}},addEventListener(k,fn){listeners.set(k,[...(listeners.get(k)||[]),fn])},removeEventListener(k,fn){listeners.set(k,(listeners.get(k)||[]).filter(x=>x!==fn))}};
 const context=vm.createContext({location:{origin},document,setTimeout(fn){timers.set(++seq,fn);return seq},clearTimeout(id){timers.delete(id)}});
 vm.runInContext('window=globalThis;self=window;top=window;',context);if(frame)context.top={};
 vm.runInContext(nativeHelpers(input.renderer.toString('utf8')),context);
 context.cache=Object.fromEntries(cache.map(id=>[id,{}]));context.throwAt=throwAt;
 vm.runInContext('var callCount=0,testVencord={Plugins:{addPatch(p,n){if(++callCount===throwAt)throw Error("fixture registration");return JS(p,n)}},Webpack:{wreq:{c:cache}}};',context);
 vm.runInContext(p.before,context,{timeout:1000});if(!skipAssign)vm.runInContext('var Vencord=testVencord;',context);
 return{context,patches:context.gs,state:context[STATE],timers};
}
function apply(patches,raw,source){let out=source;for(const p of patches){if(!(typeof p.find==='string'?raw.includes(p.find):p.find.test(raw)))continue;for(const r of p.replacement)out=out.replace(r.match,r.replace);}return out;}
function actionsFixture(){
 const context=vm.createContext({});vm.runInContext('window=globalThis;var calls=[],argSeen=null,fail=false;function record(name,arg){calls.push(name);argSeen=arg;if(fail)throw Error("original callback failure");return name}function P(){return record("P")}function M(){return record("M")}function U(){return record("U")}function w(e){return record("w",e)}function G(e){return record("G",e)}function x(e){return record("x",e)}function k(){return record("k")}window.'+STATE+'={applied:false,suppressedAutomatic:false,activated:false};'+HELPERS+';var actions={'+NEW_ACTIONS+'};var originalActions={'+ACTIONS+'};',context);
 return context;
}
function tests(p,input){
 const names=[],check=(name,fn)=>{fn();names.push(name);};
 check('real native patch normalizers register five patches using the single existing setter',()=>{const f=fixture(p,input);assert.equal(f.patches.length,5);assert(f.state.registered);assert(!f.state.applied);assert.equal((p.before.match(/set: assignedVencord/g)||[]).length,1);assert.equal(f.timers.size,0);assert.equal(Object.getOwnPropertyDescriptor(f.context,'Vencord').value,f.context.testVencord);});
 check('whole target factory compiles and only helper insertion/action wrappers differ',()=>{const f=fixture(p,input),actual=apply(f.patches,p.raw,p.normalized);assert.equal(actual,p.transformed);new vm.Script(actual);assert.equal(f.state.matched,true);assert.equal(f.state.applied,false);let restored=actual;restored=only(restored,NEW_ACTIONS,ACTIONS);restored=only(restored,HELPERS,'');assert.equal(restored,p.normalized);});
 check('Nitro, commercial extras and Quest hero transformations remain identical',()=>{const current=fixture(p,input),base=fixture({...p,before:p.base.before},input),web=input.web.toString('utf8');for(const[a,b]of [[981009,2265906],[2916046,2942074],[7183512,7186714]]){const raw=web.slice(a,b),normalized='0,function'+raw.slice(raw.indexOf('('));assert.equal(apply(current.patches,raw,normalized),apply(base.patches,raw,normalized));}});
 check('unrelated plugin modifications are preserved',()=>{const f=fixture(p,input);assert.equal(apply(f.patches,p.raw,p.normalized+'/*other*/'),p.transformed+'/*other*/');});
 for(let i=0;i<p.guards.length;i++)check('guard '+i+' drift preserves source unchanged',()=>{const f=fixture(p,input),g=p.guards[i],altered=p.normalized.replace(g,()=>g.slice(0,1)+'/*drift*/'+g.slice(1));assert.equal(apply(f.patches,p.raw,altered),altered);assert(f.state.error&&!f.state.matched);});
 check('duplicate action anchor fails unchanged',()=>{const f=fixture(p,input),altered=p.normalized+'/*'+ACTIONS+'*/';assert.equal(apply(f.patches,p.raw,altered),altered);assert(f.state.error);});
 check('different original module identity is ignored',()=>{const f=fixture(p,input);assert.equal(apply(f.patches,p.raw.replace('819235','999999'),p.normalized),p.normalized);assert(!f.state.matched);});
 check('late target skips only inventory registration',()=>{const f=fixture(p,input,{cache:['819235']});assert.equal(f.patches.length,4);assert(f.state.late&&!f.state.registered);assert(f.context.__notDiscordNativeQuestHeroState.registered);});
 check('registration error leaves prior patches registered',()=>{const f=fixture(p,input,{throwAt:5});assert.equal(f.patches.length,4);assert(f.state.error&&!f.state.registered);assert(f.context.__notDiscordCommercialPreloadState.registered);});
 check('automatic boot focus and idle do not call originals before explicit action',()=>{const f=actionsFixture();for(const name of ['POST_CONNECTION_OPEN','WINDOW_FOCUS','IDLE']){assert.equal(f.actions[name](),undefined);}assert.equal(f.calls.length,0);assert(f[STATE].applied&&f[STATE].suppressedAutomatic&&!f[STATE].activated);});
 for(const [event,expected]of [['CONTENT_INVENTORY_TOGGLE_FEED_HIDDEN','M'],['CONTENT_INVENTORY_MANUAL_REFRESH','w'],['GAME_PROFILE_OPEN','k']])check(event+' activates and restores original automatic behavior including reconnect',()=>{const f=actionsFixture(),eventData={opaque:true};assert.equal(f.actions[event](eventData),expected);if(event==='CONTENT_INVENTORY_MANUAL_REFRESH')assert.equal(f.argSeen,eventData);assert(f[STATE].activated);f.actions.POST_CONNECTION_OPEN();f.actions.WINDOW_FOCUS();f.actions.IDLE();f.actions.CONNECTION_CLOSED();f.actions.POST_CONNECTION_OPEN();assert.deepEqual(Array.from(f.calls),[expected,'P','M','M','U','P']);});
 check('cleanup Spotify and stale-inbox callbacks keep identity and do not activate the gate',()=>{const f=actionsFixture(),data={opaque:true};for(const name of ['CONNECTION_CLOSED','SPOTIFY_NEW_TRACK','CONTENT_INVENTORY_INBOX_STALE']){assert.equal(f.actions[name],f.originalActions[name]);f.actions[name](data);}assert.deepEqual(Array.from(f.calls),['U','x','G']);assert(!f[STATE].activated);});
 check('activation occurs before original callback and original errors propagate',()=>{const f=actionsFixture();f.fail=true;assert.throws(()=>f.actions.CONTENT_INVENTORY_MANUAL_REFRESH({}),/original callback failure/);assert(f[STATE].activated);assert.throws(()=>f.actions.WINDOW_FOCUS(),/original callback failure/);});
 check('policy repeat is idempotent',()=>{const f=fixture(p,input);vm.runInContext(p.before,f.context);assert.equal(f.patches.length,5);});
 check('origin and frame guards preserved',()=>{for(const options of [{origin:'https://example.invalid'},{frame:true}]){const f=fixture(p,input,options);assert.equal(f.patches.length,0);assert.equal(f.state,undefined);}});
 check('existing assignment timeout cleans up',()=>{const f=fixture(p,input,{skipAssign:true});for(const cb of [...f.timers.values()])cb();assert(f.context.__notDiscordCommercialPreloadState.expired);assert.equal(f.state,undefined);assert.equal(f.timers.size,0);});
 check('diagnostic reports counts without touching entry contents or keys',()=>{const secret=new Proxy({},{get(){throw Error('entry content must not be read')}}),feeds=new Map([['PRIVATE_KEY',[secret,secret]],['ANOTHER_PRIVATE_KEY',[]]].map(([key,entries])=>[key,{entries}]));const context=vm.createContext({Vencord:{Webpack:{wreq:{c:{99753:{exports:{A:{getFeeds:()=>feeds}}}}}}}});context.window=context;const result=vm.runInContext(DIAGNOSTIC,context);assert.equal(result.storeAvailable,true);assert.equal(result.feedCount,2);assert.equal(result.entryCount,2);assert.equal(result.feedsWithEntries,1);assert(!result.diagnosticError);assert(!JSON.stringify(result).includes('PRIVATE'));assert(Object.values(result).every(v=>v===null||typeof v==='boolean'||typeof v==='number'));});
 check('diagnostic unavailable means null counts, not zero',()=>{const c=vm.createContext({});c.window=c;const r=vm.runInContext(DIAGNOSTIC,c);assert(!r.storeAvailable&&!r.diagnosticError);assert.equal(r.entryCount,null);assert.equal(r.feedCount,null);});
 check('diagnostic errors disclose no exception details',()=>{const c=vm.createContext({Vencord:{Webpack:{wreq:{c:{99753:{exports:{A:{getFeeds(){throw Error('PRIVATE_EXCEPTION')}}}}}}}}});c.window=c;const r=vm.runInContext(DIAGNOSTIC,c);assert(r.diagnosticError);assert(!JSON.stringify(r).includes('PRIVATE_EXCEPTION'));});
 for(const k of Object.keys(INPUTS))check(k+' input digest mismatch stops preparation',()=>assert.throws(()=>prepare({...input,[k]:Buffer.concat([input[k],Buffer.from('\n')])}),/SHA mismatch/));
 check('base renderer policy and all communication/progress/tracker code remain outside edits',()=>{assert.equal(p.before.replace(p.code,''),p.base.before);assert(p.transformed.includes('n.d(t,{A:()=>B})'));assert(!HELPERS.includes('setInterval')&&!HELPERS.includes('setTimeout')&&!HELPERS.includes('fetch('));});
 return{passed:true,checks:names.length,names,mode:'isolated real Vencord normalizers/action fixtures and compile-only full factories; no client or profile access'};
}
function safeWrite(name,data){assert.equal(path.basename(name),name);assert(!fs.existsSync(STAGE)||fs.realpathSync(STAGE)===STAGE,'Redirected staging path');fs.mkdirSync(STAGE,{recursive:true});assert.equal(fs.realpathSync(STAGE),STAGE);const file=path.join(STAGE,name);assert(!fs.existsSync(file)||!fs.lstatSync(file).isSymbolicLink(),'Output symlink');fs.writeFileSync(file,data);}
function main(){
 assert(process.argv.slice(2).every(x=>x==='--self-test'),'Only --self-test accepted; no apply mode');
 const input=readInputs(),p=prepare(input),report=tests(p,input);for(const[k,v]of Object.entries(INPUTS))assert.equal(sha(fs.readFileSync(v.path)),v.sha256,k+' changed while preparing');
 if(!process.argv.includes('--self-test')){
  const manifest={schemaVersion:1,stageOnly:true,activated:false,measured:false,target25PercentVerified:false,inputs:INPUTS,preparedPolicySha256:sha(p.policy),rollbackPolicySha256:sha(input.policy),statusGlobal:STATE,
   sizeBytes:{activePolicy:input.policy.length,preparedPolicy:Buffer.byteLength(p.policy),activeBefore:Buffer.byteLength(p.base.before),preparedBefore:Buffer.byteLength(p.before),addedBefore:Buffer.byteLength(p.code),addedFactoryHelpers:Buffer.byteLength(HELPERS),nativePreload:input.preload.length,vendorRenderer:input.renderer.length},
   target:{factoryId:'819235',utf16Start:4065486,utf16End:4068163,rawFactorySha256:sha(p.raw),actionAnchorOffset:4067939,actionAnchor:ACTIONS,actionReplacement:NEW_ACTIONS,helperInsertion:HELPERS},
   guards:{staticFullInputHashes:true,runtimeWholeFactoryHash:false,runtimeExactLocalBlocksAndUniqueAnchors:true,rawFactoryIdPattern:true,cacheGuard:true,driftAction:'source unchanged'},
   automaticEventsDeferred:['POST_CONNECTION_OPEN','WINDOW_FOCUS','IDLE'],activationEvents:['CONTENT_INVENTORY_TOGGLE_FEED_HIDDEN','CONTENT_INVENTORY_MANUAL_REFRESH','GAME_PROFILE_OPEN'],
   preserved:['original functions P/M/U/w/G/x/k','cleanup and stale-inbox behavior','Spotify callback','recurrence/backoff after explicit activation','manager exports and other modules','voice/presence trackers and GameActivityToggle code','chat/voice/capture and all plugin vendor bytes','prior Nitro/commercial/Quest policy and single setter'],
   stateMeaning:{registered:'queued with Vencord',matched:'source transformed',applied:'injected factory helper declarations reached; not proof the entire app completed',suppressedAutomatic:'at least one boot/focus/idle event deferred',activated:'an explicit activation action was dispatched; not proof of human input'},
   expectedEffect:'Avoid initial complete activity-feed response retention (99753), matching activity map/scans (574520), and automatic expiration refreshes (819235/86070) before feed use.',
   limitations:['Savings unmeasured; source alone cannot substantiate the roughly 34.4 MiB needed in the latest channel comparison.','The activity/news feed is not prefetched; a view that relies only on background preload may remain empty until its explicit toggle/refresh/game-profile action.','Automatic events resume after activation, including reconnects. Other loading paths and plugins are intentionally unchanged.','Spotify publication, presence, voice and GameActivityToggle remain unchanged.','The diagnostic returns only counts/booleans, reads already loaded store exports, never serializes entries or reports IDs. Counts do not measure retained bytes or prove causal RAM savings.','Store modules and their code remain allocated; this does not unload shared chunks.'],
   diagnostic:{file:'diagnostic.js',executedAgainstClient:false,reads:'cached factory99753 exports.A.getFeeds; only Map size and entries array lengths',missingCounts:null},
   promotion:'Review staged policy; tool has no apply mode. Restore renderer-policy.base.js and restart candidate to reverse after any later promotion.',tests:report};
  safeWrite('renderer-policy.base.js',input.policy);safeWrite('renderer-policy.js',p.policy);safeWrite('manifest.json',JSON.stringify(manifest,null,2)+'\n');safeWrite('self-check.json',JSON.stringify(report,null,2)+'\n');safeWrite('diagnostic.js',DIAGNOSTIC+'\n');
 }
 console.log(JSON.stringify({stageOnly:true,activated:false,output:process.argv.includes('--self-test')?null:STAGE,preparedPolicySha256:sha(p.policy),...report},null,2));
}
if(require.main===module)main();module.exports={prepare,tests,readInputs,INPUTS,STATE,DIAGNOSTIC};
