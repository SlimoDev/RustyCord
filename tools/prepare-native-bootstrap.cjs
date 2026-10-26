'use strict';
// NotDiscord native bootstrap policy, revision 1. No original bundle code is executed.
// Input is pinned to Discord 1.0.9262; only the copied ASAR under native-candidate is writable.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const ROOT = path.resolve(__dirname, '..');
const CANDIDATE = path.join(ROOT, 'native-candidate');
const DEFAULT_SOURCE = 'C:/Users/sssli/AppData/Local/Discord/app-1.0.9262/resources/_app.asar';
const DEFAULT_OUTPUT = path.join(CANDIDATE, 'runtime', 'resources', '_app.asar');
const SOURCE_SHA256 = 'cbd26748b01c1b4847dc5471c4d2e1e46d69b0eb6f6c3347aa66ca992267e9b8';
const APP_ID = 'com.notdiscord.NativeCandidate';
const POLICY_VERSION = 1;
const sha256 = data => crypto.createHash('sha256').update(data).digest('hex');
const modulePolicies = [
  {id:74332,start:11458,end:15154,next:23151,hash:'30473b5c05accf7ae0fa7224acd67d26dbcda0f8615dc7360f36b5078bc59294',name:'windows-autostart',source:`74332(_module,exports){"use strict";Object.defineProperty(exports,"__esModule",{value:true});function complete(callback){if(typeof callback==="function")queueMicrotask(()=>callback())}exports.install=complete;exports.update=complete;exports.uninstall=complete;exports.isInstalled=function(callback){if(typeof callback==="function")queueMicrotask(()=>callback(false))}}`},
  {id:62338,start:43896,end:47614,next:2278,hash:'2d9f211f9db676f8ab539ad358bdfe614bc599025582d3e15982deb6f73e2d3b',name:'windows-first-run',source:`62338(_module,exports){"use strict";Object.defineProperty(exports,"__esModule",{value:true});exports.performFirstRunTasks=async function(_updater){}}`},
  {id:83669,start:84392,end:86526,next:53006,hash:'ea7b906947de01bb8dcbb6b572b50415f8848b6059bbeba5d834a18d1c80fa8d',name:'root-launcher',source:`83669(_module,exports){"use strict";Object.defineProperty(exports,"__esModule",{value:true});exports.launcherExe=process.execPath;exports.pathExists=async function(_target){return false};exports.ensureLauncherInstalled=async function(){}}`},
  {id:27251,start:101464,end:105647,next:19725,hash:'83a399bf6a97257636bafc855c4ab6ef05ee3e9e5871f2c6db72fe2ec6dff58d',name:'squirrel-installer',source:`27251(_module,exports){"use strict";Object.defineProperty(exports,"__esModule",{value:true});exports.spawnUpdate=function(_args,callback){if(typeof callback==="function")queueMicrotask(()=>{const error=new Error("Native candidate updates are disabled");error.code="ERR_NOTDISCORD_UPDATER_DISABLED";callback(error,"")})};exports.installProtocol=function(_protocol,callback){if(typeof callback==="function")queueMicrotask(()=>callback())};exports.handleStartupEvent=function(_protocol,_app,_command){return false};exports.updateExistsSync=function(){return false};exports.restart=function(_app,_newVersion){}}`}
];
const literalPolicies = [
  {name:'own-app-user-model-id',before:'hasArgvFlag("--localdev")?app.setAppUserModelId(process.execPath):app.setAppUserModelId(Constants.APP_ID)',after:`app.setAppUserModelId(${JSON.stringify(APP_ID)})`},
  {name:'new-updater-disabled',before:'disableUpdater:buildInfo.disableUpdater||"development"===buildInfo.releaseChannel&&(buildInfo.standaloneModules||null!=buildInfo.localModulesRoot)',after:'disableUpdater:true'},
  {name:'legacy-module-installs-disabled',before:'settings=_settings,updatable="0.0.0"!==_buildInfo.version&&!_buildInfo.debug||settings?.get(ALWAYS_ALLOW_UPDATES);',after:'settings=_settings,updatable=false;'},
  {name:'legacy-host-check-disabled',before:'skipHostUpdate=settings?.get("SKIP_HOST_UPDATE")||!hostUpdatable',after:'skipHostUpdate=true'},
  {name:'legacy-module-check-disabled',before:'skipModuleUpdate=settings?.get("SKIP_MODULE_UPDATE")||locallyInstalledModules||!updatable',after:'skipModuleUpdate=true'},
  {name:'legacy-install-relaunch-disabled',before:'exports.quitAndInstallUpdates=function(){logger.log(`Relaunching to install ${hostUpdateAvailable?"host":"module"} updates...`),hostUpdateAvailable?(analytics.getDesktopTTI().trackSplashWindowRestart(),hostUpdater.quitAndInstall()):function(){logger.end();let{app}=__webpack_require__(91288);app.relaunch(),app.quit()}()}',after:'exports.quitAndInstallUpdates=function(){logger?.log("NotDiscord native candidate: updates disabled")}'},
  {name:'win32-native-updater-fallback-disabled',before:'quitAndInstall(){squirrelUpdate.updateExistsSync()?squirrelUpdate.restart(electron_1.app,this.updateVersion??electron_1.app.getVersion()):__webpack_require__(25745).quitAndInstall()}',after:'quitAndInstall(){}'}
];
function unpack(buffer) {
  assert(buffer.length >= 16, 'ASAR truncated');
  assert.equal(buffer.readUInt32LE(0), 4, 'Unexpected ASAR size pickle');
  const headerSize = buffer.readUInt32LE(4), jsonSize = buffer.readUInt32LE(12);
  assert(headerSize >= 8 && 16 + jsonSize <= 8 + headerSize && 8 + headerSize <= buffer.length, 'Invalid ASAR header lengths');
  const header = JSON.parse(buffer.subarray(16, 16 + jsonSize).toString('utf8'));
  const dataStart = 8 + headerSize, files = new Map();
  function walk(node, prefix='') {
    for (const [name, entry] of Object.entries(node.files || {})) {
      assert(!name.includes('/') && !name.includes('\\') && name !== '..' && name !== '.', 'Unsafe ASAR member');
      const member = prefix ? `${prefix}/${name}` : name;
      if (entry.files) walk(entry, member);
      else {
        assert(!entry.link && !entry.unpacked, 'This policy expects packed regular members only');
        const offset = Number(entry.offset), size = Number(entry.size);
        assert(Number.isSafeInteger(offset) && offset >= 0 && Number.isSafeInteger(size) && size >= 0 && dataStart + offset + size <= buffer.length, 'Invalid ASAR member range');
        files.set(member, {entry, bytes:buffer.subarray(dataStart+offset,dataStart+offset+size)});
      }
    }
  }
  walk(header);
  return {header, files};
}
function repack(archive, changedBundle) {
  const header = structuredClone(archive.header), chunks = [];
  let offset = 0;
  function walk(node, prefix='') {
    for(const [name, entry] of Object.entries(node.files || {})) {
      const member = prefix ? `${prefix}/${name}` : name;
      if(entry.files) walk(entry, member);
      else {
        const bytes = member === 'bundle.js' ? Buffer.from(changedBundle, 'utf8') : archive.files.get(member).bytes;
        entry.offset = String(offset); entry.size = bytes.length;
        if(entry.integrity) {
          assert.equal(entry.integrity.algorithm, 'SHA256');
          const blockSize = entry.integrity.blockSize;
          assert(Number.isSafeInteger(blockSize) && blockSize > 0);
          entry.integrity.hash = sha256(bytes); entry.integrity.blocks = [];
          for(let i=0;i<bytes.length;i+=blockSize) entry.integrity.blocks.push(sha256(bytes.subarray(i,i+blockSize)));
        }
        chunks.push(bytes); offset += bytes.length;
      }
    }
  }
  walk(header);
  const json = Buffer.from(JSON.stringify(header), 'utf8');
  const padded = Math.ceil(json.length / 4) * 4;
  const prefix = Buffer.alloc(16 + padded);
  prefix.writeUInt32LE(4,0); prefix.writeUInt32LE(8+padded,4);
  prefix.writeUInt32LE(4+padded,8); prefix.writeUInt32LE(json.length,12); json.copy(prefix,16);
  return Buffer.concat([prefix,...chunks]);
}
function patchBundle(original) {
  const edits = [];
  for(const policy of modulePolicies) {
    const before = original.slice(policy.start,policy.end);
    assert.equal(sha256(before),policy.hash,`Module ${policy.id} changed`);
    assert(original.slice(policy.end).startsWith(`,${policy.next}(`),`Module ${policy.id} boundary changed`);
    assert(before.startsWith(`${policy.id}(`));
    edits.push({name:policy.name,start:policy.start,end:policy.end,before,after:policy.source});
  }
  for(const policy of literalPolicies) {
    const start = original.indexOf(policy.before);
    assert(start >= 0,`Missing policy anchor ${policy.name}`);
    assert.equal(original.indexOf(policy.before,start+1),-1,`Ambiguous policy anchor ${policy.name}`);
    edits.push({name:policy.name,start,end:start+policy.before.length,before:policy.before,after:policy.after});
  }
  edits.sort((a,b)=>a.start-b.start);
  for(let i=1;i<edits.length;i++) assert(edits[i-1].end<=edits[i].start,'Overlapping patches');
  let result=original;
  for(const edit of [...edits].reverse()) result=result.slice(0,edit.start)+edit.after+result.slice(edit.end);
  new vm.Script(result,{filename:'NotDiscord.patched-bootstrap.syntax-only.js'}); // Compile only; never run original code.
  return {code:result,edits:edits.map(e=>({name:e.name,utf16Start:e.start,utf16End:e.end,beforeSha256:sha256(e.before),afterSha256:sha256(e.after)}))};
}
function prepare(input) {
  assert.equal(sha256(input),SOURCE_SHA256,'Unexpected original ASAR SHA256; policy will not guess at another build');
  const archive=unpack(input), original=archive.files.get('bundle.js')?.bytes.toString('utf8');
  assert(original,'bundle.js missing');
  const patched=patchBundle(original), output=repack(archive,patched.code), roundtrip=unpack(output);
  assert.deepEqual([...roundtrip.files.keys()],[...archive.files.keys()]);
  for(const [name,file] of archive.files) {
    const newFile=roundtrip.files.get(name);
    if(name==='bundle.js') assert.equal(newFile.bytes.toString('utf8'),patched.code);
    else assert(file.bytes.equals(newFile.bytes),`Unrelated member changed: ${name}`);
    if(newFile.entry.integrity) assert.equal(newFile.entry.integrity.hash,sha256(newFile.bytes));
  }
  return {output,archive,patched};
}
function inside(base,target) { const relative=path.relative(base,target); return relative!=='' && !relative.startsWith('..'+path.sep) && relative!=='..' && !path.isAbsolute(relative); }
function assertNoReparse(target) {
  for(let current=path.resolve(target);;current=path.dirname(current)) {
    if(fs.existsSync(current)) assert(!fs.lstatSync(current).isSymbolicLink(),`Reparse/symlink path forbidden: ${current}`);
    const parent=path.dirname(current); if(current===parent) break;
  }
}
function validateOutput(output) {
  const resolved=path.resolve(output);
  assert(inside(CANDIDATE,resolved),'Output must remain inside native-candidate');
  assert.equal(path.basename(resolved),'_app.asar','Output must be the copied _app.asar');
  assertNoReparse(resolved); return resolved;
}
async function selfTest(input,prepared) {
  const checks=[];
  async function check(name,fn) {await fn();checks.push({name,passed:true});}
  await check('original SHA256 pinned',()=>assert.equal(sha256(input),SOURCE_SHA256));
  await check('changed original is rejected',()=>{const bad=Buffer.from(input);bad[bad.length-1]^=1;assert.throws(()=>prepare(bad),/SHA256/)});
  await check('bundle syntactically valid without execution',()=>new vm.Script(prepared.patched.code));
  await check('eleven versioned patches applied',()=>assert.equal(prepared.patched.edits.length,11));
  await check('output remains in candidate',()=>assert.equal(validateOutput(DEFAULT_OUTPUT),DEFAULT_OUTPUT));
  await check('original installation cannot be output',()=>assert.throws(()=>validateOutput(DEFAULT_SOURCE),/native-candidate/));
  await check('traversal cannot escape candidate',()=>assert.throws(()=>validateOutput(path.join(CANDIDATE,'..','_app.asar')),/native-candidate/));
  const stubs=new Map(); let forbiddenImports=0;
  for(const policy of modulePolicies) {
    const context=vm.createContext({process:Object.freeze({execPath:DEFAULT_OUTPUT.replace(/resources[\\/]_app\.asar$/,'Discord.exe')}),queueMicrotask});
    const factory=new vm.Script(`({${policy.source}})[${policy.id}]`).runInContext(context,{timeout:1000});
    const exp={}; factory({exports:exp},exp,()=>{forbiddenImports++;throw Error('Original import forbidden in stub')});stubs.set(policy.id,exp);
    await check(`${policy.name} ES module marker`,()=>assert.equal(exp.__esModule,true));
  }
  const auto=stubs.get(74332), squirrel=stubs.get(27251), launcher=stubs.get(83669), first=stubs.get(62338);
  for(const name of ['install','update','uninstall']) await check(`autostart ${name} callback once/no args`,async()=>{let calls=0,args;auto[name]((...a)=>{calls++;args=a});assert.equal(calls,0);await Promise.resolve();assert.equal(calls,1);assert.equal(args.length,0)});
  await check('autostart isInstalled boolean false',async()=>{let args;auto.isInstalled((...a)=>args=a);await Promise.resolve();assert.deepEqual(args,[false])});
  await check('first run Promise<void>',async()=>assert.equal(await first.performFirstRunTasks(new Proxy({}, {get(){throw Error('Updater touched')}})),undefined));
  await check('root launcher no-op/false',async()=>{assert.equal(await launcher.pathExists('C:/forbidden'),false);assert.equal(await launcher.ensureLauncherInstalled(),undefined);assert(inside(CANDIDATE,path.resolve(launcher.launcherExe)))});
  await check('squirrel flags never perform startup actions',()=>{const app=new Proxy({}, {get(){throw Error('App action touched')}});for(const flag of [undefined,'--squirrel-install','--squirrel-updated','--squirrel-uninstall','--squirrel-obsolete','--squirrel-firstrun'])assert.equal(squirrel.handleStartupEvent('discord',app,flag),false);assert.equal(squirrel.updateExistsSync(),false);assert.equal(squirrel.restart(app,'9.9.9'),undefined)});
  await check('protocol callback completes without mutation',async()=>{let calls=0;squirrel.installProtocol('discord',()=>calls++);await Promise.resolve();assert.equal(calls,1)});
  await check('spawnUpdate callback explicitly disabled',async()=>{let args;squirrel.spawnUpdate(['--anything'],(...a)=>args=a);await Promise.resolve();assert.equal(args[0].code,'ERR_NOTDISCORD_UPDATER_DISABLED');assert.equal(args[1],'')});
  await check('no stub imported original dependencies',()=>assert.equal(forbiddenImports,0));
  await check('all non-bundle ASAR payloads unchanged',()=>{const result=unpack(prepared.output);for(const [name,file] of prepared.archive.files)if(name!=='bundle.js')assert(file.bytes.equals(result.files.get(name).bytes))});
  await check('all exact update/identity replacements present',()=>{for(const p of literalPolicies){assert(!prepared.patched.code.includes(p.before));assert(prepared.patched.code.includes(p.after))}});
  return checks;
}
function writeAtomic(file,bytes) {
  assertNoReparse(file);fs.mkdirSync(path.dirname(file),{recursive:true});assertNoReparse(file);
  const temp=file+'.preparing-'+process.pid;
  fs.writeFileSync(temp,bytes,{flag:'wx'});
  try {fs.renameSync(temp,file);} catch(error) {fs.unlinkSync(temp);throw error;}
}
async function main(argv) {
  let source=DEFAULT_SOURCE,output=DEFAULT_OUTPUT,testOnly=false;
  for(let i=0;i<argv.length;i++) {
    if(argv[i]==='--source'){assert(argv[++i],'--source needs value');source=argv[i];}
    else if(argv[i]==='--output'){assert(argv[++i],'--output needs value');output=argv[i];}
    else if(argv[i]==='--self-test')testOnly=true;
    else throw Error(`Unknown argument ${argv[i]}`);
  }
  output=validateOutput(output);
  const input=fs.readFileSync(source),prepared=prepare(input),checks=await selfTest(input,prepared);
  const manifest={policyVersion:POLICY_VERSION,createdUtc:new Date().toISOString(),source:path.resolve(source),sourceSha256:SOURCE_SHA256,output,outputSha256:sha256(prepared.output),sourceBytes:input.length,outputBytes:prepared.output.length,appUserModelId:APP_ID,patches:prepared.patched.edits,checks,passed:checks.every(c=>c.passed),archiveWritten:!testOnly,originalCodeExecuted:false,prerequisites:['entry rejects --squirrel-* arguments','build_info.json uses standaloneModules:true or localModulesRoot under candidate','all original native modules must be copied to the selected local root','entry sets private DISCORD_USER_DATA_DIR/VENCORD_USER_DATA_DIR/TEMP/TMP before requiring Vencord','entry disables Vencord updater auto-patching; original profile is never copied'],limits:['Native .node code is preserved and not sandboxed by these JavaScript patches','These tests validate patches and callbacks, not Discord login/voice/screenshare or memory savings']};
  if(!testOnly) {
    if(fs.existsSync(output)) {const existing=sha256(fs.readFileSync(output));assert(existing===SOURCE_SHA256||existing===manifest.outputSha256,'Refusing to overwrite an unrelated copied ASAR');}
    writeAtomic(output,prepared.output);
    assert.equal(sha256(fs.readFileSync(output)),manifest.outputSha256);
  }
  const evidence=path.join(ROOT,'evidence','native','bootstrap-policy'+(testOnly?'-self-test':'')+'.json');
  writeAtomic(evidence,JSON.stringify(manifest,null,2)+'\n');
  console.log(JSON.stringify({passed:manifest.passed,checks:checks.length,archiveWritten:!testOnly,output,outputSha256:manifest.outputSha256,evidence},null,2));
}
module.exports={prepare,patchBundle,unpack,repack,validateOutput,selfTest,SOURCE_SHA256,APP_ID,modulePolicies,literalPolicies};
if(require.main===module)main(process.argv.slice(2)).catch(error=>{console.error(error.stack||String(error));process.exitCode=1});
