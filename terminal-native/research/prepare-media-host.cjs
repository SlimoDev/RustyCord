'use strict';
const fs=require('node:fs'),path=require('node:path'),crypto=require('node:crypto');
const root=path.resolve(__dirname,'..'),source=path.resolve(root,'../native-candidate/runtime'),destination=path.join(root,'media-host/runtime');
fs.mkdirSync(path.join(destination,'resources'),{recursive:true});
const executable=path.join(destination,'NotDiscord.Media.exe');
// The executable is a separate file; immutable runtime dependencies share disk blocks.
if(!fs.existsSync(executable))fs.copyFileSync(path.join(source,'NotDiscord.exe'),executable,fs.constants.COPYFILE_EXCL);
const files=['chrome_100_percent.pak','chrome_200_percent.pak','d3dcompiler_47.dll','discord_wer.dll','dxcompiler.dll','dxil.dll','ffmpeg.dll','icudtl.dat','libEGL.dll','libGLESv2.dll','resources.pak','snapshot_blob.bin','v8_context_snapshot.bin','vk_swiftshader_icd.json','vk_swiftshader.dll','vulkan-1.dll'];
for(const file of files){const target=path.join(destination,file);if(!fs.existsSync(target)&&fs.existsSync(path.join(source,file)))fs.linkSync(path.join(source,file),target);}
fs.mkdirSync(path.join(destination,'locales'),{recursive:true});
for(const file of ['en-US.pak','es.pak','es-419.pak']){const target=path.join(destination,'locales',file);if(!fs.existsSync(target)&&fs.existsSync(path.join(source,'locales',file)))fs.linkSync(path.join(source,'locales',file),target);}
const boot=String.raw`'use strict';
const path=require('node:path'),fs=require('node:fs'),net=require('node:net');
const {app}=require('electron');
const root=path.resolve(process.resourcesPath,'../../..');
const ownData=path.join(root,'data/media-host');fs.mkdirSync(ownData,{recursive:true});
app.setName('NotDiscord Media');app.setAppUserModelId('com.notdiscord.Terminal.Native');
app.setPath('userData',ownData);app.setPath('sessionData',ownData);
app.disableHardwareAcceleration();
app.commandLine.appendSwitch('disable-background-networking');
app.on('window-all-closed',()=>{});
app.whenReady().then(()=>{
  if(process.argv.includes('--media-probe-mls')){
    process.argv.push('--child','--mls','--mls-callback');
    require(path.join(root,'research/probe-voice-connection-loopback.cjs'));
  }else{
    const pipe=process.env.NOTDISCORD_MEDIA_PIPE;
    if(typeof pipe!=='string'||!/^\\\\\.\\pipe\\NotDiscord\.Media\.(?:[a-f0-9]{32}|[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})$/i.test(pipe)){app.exit(2);return;}
    delete process.env.NOTDISCORD_MEDIA_PIPE;
    const socket=net.createConnection(pipe),timeout=setTimeout(()=>app.exit(3),10000);
    let connected=false;
    socket.once('error',()=>{clearTimeout(timeout);if(!connected)app.exit(4);});
    socket.once('connect',()=>{
      connected=true;
      clearTimeout(timeout);
      global.__notDiscordMediaTransport={input:socket,output:socket};
      require(path.join(root,'media-helper/main.cjs'));
    });
  }
});
`;
const payloads={'package.json':Buffer.from(JSON.stringify({name:'notdiscord-media',productName:'NotDiscord Media',version:'0.1.0',main:'boot.cjs'})),'boot.cjs':Buffer.from(boot)};
const header={files:{}};let offset=0;for(const [file,bytes]of Object.entries(payloads)){header.files[file]={size:bytes.length,offset:String(offset)};offset+=bytes.length;}
const json=Buffer.from(JSON.stringify(header)),padding=Math.ceil(json.length/4)*4,prefix=Buffer.alloc(16+padding);prefix.writeUInt32LE(4,0);prefix.writeUInt32LE(8+padding,4);prefix.writeUInt32LE(4+padding,8);prefix.writeUInt32LE(json.length,12);json.copy(prefix,16);
fs.writeFileSync(path.join(destination,'resources/app.asar'),Buffer.concat([prefix,...Object.values(payloads)]));
const bytes=fs.readFileSync(executable),sentinel=Buffer.from('dL7pKGdnNz796PbbjQWNKmHXBZaB9tsX'),at=bytes.indexOf(sentinel);
const report={createdAtUtc:new Date().toISOString(),executable,sha256:crypto.createHash('sha256').update(bytes).digest('hex'),separateExecutableCopy:true,hardlinkedReadOnlyDependencies:files.length,originalAppLoaded:false,browserWindowsCreated:0,fuses:at<0?null:{version:bytes[at+sentinel.length],count:bytes[at+sentinel.length+1],values:bytes.subarray(at+sentinel.length+2,at+sentinel.length+2+bytes[at+sentinel.length+1]).toString('ascii')}};
fs.writeFileSync(path.join(root,'research/media-host-build.json'),JSON.stringify(report,null,2));process.stdout.write(JSON.stringify(report,null,2)+'\n');
