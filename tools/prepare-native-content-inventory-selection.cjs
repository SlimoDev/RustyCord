'use strict';
// STAGING ONLY. Does not apply policies, open clients or read profile content.
const fs=require('fs'),path=require('path'),crypto=require('crypto'),assert=require('assert/strict'),vm=require('vm');
const ROOT='E:\\DESCARGAS\\REA\\NotDiscord';
const STAGE=path.join(ROOT,'native-candidate','prepared-content-inventory','selection');
const sha=b=>crypto.createHash('sha256').update(b).digest('hex');
const INPUTS={
 base:{path:path.join(ROOT,'native-candidate','host','renderer-policy.js'),sha256:'30dba1b858d66dd7561acbea0901701ab7ae47a1ce1cabb0bb041ded57705685'},
 prepared:{path:path.join(ROOT,'native-candidate','prepared-content-inventory','renderer-policy.js'),sha256:'3b59dd504c091bdd5e580f0da25d47568f418347d293d3f1c65bcbee8bf776ce'},
 web:{path:path.join(ROOT,'evidence','public-web','web.4d4b2f75fdea5470.js'),sha256:'667a771c61ff709e2f0400d395a324f0a5b1b5aa89e11e263289144a12d084ae'},
 channel:{path:path.join(ROOT,'evidence','public-web','bf3e6ee72c2d4db9.js'),sha256:'aae43c0c6474a7d71e80d3086ad38bc2b373c5dacf46f7100f917db354317769'}
};
const SELECTOR=[
'"use strict";',
'// Fixed sibling files only. This selects at preload startup; changing the flag needs a restart.',
'const fs=require("node:fs"),path=require("node:path");',
'const filename=path.join(__dirname,"activity-feed.json");',
'let activityFeedPreload=true,configurationValid=false;',
'try {',
' const stat=fs.statSync(filename);',
' if(!stat.isFile()||stat.size>4096)throw new Error("Invalid activity feed configuration");',
' const config=JSON.parse(fs.readFileSync(filename,"utf8"));',
' if(config===null||Array.isArray(config)||typeof config!=="object"||',
'    Object.keys(config).length!==1||typeof config.activityFeedPreload!=="boolean")',
'  throw new Error("Invalid activity feed configuration");',
' activityFeedPreload=config.activityFeedPreload;configurationValid=true;',
'} catch {}',
'const selected=activityFeedPreload?',
' require("./renderer-policy.base.js"):require("./renderer-policy.activity-disabled.js");',
'module.exports=Object.freeze({...selected,activityFeedSelection:Object.freeze({',
' version:1,activityFeedPreload,configurationValid,preloadSuppressed:!activityFeedPreload',
'})});',
''
].join('\n');
const CONFIG='{\n  "activityFeedPreload": false\n}\n';
function readInputs(){
 return Object.fromEntries(Object.entries(INPUTS).map(([k,v])=>{const b=fs.readFileSync(v.path);assert.equal(sha(b),v.sha256,k+' input changed');return[k,b];}));
}
function parsePolicy(bytes){
 const s=bytes.toString('utf8'),a='module.exports=Object.freeze(';
 assert(s.includes(a)&&s.trimEnd().endsWith(');'));
 return JSON.parse(s.slice(s.indexOf(a)+a.length,s.lastIndexOf(');')));
}
function anchor(s,text){const i=s.indexOf(text);assert(i>=0&&s.indexOf(text,i+text.length)<0,'Evidence anchor not unique: '+text);return{utf16Offset:i,source:text};}
function scope(input){
 const web=input.web.toString('utf8'),channel=input.channel.toString('utf8');
 return{
  schemaVersion:1,kind:'static shipped JavaScript evidence',observedRuntime:false,
  sources:{web:INPUTS.web,channel:{...INPUTS.channel,url:'https://discord.com/assets/bf3e6ee72c2d4db9.js',chunkId:'192461',factoryId:'284055'}},
  optionalFeed:{
   description:'Global activity/media/game feed. It can include friends activity and game or media recommendations; it is not exclusively advertising.',
   distinction:'The reported 50 entries are global retained entries, not necessarily visible member-list rows and not a retained-byte estimate.',
   channelFilters:anchor(channel,'return s.useMemo(()=>n?.filter(e=>d.has(e.author_id)),[n,d])'),
   emptyFeedPreservesMemberRows:anchor(channel,'if(null==c||0===c.length||null==d||!x)return[n,t,i]'),
   feedAppendedSeparately:anchor(channel,'return[[m,...n],[...t,m,...e],Math.random(),e]'),
   memberRow:anchor(channel,'if(r.type===oX.S9.MEMBER&&"user"in r)'),
   optionalFeedRow:anchor(channel,'if(r.type===oX.S9.CONTENT_INVENTORY){let e='),
   memberSubscription:anchor(channel,'(0,F.NJ)({guildId:e.guild_id,channelId:e.id,y:t,height:i,rowHeight:n})'),
   manualControlRequiresFeed:'The settings/toggle header is part of the feed group. Without initial entries that group and its controls can be absent; no on-open/manual autoload guarantee.',
   toggleCall:anchor(channel,'x=s.useCallback(()=>u?(0,oF.Il)():r(),[u,r])')
  },
  otherObservedGlobalFeedConsumers:[
   {factoryId:'365185',role:'Associates an activity/user with an inbox feed entry; falls back to an existing outbox match when the inbox has no match.',
    consequence:'Optional activity-card metadata/enrichment based on the global inbox may be unavailable; the existing outbox fallback remains.',
    evidence:anchor(web,'l=(0,i.bG)([a.A],()=>a.A.getMatchingInboxEntry({activity:t,userId:n.id,feedId:s.X1.GLOBAL_FEED}),[t,n.id]),o=(0,i.bG)([r.A],()=>r.A.getMatchingOutboxEntry({activity:t,userId:n.id}),[t,n.id]);return l??o')},
   {factoryId:'409626',role:'Game profile open/close analytics obtains the global-feed request ID.',
    consequence:'This optional analytics metadata can be absent. These reads do not control core chat or voice.',
    openEvidence:anchor(web,'request_id:o.A.getFeedRequestId(u.X1.GLOBAL_FEED),profile_type:s}),t'),
    closeEvidence:anchor(web,'request_id:o.A.getFeedRequestId(u.X1.GLOBAL_FEED),official_guild_id:_?l:void 0,guild_id:l')}
  ],
  membersAndPresence:{
   factoryId:'963307',activityStore:'290863',streamStore:'616356',
   evidence:anchor(web,'s=i?g.A.getStatus():m.A.getStatus(n,e),l=i?g.A.getActivities():m.A.getActivities(n,e),o=E.A.getStreamForUser(n,e)'),
   conclusion:'Member rows derive status, activities and streams independently of the content-inventory store. Their factories and subscription code are not changed.'
  },
  retainedBehavior:['Chat, voice and screen capture code is not edited.','GameActivityToggle and other Vencord plugin vendor bytes are unchanged.','Spotify publication, member presence and activity trackers are retained.','Explicit inventory toggle/manual-refresh/game-profile actions may populate the feed and resume automatic behavior.'],
  limitations:['Static code separation is not a real chat/call/screenshare test.','Only the local main bundle and focused Channel chunk were traced; other lazy global-feed consumers may exist.','The staged policy suppresses automatic startup/focus/idle loading until an explicit retained action; it is not a permanent total feed shutdown.','No memory saving is guaranteed. Store code and shared chunks remain present.']
 };
}
function selectorTests(input){
 const base=parsePolicy(input.base),disabled=parsePolicy(input.prepared),checks=[];
 const run=(config,options={})=>{
  const module={exports:{}},requires=[],reads=[],dir='E:\\fixed-test-host';
  const filename=path.join(dir,'activity-feed.json');
  const disk={
   statSync(p){assert.equal(p,filename);reads.push(p);if(options.missing)throw Error('missing');return{isFile:()=>!options.directory,size:options.oversize?4097:Buffer.byteLength(config)};},
   readFileSync(p,encoding){assert.equal(p,filename);assert.equal(encoding,'utf8');reads.push(p);return config;}
  };
  vm.runInNewContext(SELECTOR,{__dirname:dir,module,require(id){requires.push(id);if(id==='node:fs')return disk;if(id==='node:path')return path;if(id==='./renderer-policy.base.js')return base;if(id==='./renderer-policy.activity-disabled.js')return disabled;throw Error('Unexpected require '+id);}});
  const result=module.exports,expected=options.base?base:disabled;
  assert.equal(result.before,expected.before);
  for(const [key,value]of Object.entries(expected))assert.deepEqual(result[key],value);
  assert(Object.isFrozen(result)&&Object.isFrozen(result.activityFeedSelection));
  assert.equal(result.activityFeedSelection.activityFeedPreload,!!options.base);
  assert.equal(result.activityFeedSelection.preloadSuppressed,!options.base);
  assert.equal(result.activityFeedSelection.configurationValid,!options.invalid);
  assert.equal(requires.filter(x=>x.startsWith('./')).length,1);
  assert.equal(requires.at(-1),options.base?'./renderer-policy.base.js':'./renderer-policy.activity-disabled.js');
  assert(reads.length>=1);
  return result;
 };
 run(CONFIG);checks.push('false selects exact prepared policy with prior patches and immutable boolean metadata');
 run('{"activityFeedPreload":true}',{base:true});checks.push('true selects exact base policy without loading the disabled policy');
 const invalid=['{','null','[]','{}','{"activityFeedPreload":"false"}','{"activityFeedPreload":false,"path":"C:/ignored.js"}'];
 for(const text of invalid)run(text,{base:true,invalid:true});
 run('',{base:true,invalid:true,missing:true});run('{}',{base:true,invalid:true,oversize:true});run('{}',{base:true,invalid:true,directory:true});
 checks.push('malformed missing oversized or non-boolean config falls back to base; arbitrary paths cannot select files');
 return{passed:true,checks:checks.length,names:checks,invalidConfigVariants:invalid.length+3};
}
function safeWrite(name,value){
 assert.equal(path.basename(name),name);
 if(fs.existsSync(STAGE))assert.equal(fs.realpathSync(STAGE),STAGE,'Redirected selection staging');
 fs.mkdirSync(STAGE,{recursive:true});assert.equal(fs.realpathSync(STAGE),STAGE);
 const filename=path.join(STAGE,name);
 assert(!fs.existsSync(filename)||!fs.lstatSync(filename).isSymbolicLink(),'Output symlink');
 fs.writeFileSync(filename,value);
}
function main(){
 assert(process.argv.slice(2).every(x=>x==='--self-test'),'Only --self-test supported; no apply mode');
 const input=readInputs(),evidence=scope(input),report=selectorTests(input);
 const prior=require('./prepare-native-content-inventory.cjs');
 const patchInput=prior.readInputs(),prepared=prior.prepare(patchInput);
 assert.equal(sha(prepared.policy),INPUTS.prepared.sha256);
 const patchReport=prior.tests(prepared,patchInput);
 assert(patchReport.passed);
 const combined={passed:true,checks:report.checks+patchReport.checks,selector:report,patch:patchReport,mode:'Isolated selector fixtures, real Vencord normalizers and full-factory compilation; no client execution'};
 for(const [k,v]of Object.entries(INPUTS))assert.equal(sha(fs.readFileSync(v.path)),v.sha256,k+' changed during preparation');
 const outputs={'renderer-policy.js':SELECTOR,'renderer-policy.base.js':input.base,'renderer-policy.activity-disabled.js':input.prepared,'activity-feed.json':CONFIG};
 const manifest={
  schemaVersion:1,stageOnly:true,activated:false,measured:false,target25PercentVerified:false,
  inputs:INPUTS,files:Object.fromEntries(Object.entries(outputs).map(([name,bytes])=>[name,{sha256:sha(bytes),bytes:Buffer.byteLength(bytes)}])),
  deployment:'Four fixed sibling files are staged for later review. No host files are changed by this tool. No apply mode.',
  config:{filename:'activity-feed.json',key:'activityFeedPreload',stagedDefault:false,true:'Base policy: original automatic activity feed behavior',false:'Prepared policy: suppress automatic boot/focus/idle feed preload',invalidOrMissing:'Base policy',reload:'Full candidate restart required'},
  metadataExport:'activityFeedSelection (frozen booleans; host selection only, not runtime patch success)',
  scopeFile:'scope-evidence.json',testsFile:'self-check.json',
  caveats:['Optional feed panel and its controls may not appear; no manual/on-open autoload guarantee.','Retained explicit inventory/game-profile actions can reactivate the feed.','Set activityFeedPreload=true and restart to restore original feed behavior while retaining other optimizations.','Other global-feed metadata consumers are documented in scope-evidence.json.','Existing prepared-policy comment uses historical on-demand wording; it is not a user-visible functionality guarantee.'],
  tests:{passed:true,checks:combined.checks,selectorChecks:report.checks,patchChecks:patchReport.checks}
 };
 const documentation=[
 '# Selección provisional de la precarga de actividad',
 '',
 'Este directorio es staging: no se ha aplicado al cliente. El selector conserva los parches previos de Nitro, marketing y Quest.',
 '',
 'Los cuatro archivos de ejecución deben quedar como hermanos en host sólo tras una promoción revisada: renderer-policy.js, renderer-policy.base.js, renderer-policy.activity-disabled.js y activity-feed.json.',
 '',
 'La configuración admite una sola clave booleana, activityFeedPreload. false es el valor preparado: suprime la precarga automática del feed opcional de juegos/multimedia en arranque, foco e inactividad. true recupera la política base. Configuración ausente, malformada o con campos extra usa la base. El selector sólo lee archivos con nombres fijos; no admite rutas ni ejecutables en la configuración.',
 '',
 'El panel opcional puede no aparecer, incluidos sus controles. No se garantiza activación al abrirlo ni mediante un botón manual. Las acciones explícitas que aún existan, incluida apertura de perfiles de juego, pueden cargarlo y reanudar el comportamiento automático. Para recuperar el comportamiento original del feed, establecer activityFeedPreload en true y reiniciar completamente el candidato.',
 '',
 'La fuente de la vista conserva los grupos y filas normales cuando no hay feed. Miembros, estados, presencia y suscripciones siguen sus rutas originales. Chat, voz, captura y el código de los plugins no se modifican. Esto es evidencia estática, no una prueba funcional con una llamada real.',
 '',
 'El feed global no es exclusivamente publicidad: incluye actividad y multimedia. Además del panel, 365185 usa sus entradas como metadatos de actividad con fallback a outbox; 409626 usa su request ID en seguimiento de perfiles de juego. Puede haber más consumidores en fragmentos no examinados. Véase scope-evidence.json.',
 '',
 'activityFeedSelection informa sólo de la selección del host; los indicadores de aplicación real siguen en __notDiscordNativeContentInventoryState. Las 50 entradas observadas antes no equivalen a 50 filas visibles ni permiten calcular ahorro de RAM.',
 '',
 'Validación: 3 casos del selector (9 variantes inválidas) y 30 comprobaciones del parche con normalizadores reales de Vencord y compilación de fábricas. No se abrieron clientes ni perfiles. El ahorro de memoria queda pendiente.',
 ''
 ].join('\n');
 if(!process.argv.includes('--self-test')){
  for(const [name,bytes]of Object.entries(outputs))safeWrite(name,bytes);
  safeWrite('scope-evidence.json',JSON.stringify(evidence,null,2)+'\n');
  safeWrite('manifest.json',JSON.stringify(manifest,null,2)+'\n');
  safeWrite('self-check.json',JSON.stringify(combined,null,2)+'\n');
  safeWrite('README.md',documentation);
 }
 console.log(JSON.stringify({stageOnly:true,activated:false,output:process.argv.includes('--self-test')?null:STAGE,files:manifest.files,tests:manifest.tests},null,2));
}
if(require.main===module)main();
module.exports={SELECTOR,CONFIG,INPUTS,readInputs,selectorTests,scope};
