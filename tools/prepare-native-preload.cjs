'use strict';
// Native candidate only. Pin all inputs, preserve native IPC/preload, and fail before writes on mismatch.
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto'),vm=require('node:vm'),assert=require('node:assert/strict');
const ROOT=path.resolve(__dirname,'..'),CANDIDATE=path.join(ROOT,'native-candidate');
const SOURCE='C:/Users/sssli/AppData/Roaming/Vencord/dist/preload.js';
const OUTPUT=path.join(CANDIDATE,'vencord','dist','preload.js');
const RENDERER=path.join(CANDIDATE,'vencord','dist','renderer.js');
const POLICY=path.join(CANDIDATE,'host','renderer-policy.js');
const SOURCE_SHA='2b6a28c8b54a847056aee1e0bac70bcbff68cab0cd4d45d6b328572baea0bebd';
const RENDERER_SHA='e1283ec8d13bbbbbd1b2fbc6d3ea5e60eb7a9fbf314dd077644dcdb41f285f08';
const COMMERCIAL_SHA='2abb5bb0b793be8c10242154e71661c3c547ecfeb8c9b0db6706f96b635ee155';
const LEAN_SHA='e45d4eb4a16d1a787f517e80b46deaa5183aceb4ac6b7eac65685cf680675c6a';
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const ANCHOR='t.webFrame.executeJavaScript(i("VencordPreloadGetRendererJs"))';
const ROUTES=['/shop','/store','/quest-home','/discovery/quests'];
function extractScript(source) {
  const match=/internal const string Script = """\r?\n([\s\S]*?)\r?\n    """;/.exec(source);
  assert(match,'C# raw Script not found');
  return match[1].split(/\r?\n/).map(line=>line.startsWith('    ')?line.slice(4):line).join('\n');
}
function cssScript() {
  return `(()=>{"use strict";
if(location.origin!=="https://discord.com"||window.top!==window.self)return;
const stateName="__notDiscordNativeLeanState",styleId="notdiscord-native-lean-style";
if(window[stateName]?.version===1)return;
const state={version:1,styleInstalled:false,paths:${JSON.stringify(ROUTES)},settingsChanged:false};
window[stateName]=state;
function install(){
 if(!document.head)return;
 if(document.getElementById(styleId)){state.styleInstalled=true;return;}
 const selectors=[];
 for(const route of state.paths)for(const href of [route,"https://discord.com"+route]){
   selectors.push('nav a[href="'+href+'"]');
   selectors.push('[role="navigation"] a[href="'+href+'"]');
 }
 const style=document.createElement("style");style.id=styleId;
 style.textContent=selectors.join(",\\n")+" { display: none !important; }";
 document.head.appendChild(style);state.styleInstalled=true;
}
if(document.readyState==="loading")document.addEventListener("DOMContentLoaded",install,{once:true});else install();
})();void 0;`;
}
function injection() {
  return `((rendererCode)=>{let started=false;const runRenderer=()=>{if(started)return;started=true;try{const p=t.webFrame.executeJavaScript(rendererCode);if(p&&typeof p.catch==="function")p.catch(()=>console.warn("NotDiscord: Vencord renderer execution failed"))}catch{console.warn("NotDiscord: Vencord renderer could not start")}};let policy;try{policy=require(require("node:path").join(__dirname,"..","..","host","renderer-policy.js"));if(policy.version!==1||typeof policy.before!=="string")throw new Error("Unsupported renderer policy")}catch{return runRenderer()}try{const p=t.webFrame.executeJavaScript(policy.before,false,runRenderer);if(p&&typeof p.catch==="function")p.catch(runRenderer)}catch{runRenderer()}})(i("VencordPreloadGetRendererJs"))`;
}
function prepare(original,renderer,commercialCs,leanCs) {
  assert.equal(sha(original),SOURCE_SHA,'Vencord preload SHA mismatch');
  assert.equal(sha(renderer),RENDERER_SHA,'Vencord renderer SHA mismatch');
  assert.equal(sha(commercialCs),COMMERCIAL_SHA,'CommercialPreload.cs SHA mismatch; review the policy first');
  assert.equal(sha(leanCs),LEAN_SHA,'LeanInterface.cs SHA mismatch; review the CSS routes first');
  const source=original.toString('utf8');assert(source.startsWith('// Vencord 32c9063\n'),'Vencord build mismatch');
  const offset=source.indexOf(ANCHOR);assert(offset>=0&&source.indexOf(ANCHOR,offset+1)===-1,'Preload injection anchor mismatch');
  const commercial=extractScript(commercialCs.toString('utf8')),lean=cssScript();
  for(const route of ROUTES)assert(leanCs.toString('utf8').includes(JSON.stringify(route)),'Unverified lean route');
  const before= commercial+'\n;\n'+lean+'\n;void 0;';
  const policy='"use strict";\n// NotDiscord native renderer policy v1; generated from pinned local sources.\nmodule.exports=Object.freeze('+JSON.stringify({version:1,before})+');\n';
  const replacement=injection();
  const patched=source.slice(0,offset)+replacement+source.slice(offset+ANCHOR.length);
  new vm.Script(patched,{filename:'native-preload.syntax-only.js'});
  new vm.Script(before,{filename:'native-renderer-policy.syntax-only.js'});
  new vm.Script(policy,{filename:'native-renderer-policy-module.syntax-only.js'});
  return {patched,policy,before,commercial,lean,replacement,offset,source};
}
function fixture(policyScript,{origin='https://discord.com',frame=false,ready='loading'}={}) {
  const listeners=new Map(),styles=[],timers=new Map();let id=0;
  const document={readyState:ready,head:{appendChild(x){styles.push(x)}},getElementById(name){return styles.find(x=>x.id===name)||null},createElement(tag){assert.equal(tag,'style');return{}},addEventListener(name,callback){const items=listeners.get(name)||[];items.push(callback);listeners.set(name,items)},removeEventListener(name,callback){listeners.set(name,(listeners.get(name)||[]).filter(x=>x!==callback))}};
  const context=vm.createContext({location:{origin},document,setTimeout(callback){timers.set(++id,callback);return id},clearTimeout(key){timers.delete(key)}});
  vm.runInContext('window=globalThis;window.self=window;window.top=window;',context);
  if(frame)context.top={};
  new vm.Script(policyScript).runInContext(context,{timeout:1000});
  return {context,document,styles,listeners,timers,ready(){for(const f of [...(listeners.get('DOMContentLoaded')||[])])f()}};
}
async function test(prepared,inputs) {
  const checks=[];async function check(name,fn){await fn();checks.push({name,passed:true})}
  await check('four pinned inputs verified',()=>{assert.equal(sha(inputs[0]),SOURCE_SHA);assert.equal(sha(inputs[1]),RENDERER_SHA);assert.equal(sha(inputs[2]),COMMERCIAL_SHA);assert.equal(sha(inputs[3]),LEAN_SHA)});
  await check('mismatched preload is rejected before writes',()=>assert.throws(()=>prepare(Buffer.from('other build'),...inputs.slice(1)),/SHA mismatch/));
  await check('only one executeJavaScript expression changes',()=>assert.equal(prepared.patched.replace(prepared.replacement,ANCHOR),prepared.source));
  await check('DISCORD_PRELOAD and data branch preserved',()=>assert(prepared.patched.endsWith(prepared.source.slice(prepared.offset+ANCHOR.length))));
  await check('all original IPC names and bridge exports unchanged',()=>{const tokens=s=>s.match(/"Vencord[A-Za-z]+"/g)||[];assert.deepEqual(tokens(prepared.patched),tokens(prepared.source));assert.equal((prepared.patched.match(/contextBridge\.exposeInMainWorld/g)||[]).length,(prepared.source.match(/contextBridge\.exposeInMainWorld/g)||[]).length)});
  async function order(mode) {
    const events=[],callbacks=[];let query=0,rendererCount=0;
    const context=vm.createContext({__dirname:path.dirname(OUTPUT),console:{warn(){events.push('warn')}},require(name){if(name==='node:path')return path;if(name===POLICY){if(mode==='missing')throw Error('Missing policy');return{version:mode==='badversion'?2:1,before:'HOOK'}};throw Error('Unexpected require '+name)},i(name){assert.equal(name,'VencordPreloadGetRendererJs');query++;events.push('query');return 'RENDERER'},t:{webFrame:{executeJavaScript(code,gesture,cb){if(code==='HOOK'){events.push('hook');if(mode==='throw')throw Error('hook failed');if(mode==='deferred'){callbacks.push(cb);return Promise.resolve()}if(mode==='reject')return Promise.reject(Error('hook failed'));cb();if(mode==='callback-reject')return Promise.reject(Error('after callback'));return Promise.resolve()}assert.equal(code,'RENDERER');rendererCount++;events.push('renderer');return Promise.resolve()}}},markCore(){events.push('core')}});
    new vm.Script(prepared.replacement+';markCore();').runInContext(context,{timeout:1000});
    if(mode==='deferred'){assert.deepEqual(events,['query','hook','core']);callbacks[0]();}
    await Promise.resolve();await Promise.resolve();
    assert.equal(query,1);assert.equal(rendererCount,1);assert.equal(events.filter(x=>x==='core').length,1);
    if(mode==='normal')assert.deepEqual(events,['query','hook','renderer','core']);
    return events;
  }
  for(const mode of ['normal','deferred','throw','reject','callback-reject','missing','badversion'])await check('renderer starts exactly once: '+mode,()=>order(mode));
  await check('commercial hook installed before separate Vencord var declaration',()=>{
    const f=fixture(prepared.before);f.context.__patches=[];
    vm.runInContext('var Vencord={Plugins:{addPatch:function(p,n){__patches.push({p,n})}},Webpack:{wreq:{c:{}}}};',f.context,{timeout:1000});
    assert.equal(f.context.__patches.length,1);assert.equal(f.context.__notDiscordCommercialPreloadState.registered,true);
    const descriptor=Object.getOwnPropertyDescriptor(f.context,'Vencord');assert('value'in descriptor);assert.equal(f.context.__patches[0].n,'NotDiscordLean');
  });
  await check('lean CSS waits for DOM ready and uses only commercial anchors',()=>{const f=fixture(prepared.before);assert.equal(f.styles.length,0);f.ready();assert.equal(f.styles.length,1);assert.equal(f.context.__notDiscordNativeLeanState.styleInstalled,true);assert.equal(f.context.__notDiscordNativeLeanState.settingsChanged,false);assert.equal((f.styles[0].textContent.match(/a\[href=/g)||[]).length,16);assert(!/channels|video|audio|canvas|button|img|input|textarea|animation|transition/.test(f.styles[0].textContent))});
  await check('lean CSS also supports already ready documents',()=>assert.equal(fixture(prepared.before,{ready:'complete'}).styles.length,1));
  await check('policy is idempotent',()=>{const f=fixture(prepared.before,{ready:'complete'});new vm.Script(prepared.before).runInContext(f.context,{timeout:1000});assert.equal(f.styles.length,1)});
  await check('foreign origins receive no hook or style',()=>{const f=fixture(prepared.before,{origin:'https://example.com',ready:'complete'});assert.equal(f.styles.length,0);assert.equal(f.context.__notDiscordCommercialPreloadState,undefined);assert.equal(f.context.__notDiscordNativeLeanState,undefined)});
  await check('subframes receive no hook or style',()=>{const f=fixture(prepared.before,{frame:true,ready:'complete'});assert.equal(f.styles.length,0);assert.equal(f.context.__notDiscordCommercialPreloadState,undefined)});
  await check('policy exposes no Node/IPC/network bridge in page',()=>assert(!/\brequire\s*\(|\bprocess\.|\bipcRenderer\b|contextBridge|\bfetch\s*\(|XMLHttpRequest|WebSocket/.test(prepared.before)));
  await check('native renderer payload is not rewritten',()=>assert.equal(sha(inputs[1]),RENDERER_SHA));
  return checks;
}
function secure(file) {
  const resolved=path.resolve(file),rel=path.relative(ROOT,resolved);assert(rel&&!rel.startsWith('..'+path.sep)&&rel!=='..'&&!path.isAbsolute(rel),'Write outside project');
  for(let p=resolved;;p=path.dirname(p)){if(fs.existsSync(p))assert(!fs.lstatSync(p).isSymbolicLink(),'Reparse path refused');if(path.dirname(p)===p)break;}
}
function write(file,data){secure(file);fs.mkdirSync(path.dirname(file),{recursive:true});secure(file);const temp=file+'.preparing-'+process.pid;fs.writeFileSync(temp,data,{flag:'wx'});try{fs.renameSync(temp,file)}catch(error){fs.unlinkSync(temp);throw error}}
async function main(args) {
  assert(args.length===0||(args.length===1&&args[0]==='--self-test'),'Only --self-test is accepted');const testOnly=args.length===1;
  const inputs=[fs.readFileSync(SOURCE),fs.readFileSync(RENDERER),fs.readFileSync(path.join(ROOT,'src','NotDiscord','CommercialPreload.cs')),fs.readFileSync(path.join(ROOT,'src','NotDiscord','LeanInterface.cs'))];
  const result=prepare(...inputs),checks=await test(result,inputs),outputSha=sha(result.patched);
  const existing=sha(fs.readFileSync(OUTPUT));assert(existing===SOURCE_SHA||existing===outputSha,'Copied preload is neither pinned original nor this policy output');
  secure(OUTPUT);secure(POLICY);
  const report={policyVersion:1,vencordBuild:'32c9063',createdUtc:new Date().toISOString(),source:SOURCE,sourceSha256:SOURCE_SHA,rendererSha256:RENDERER_SHA,commercialCsSha256:COMMERCIAL_SHA,leanCsSha256:LEAN_SHA,output:OUTPUT,outputSha256:outputSha,policy:POLICY,policySha256:sha(result.policy),anchorUtf16Offset:result.offset,checks,passed:checks.every(x=>x.passed),filesWritten:!testOnly,discordExecuted:false,originalRendererExecuted:false,scope:{commercialPreload:'Exact existing guarded Vencord patch',lean:'CSS navigation links only; no plugin settings, resource interception or media changes',ordering:'Separate webFrame.executeJavaScript calls; callback then renderer once; original Discord preload stays outside callback'},sources:['https://raw.githubusercontent.com/electron/electron/v42.7.1/docs/api/web-frame.md','https://raw.githubusercontent.com/electron/electron/v42.7.1/shell/renderer/api/electron_api_web_frame.cc'],limits:['No authenticated or native voice/screenshare test performed by this preparer','No measured RAM saving is attributed to CSS','A changed Discord web factory is left untouched by the commercial patch']};
  if(!testOnly){write(POLICY,result.policy);write(OUTPUT,result.patched);assert.equal(sha(fs.readFileSync(OUTPUT)),outputSha);assert.equal(sha(fs.readFileSync(SOURCE)),SOURCE_SHA);}
  const evidence=path.join(ROOT,'evidence','native','preload-policy'+(testOnly?'-self-test':'')+'.json');write(evidence,JSON.stringify(report,null,2)+'\n');
  console.log(JSON.stringify({passed:report.passed,checks:checks.length,filesWritten:!testOnly,output:OUTPUT,policy:POLICY,outputSha256:outputSha,evidence},null,2));
}
module.exports={prepare,test,injection,cssScript,extractScript};
if(require.main===module)main(process.argv.slice(2)).catch(error=>{console.error(error.stack||String(error));process.exitCode=1});

