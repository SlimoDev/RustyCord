'use strict';
const {MediaError,MAX_FRAME,requireString,snowflake,volumeScale,safeError}=require('./protocol.cjs');
const native=require('./native.cjs');
const {VoiceSession}=require('./voice-session.cjs');
const {readStats}=require('./stats.cjs');
const {normalizeAudioSettings,applyAudioSettings,applyInputMode,LocalMicTest}=require('./audio-settings.cjs');
// Never log request arguments, account/voice credentials, native errors, or frame bodies.
for(const key of ['log','info','warn','error','debug']) console[key]=()=>{};
const input=global.__notDiscordMediaTransport?.input||process.stdin;
const output=global.__notDiscordMediaTransport?.output||process.stdout;
const write=value=>{if(!output.destroyed&&output.writable)output.write(JSON.stringify(value)+'\n');};
native.setStageListener(stage=>write({event:'mediaStage',stage,kind:'native'}));
let voice=null,stream=null,queue=Promise.resolve(),queued=0,closing=false,sourceInventory=new Set(),sourcesListedAt=0,cameraInventory=new Set(),camerasListedAt=0;
let audioSettings=normalizeAudioSettings();
const micTest=new LocalMicTest(()=>native.initialize(),value=>write({event:'micTest',...value}));
function watch(session){session.on('state',value=>write({event:value.kind==='share'?'shareState':'voiceState',...value}));session.on('speaking',value=>write({event:'speaking',...value}));session.on('participant',value=>write({event:'participant',...value}));session.on('camera',value=>write({event:'cameraState',...value}));session.on('stage',value=>write({event:'mediaStage',...value}));}
function requireVoice(){if(!voice||voice.closed)throw new MediaError('NOT_IN_VOICE','Join a voice channel first.');return voice;}
async function command(name,p={}) {
  switch(name){
    case 'capabilities':return native.capabilities();
    case 'stats':return readStats(voice,stream);
    case 'listDevices':return native.listDevices();
    case 'configureAudio':{
      const settings=normalizeAudioSettings(p),inventory=await native.listDevices(),fallbacks=[];
      for(const [key,list]of [['inputId',inventory.input],['outputId',inventory.output]])if(settings[key]!=='default'&&!list.some(d=>d.id===settings[key])){settings[key]='default';fallbacks.push(key);}
      applyAudioSettings(native.initialize(),settings,{captureInput:!!(voice?.localReady&&!voice.closed||micTest.active)});if(voice?.conn&&!voice.closed)applyInputMode(voice.conn,settings);
      audioSettings=settings;if(voice&&!voice.closed)voice.audioSettings=settings;return{applied:true,fallbacks};
    }
    case 'ptt':{if(typeof p.active!=='boolean')throw new MediaError('INVALID_PTT','PTT active must be a boolean.');const current=requireVoice();if(!audioSettings.pushToTalk)throw new MediaError('PTT_DISABLED','Enable push to talk first.');return current.ptt(p.active);}
    case 'startMicTest':if(voice&&!voice.closed||stream&&!stream.closed)throw new MediaError('MIC_TEST_IN_CALL','Disconnect before starting a local microphone test.');return micTest.start(audioSettings);
    case 'stopMicTest':return micTest.stop();
    case 'listCameras':{const devices=await native.listCameras();cameraInventory=new Set(devices.map(d=>d.id));camerasListedAt=Date.now();return devices;}
    case 'startCamera':{const current=requireVoice();requireString(p.deviceId,'deviceId',1024);if(!cameraInventory.has(p.deviceId)||Date.now()-camerasListedAt>600000)throw new MediaError('CAMERA_NOT_LISTED','Select an available camera again.');return current.startCamera(p.deviceId);}
    case 'stopCamera':return requireVoice().stopCamera();
    case 'setDevices':{const m=native.initialize();audioSettings=normalizeAudioSettings({...audioSettings,...(p.inputId!==undefined?{inputId:requireString(p.inputId,'inputId',512)}:{}),...(p.outputId!==undefined?{outputId:requireString(p.outputId,'outputId',512)}:{})});applyAudioSettings(m,audioSettings,{captureInput:!!(voice?.localReady&&!voice.closed||micTest.active)});if(voice&&!voice.closed)voice.audioSettings=audioSettings;return {applied:true};}
    case 'connectVoice':{micTest.stop();stream?.close();voice?.close();stream=null;voice=new VoiceSession({...p,audioSettings});watch(voice);return voice.start();}
    case 'disconnect':stream?.close();voice?.close();stream=null;voice=null;return {state:'disconnected'};
    case 'mute':return requireVoice().mute(p.value);
    case 'deafen':return requireVoice().deafen(p.value);
    case 'setVolume':{const value=Number(p.value),scale=volumeScale(value);if(p.target==='user'){requireVoice().setUserVolume(snowflake(p.userId,'userId'),scale);}else if(p.target==='input')native.initialize().setInputVolume(scale);else if(p.target==='output')native.initialize().setOutputVolume(scale);else throw new MediaError('INVALID_VOLUME_TARGET','Volume target must be input, output or user.');return {target:p.target,value};}
    case 'listSources':{const sources=await native.listSources();sourceInventory=new Set(sources.map(s=>s.id));sourcesListedAt=Date.now();return sources;}
    case 'startShare':{
      if(!requireVoice().connected)throw new MediaError('VOICE_NOT_READY','Wait for encrypted voice transport before sharing.');
      requireString(p.sourceId,'sourceId',512);if(!sourceInventory.has(p.sourceId)||Date.now()-sourcesListedAt>600000)throw new MediaError('SOURCE_NOT_LISTED','Open the source picker again and select an available source.');stream?.close();stream=new VoiceSession(p,{stream:true});watch(stream);
      const current=stream;current.once('connected',()=>{try{const result=current.share(p.sourceId,p.quality,p.audio);write({event:'shareState',kind:'share',...result});}catch(e){current.fail(e);}});
      return current.start();
    }
    case 'stopShare':stream?.stopShare();stream?.close();stream=null;return {state:'stopped'};
    case 'shutdown':closing=true;micTest.stop();stream?.close();voice?.close();return {state:'closed'};
    default:throw new MediaError('UNKNOWN_COMMAND','Unknown media command.');
  }
}
function enqueue(line){
  if(++queued>32){write({id:null,ok:false,error:{code:'QUEUE_LIMIT',message:'Too many media requests.'}});closing=true;input.destroy();return;}
  queue=queue.then(async()=>{
    let id=null;
    try{const request=JSON.parse(line);if(typeof request.id==='string'&&request.id.length<=128)id=request.id;else if(Number.isSafeInteger(request.id))id=request.id;else throw new MediaError('INVALID_REQUEST','A short request id is required.');const result=await command(request.command,request.params||{});write({id,ok:true,result});}
    catch(error){write({id,ok:false,error:safeError(error)});}
    finally{queued--;if(closing){input.pause();setTimeout(()=>process.exit(0),25);}}
  });
}
let buffer='',inputClosed=false;input.setEncoding('utf8');
input.on('data',chunk=>{buffer+=chunk;if(Buffer.byteLength(buffer)>MAX_FRAME){write({id:null,ok:false,error:{code:'REQUEST_TOO_LARGE',message:'The request exceeded the IPC size limit.'}});stream?.close();voice?.close();process.exit(1);}let at;while((at=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,at).trim();buffer=buffer.slice(at+1);if(line)enqueue(line);}});
function closeInput(){if(inputClosed)return;inputClosed=true;if(buffer.trim())enqueue(buffer.trim());queue.finally(()=>{micTest.stop();stream?.close();voice?.close();setTimeout(()=>process.exit(0),25);});}
input.on('end',closeInput);input.on('close',closeInput);input.on('error',closeInput);
process.on('SIGTERM',()=>{micTest.stop();stream?.close();voice?.close();process.exit(0);});
process.on('uncaughtException',()=>{write({event:'fatal',error:{code:'MEDIA_HOST_ERROR',message:'The media helper stopped after an internal error.'}});stream?.close();voice?.close();process.exit(1);});
process.on('unhandledRejection',()=>{write({event:'fatal',error:{code:'MEDIA_HOST_ERROR',message:'The media helper stopped after an asynchronous error.'}});stream?.close();voice?.close();process.exit(1);});
write({event:'ready',protocol:1,nativeInitialized:false,backend:process.versions.electron?'electron-main-napi':'node-napi',electronHost:!!process.versions.electron,chromiumRenderer:false,originalUiLoaded:false,browserWindows:0});
