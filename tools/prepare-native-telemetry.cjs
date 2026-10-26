'use strict';
// Prepare an isolated, reviewable telemetry patch. This script NEVER modifies active runtime files.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const {unpack,repack} = require('./prepare-native-bootstrap.cjs');
const ROOT = path.resolve(__dirname, '..');
const CANDIDATE = path.join(ROOT, 'native-candidate');
const STAGED = path.join(CANDIDATE, 'prepared-telemetry');
const RESOURCES = path.join(CANDIDATE, 'runtime', 'resources');
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const ARTIFACTS = [
  {name:'bootstrap', source:path.join(RESOURCES,'_app.asar'), output:'resources/_app.asar', hash:'4acdcbc51450ec2a5cb9c7e5297f2f1f23c42c024fe1ebf09d12f3262df993bf', members:[{name:'bundle.js',hash:'f7f1782148f2afc06b9555131027e55cff84ca4d0aeb017f659f45b94605aaee',id:62457,next:18888,start:127744,end:132680,factoryHash:'73c61ba73853df286e88e23631775f7fab68f82fdff9258b97e253c5837db3af',reconcile:false}]},
  {name:'core',source:path.join(RESOURCES,'standalone_modules/discord_desktop_core/core.asar'),output:'resources/standalone_modules/discord_desktop_core/core.asar',hash:'48d3ebb69c1f6625ba121ab7c823b5d1fa7629cbb98d372554f1a1d1a5d0bd71',members:[
    {name:'bundle.js',hash:'513bcb44287de3294876ec25792fedfe323c2325cf908d54d197a2340673cb5c',id:2457,next:977,start:221205,end:226133,factoryHash:'62a5f25230e92d57fd93111b29593a7cd49ae215ae87c6e5ea9bc3fd8fb220af',reconcile:true},
    {name:'mainScreenPreload.js',hash:'87bfb4cad4edc4ab28ce70971f1dd01355b36c8d881866bf8da99e66987ea181',id:2457,next:977,start:142994,end:147922,factoryHash:'62a5f25230e92d57fd93111b29593a7cd49ae215ae87c6e5ea9bc3fd8fb220af',reconcile:true}
  ]},
  {name:'overlay-host',kind:'javascript',source:path.join(RESOURCES,'standalone_modules/discord_overlay2/overlayHost.js'),output:'resources/standalone_modules/discord_overlay2/overlayHost.js',hash:'ad8ec59ecd01331a32830ad0979554e1c8e527452df453e91bf00a1bb01bccf9',member:{name:'overlayHost.js',hash:'ad8ec59ecd01331a32830ad0979554e1c8e527452df453e91bf00a1bb01bccf9',id:924,next:8940,start:80169,end:85096,factoryHash:'f75d466227c707b7d7bca2be9a0c351501ab8d77b400f65ad107f460777d4a3a',reconcile:true}},
  {name:'overlay-preload',kind:'javascript',source:path.join(RESOURCES,'standalone_modules/discord_overlay2/overlayPreload.js'),output:'resources/standalone_modules/discord_overlay2/overlayPreload.js',hash:'27e0d12ee560206775d3180dadd6d65a68b8c104d740e1f36177b29b861502ac',member:{name:'overlayPreload.js',hash:'27e0d12ee560206775d3180dadd6d65a68b8c104d740e1f36177b29b861502ac',id:924,next:8940,start:148160,end:153087,factoryHash:'f75d466227c707b7d7bca2be9a0c351501ab8d77b400f65ad107f460777d4a3a',reconcile:true}},
  {name:'native-utils',source:path.join(RESOURCES,'standalone_modules/discord_utils/index.js'),output:'resources/standalone_modules/discord_utils/index.js',hash:'de838460a97ff880647f577666ac04c8a95ae0a1bf4385e8977aad2523e9e4cb',start:3305,end:4408,bodyHash:'f1eaf6ec84650ed46ced3e89d3e53a833f142566ee65c8692cf6e0188b0aeadf'}
];
const RECONCILE_BEFORE = 'exports.reconcileCrashReporterMetadata=function(crashReporter,metadata){let newMetadata=flatten(metadata),oldMetadata=crashReporter.getParameters();for(let key in oldMetadata)newMetadata.hasOwnProperty(key)||crashReporter.removeExtraParameter(key);for(let key in newMetadata)oldMetadata.hasOwnProperty(key)||crashReporter.addExtraParameter(key,String(newMetadata[key]))}';
const RECONCILE_AFTER = 'exports.reconcileCrashReporterMetadata=function(_crashReporter,_metadata){}';
const LIVE_AFTER = 'nativeUtils.submitLiveCrashReport = async (channel, sentryMetadata) => {\n    // NotDiscord: preserve the Promise<null> contract without generating or uploading a dump.\n    return null;\n};\n';
function setupFactory(id) {
  return `${id}(_module,exports){"use strict";Object.defineProperty(exports,"__esModule",{value:true});let initialized=false;exports.metadata={};exports.init=function(buildInfo,_sentry){if(initialized)return void console.warn("Ignoring double initialization of crash reporter.");exports.metadata.release_channel=buildInfo.releaseChannel;let metadata=exports.metadata.sentry??{};metadata.environment=buildInfo.releaseChannel;metadata.release=buildInfo.sentryRelease??buildInfo.version;if(buildInfo.sentryDist!=null)metadata.dist=buildInfo.sentryDist;exports.metadata.sentry=metadata;initialized=true};exports.isInitialized=function(){return initialized};exports.getGlobalSentry=function(){return null}}`;
}
function patchSource(source, spec) {
  assert.equal(sha256(source),spec.hash,`Unexpected member ${spec.name}`);
  const before=source.slice(spec.start,spec.end), after=setupFactory(spec.id);
  assert.equal(sha256(before),spec.factoryHash,`Factory ${spec.id} hash changed`);
  assert(before.startsWith(`${spec.id}(__unused_rspack_module,exports,__webpack_require__){`));
  assert(source.slice(spec.end).startsWith(`,${spec.next}(`));
  const edits=[{name:'disable-sentry-setup',start:spec.start,end:spec.end,before,after}];
  if(spec.reconcile) {
    const start=source.indexOf(RECONCILE_BEFORE);assert(start>=0,'Metadata helper missing');
    assert.equal(source.indexOf(RECONCILE_BEFORE,start+1),-1,'Metadata helper ambiguous');
    edits.push({name:'skip-inactive-crashpad-metadata',start,end:start+RECONCILE_BEFORE.length,before:RECONCILE_BEFORE,after:RECONCILE_AFTER});
  }
  edits.sort((a,b)=>a.start-b.start);
  for(let i=1;i<edits.length;i++)assert(edits[i-1].end<=edits[i].start);
  let result=source;
  for(const edit of [...edits].reverse())result=result.slice(0,edit.start)+edit.after+result.slice(edit.end);
  new vm.Script(result,{filename:`NotDiscordTelemetry-${spec.name}.syntax-only.js`});
  // Prefixes, gaps and suffixes are copied verbatim; no event, console or error handler is edited.
  let originalAt=0,changedAt=0;
  for(const edit of edits){const length=edit.start-originalAt;assert.equal(result.slice(changedAt,changedAt+length),source.slice(originalAt,edit.start));changedAt+=length+edit.after.length;originalAt=edit.end;}
  assert.equal(result.slice(changedAt),source.slice(originalAt));
  return {source:result,edits};
}
function prepareArtifact(spec,input) {
  assert.equal(sha256(input),spec.hash,`Unexpected source ${spec.name}`);
  if(spec.kind==='javascript') {
    const patched=patchSource(input.toString('utf8'),spec.member);
    return {spec,input,output:Buffer.from(patched.source),edits:patched.edits.map(edit=>({member:spec.member.name,...edit}))};
  }
  if(spec.name==='native-utils') {
    const source=input.toString('utf8'),before=source.slice(spec.start,spec.end);
    assert.equal(sha256(before),spec.bodyHash,'Native live reporter changed');
    assert(before.startsWith('nativeUtils.submitLiveCrashReport = async '));
    assert(source.slice(spec.end).startsWith('nativeUtils.shouldDisplayNotifications'));
    const output=Buffer.from(source.slice(0,spec.start)+LIVE_AFTER+source.slice(spec.end));
    new vm.Script(output.toString(),{filename:'NotDiscordTelemetry-native-utils.syntax-only.js'});
    return {spec,input,output,edits:[{member:'index.js',name:'disable-live-minidump-upload',start:spec.start,end:spec.end,before,after:LIVE_AFTER}]};
  }
  const archive=unpack(input),updates=new Map(),edits=[];
  for(const member of spec.members) {
    const file=archive.files.get(member.name);assert(file,`Missing ${member.name}`);
    const patched=patchSource(file.bytes.toString('utf8'),member);
    updates.set(member.name,Buffer.from(patched.source));
    for(const edit of patched.edits)edits.push({member:member.name,...edit});
  }
  const originalFiles=new Map([...archive.files].map(([name,f])=>[name,Buffer.from(f.bytes)]));
  for(const[name,bytes]of updates)archive.files.get(name).bytes=bytes;
  const output=repack(archive,archive.files.get('bundle.js').bytes.toString('utf8'));
  const roundtrip=unpack(output);assert.deepEqual([...roundtrip.files.keys()],[...originalFiles.keys()]);
  for(const[name,bytes]of originalFiles){const actual=roundtrip.files.get(name).bytes;assert(actual.equals(updates.get(name)||bytes),`Unexpected archive member change ${name}`);}
  return {spec,input,output,edits};
}
async function selfTest(prepared) {
  const checks=[];
  async function check(name,test){await test();checks.push({name,passed:true});}
  for(const item of prepared){
    await check(`${item.spec.name}: source SHA pinned`,()=>assert.equal(sha256(item.input),item.spec.hash));
    await check(`${item.spec.name}: changed source rejected`,()=>{const bad=Buffer.from(item.input);bad[bad.length-1]^=1;assert.throws(()=>prepareArtifact(item.spec,bad),/Unexpected source/);});
    await check(`${item.spec.name}: output differs`,()=>assert.notEqual(sha256(item.output),item.spec.hash));
  }
  for(const id of [62457,2457,924]) {
    let imports=0;const warnings=[];
    const context=vm.createContext({console:{warn:message=>warnings.push(message)}});
    const factory=new vm.Script(`({${setupFactory(id)}})[${id}]`).runInContext(context,{timeout:1000});
    const exp={};factory({exports:exp},exp,()=>{imports++;throw Error('Import forbidden');});
    await check(`${id}: exports retain ES module contract`,()=>{assert.equal(exp.__esModule,true);assert.deepEqual(Object.keys(exp).sort(),['getGlobalSentry','init','isInitialized','metadata']);});
    const metadata=exp.metadata;
    await check(`${id}: initial state has no Sentry client`,()=>{assert.equal(exp.isInitialized(),false);assert.equal(exp.getGlobalSentry(),null);assert.equal(typeof metadata,'object');});
    const inaccessibleSentry=new Proxy({},{get(){throw Error('Sentry must not be touched');}});
    metadata.sentry={custom:'retained'};
    await check(`${id}: init returns void without Sentry access`,()=>assert.equal(exp.init({releaseChannel:'stable',version:'1.0.9262',sentryRelease:'test-release',sentryDist:'test-dist'},inaccessibleSentry),undefined));
    await check(`${id}: initialized opt-out metadata compatible`,()=>{assert.equal(exp.isInitialized(),true);assert.equal(exp.getGlobalSentry(),null);assert.equal(exp.metadata,metadata);assert.equal(metadata.release_channel,'stable');assert.equal(metadata.sentry.environment,'stable');assert.equal(metadata.sentry.release,'test-release');assert.equal(metadata.sentry.dist,'test-dist');assert.equal(metadata.sentry.custom,'retained');});
    await check(`${id}: duplicate initialization is inert/local warning`,()=>{assert.equal(exp.init(null,inaccessibleSentry),undefined);assert.equal(warnings.length,1);assert.equal(metadata.release_channel,'stable');});
    await check(`${id}: no native or network import`,()=>assert.equal(imports,0));
    const second={};factory({exports:second},second,()=>{throw Error('Import forbidden');});
    await check(`${id}: no Sentry argument/fallback release supported`,()=>{second.init({releaseChannel:'stable',version:'1.0.9262'});assert.equal(second.metadata.sentry.release,'1.0.9262');assert(!('dist'in second.metadata.sentry));});
  }
  await check('metadata reconciliation does not call an unstarted reporter',()=>{const context=vm.createContext({exports:{}});new vm.Script(RECONCILE_AFTER).runInContext(context);const forbidden=new Proxy({},{get(){throw Error('Crashpad touched');}});assert.equal(context.exports.reconcileCrashReporterMetadata(forbidden,forbidden),undefined);});
  await check('live dump API preserves Promise<null> without argument access',async()=>{const context=vm.createContext({nativeUtils:{}});new vm.Script(LIVE_AFTER).runInContext(context);const forbidden=new Proxy({},{get(){throw Error('Unexpected argument access');}});const result=context.nativeUtils.submitLiveCrashReport(forbidden,forbidden);assert.equal(typeof result.then,'function');assert.equal(await result,null);});
  await check('no diagnostic environment workaround introduced',()=>{for(const item of prepared)for(const edit of item.edits)assert(!edit.after.includes('ELECTRON_ENABLE_STACK_DUMPING'));});
  await check('ten focused edits only',()=>assert.equal(prepared.reduce((sum,item)=>sum+item.edits.length,0),10));
  return checks;
}
function inside(base,file){const relative=path.relative(base,file);return relative!==''&&relative!=='..'&&!relative.startsWith('..'+path.sep)&&!path.isAbsolute(relative);}
function assertNoLink(file){for(let current=path.resolve(file);;current=path.dirname(current)){if(fs.existsSync(current))assert(!fs.lstatSync(current).isSymbolicLink(),'Reparse path forbidden');if(current===path.dirname(current))break;}}
function safeWrite(base,file,bytes){assert(inside(base,file),'Output outside designated staging/evidence root');assertNoLink(file);fs.mkdirSync(path.dirname(file),{recursive:true});assertNoLink(file);fs.writeFileSync(file,bytes);}
async function main(argv){
  assert(argv.length===0||argv.length===1&&argv[0]==='--check','Only optional --check is accepted; this tool cannot apply patches');
  const prepared=ARTIFACTS.map(spec=>prepareArtifact(spec,fs.readFileSync(spec.source)));
  const checks=await selfTest(prepared),stage=argv.length===0;
  for(const item of prepared)if(stage)safeWrite(STAGED,path.join(STAGED,item.spec.output),item.output);
  // Source recheck catches concurrent changes and documents that preparation never wrote to them.
  for(const item of prepared)assert.equal(sha256(fs.readFileSync(item.spec.source)),item.spec.hash,'Active runtime changed during preparation');
  const manifest={policyVersion:1,preparedUtc:new Date().toISOString(),applied:false,sourceCodeExecuted:false,staged:stage,stagingRoot:STAGED,checks,passed:checks.every(c=>c.passed),artifacts:prepared.map(item=>({name:item.spec.name,source:item.spec.source,sourceSha256:item.spec.hash,output:path.join(STAGED,item.spec.output),outputSha256:sha256(item.output),outputBytes:item.output.length,edits:item.edits.map(edit=>({member:edit.member,name:edit.name,utf16Start:edit.start,utf16End:edit.end,beforeSha256:sha256(edit.before),afterSha256:sha256(edit.after)}))})),scope:['Disable Sentry initialization in the pinned bootstrap, core, main preload and overlay setup factories','Keep metadata and initialization/getter API contracts, with no global Sentry client','Skip Crashpad metadata API calls because the reporter is not initialized by this policy','Keep the live-minidump helper Promise<null> contract without dump generation or upload','Preserve all bytes outside the ten selected edits and every other ASAR member'],prerequisites:['Stop the native candidate before any future installation','Review this proposal and update the entry bootstrap SHA assertion before installing its output','Repack entry app.asar with the reviewed new bootstrap hash; this preparer intentionally does not modify entry','Apply only to native-candidate copies, never to installed Discord'],limits:['Sentry library imports remain bundled; memory savings require runtime measurement','Existing local console/error handlers outside the modified factories are unchanged','Native .node/DLL telemetry, other reporting code paths and network silence are not proven','Discord web tracking remains a separate NoTrack/application policy','No authenticated profile, token, cookie, message or runtime log was read','This is an unexecuted proposal, not a claim of startup or feature parity']};
  const evidenceRoot=path.join(ROOT,'evidence','native');safeWrite(evidenceRoot,path.join(evidenceRoot,'telemetry-policy.json'),JSON.stringify(manifest,null,2)+'\n');
  console.log(JSON.stringify({prepared:stage,applied:false,checks:checks.length,passed:manifest.passed,outputs:manifest.artifacts.map(a=>({name:a.name,sha256:a.outputSha256})),evidence:path.join(evidenceRoot,'telemetry-policy.json')},null,2));
}
module.exports={ARTIFACTS,setupFactory,patchSource,prepareArtifact,selfTest,RECONCILE_BEFORE,RECONCILE_AFTER,LIVE_AFTER};
if(require.main===module)main(process.argv.slice(2)).catch(error=>{console.error(error.stack||String(error));process.exitCode=1;});
