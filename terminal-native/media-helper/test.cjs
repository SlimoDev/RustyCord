'use strict';
const test=require('node:test'),assert=require('node:assert/strict');
const {EventEmitter}=require('node:events');
const p=require('./protocol.cjs');
const {VoiceSession}=require('./voice-session.cjs');
const {sanitizeNativeStats,readStats}=require('./stats.cjs');
const {normalizeAudioSettings,configureInitialDevices,applyAudioSettings,applyInputMode,LocalMicTest}=require('./audio-settings.cjs');
const params={userId:'123',guildId:'456',channelId:'789',sessionId:'fixture-session',voiceToken:'fixture-voice-secret',endpoint:'fixture.discord.media',selfMute:false};
test('audio settings validate ratios, switches and ids; unsupported volumes cannot reach native code',()=>{
  const d=normalizeAudioSettings();assert.equal(d.inputId,'default');assert.equal(d.inputVolume,100);assert.equal(d.pushToTalk,false);
  for(const invalid of [{inputVolume:201},{outputVolume:401},{inputId:''},{noiseSuppression:'true'},{pushToTalk:1}])assert.throws(()=>normalizeAudioSettings(invalid));
  const calls=[],engine=new Proxy({},{get:(_,name)=>(...args)=>calls.push([name,...args])});
  const settings=normalizeAudioSettings({inputId:'fixture-input',outputId:'fixture-output',inputVolume:50,outputVolume:150,noiseSuppression:false,pushToTalk:true});
  applyAudioSettings(engine,settings);applyInputMode(engine,settings);
  assert(calls.some(c=>c[0]==='setInputVolume'&&c[1]===.5));assert(calls.some(c=>c[0]==='setOutputVolume'&&c[1]===1.5));
  assert(calls.some(c=>c[0]==='setTransportOptions'&&c[1].inputMode===2));assert(calls.some(c=>c[0]==='setPTTActive'&&c[1]===false));
  assert(!calls.some(c=>c[0]==='setVideoInputDevice'||c[0]==='VoiceConnection'));
});
test('local microphone test has no network or recording path, caps levels and always disables loopback on stop',()=>{
  const calls=[],events=[];let callback;
  const engine={setOnVoiceCallback:cb=>callback=cb,setLoopback:(...args)=>calls.push(['loopback',...args]),setEmitVADLevel2:value=>calls.push(['vad',value]),setInputDevice:id=>calls.push(['input',id]),setOutputDevice(){},setInputVolume(){},setOutputVolume(){},setTransportOptions(){}};
  const probe=new LocalMicTest(()=>engine,event=>events.push(event));const result=probe.start(normalizeAudioSettings());
  assert.equal(result.localOnly,true);assert.equal(result.maximumSeconds,30);assert.equal(probe.active,true);
  callback(500,true);assert.equal(events.at(-1).level,1);assert.equal(events.at(-1).speaking,true);
  probe.stop();assert.equal(probe.active,false);assert.deepEqual(calls.at(-3),['loopback',false,{}]);assert.deepEqual(calls.at(-2),['vad',false]);assert.deepEqual(calls.at(-1),['input','disabled']);assert.equal(events.at(-1).active,false);
  callback(.5,true);assert.equal(events.at(-1).active,false);assert(!calls.some(c=>/record|connect|socket/i.test(c[0])));
});
test('local microphone test stops automatically and releases input after partial native failure',async()=>{
  const calls=[];const engine={setOnVoiceCallback(){},setLoopback:active=>calls.push(active),setEmitVADLevel2(){},setInputDevice(){},setOutputDevice(){},setInputVolume(){},setOutputVolume(){},setTransportOptions(){}};
  const probe=new LocalMicTest(()=>engine,()=>{},{maxDurationMs:10});probe.start(normalizeAudioSettings());await new Promise(resolve=>setTimeout(resolve,25));assert.equal(probe.active,false);assert.equal(calls.at(-1),false);
  const broken=new LocalMicTest(()=>({...engine,setEmitVADLevel2:active=>{if(active)throw Error('fixture');}}),()=>{});assert.throws(()=>broken.start(normalizeAudioSettings()));assert.equal(broken.active,false);assert.equal(calls.at(-1),false);
});
test('enumerating devices and configuring an idle helper never selects a real microphone',async()=>{
  const calls=[];const engine={setInputDevice:id=>calls.push(['input',id]),setOutputDevice:id=>calls.push(['output',id]),setVideoInputDevice:id=>calls.push(['camera',id]),setInputVolume(){},setOutputVolume(){},setTransportOptions(){},getInputDevices:cb=>cb([{guid:'fixture-input',name:'fixture microphone'}]),getOutputDevices:cb=>cb([{guid:'fixture-output',name:'fixture output'}]),getVideoInputDevices:cb=>cb([{guid:'fixture-camera',name:'fixture camera'}])};
  const native=require('./native.cjs');let initialized=false;
  const factory=()=>{if(!initialized){configureInitialDevices(engine);initialized=true;}return engine;};
  const inventory=await native.listDevices(factory);const cameras=await native.listCameras(factory);
  applyAudioSettings(factory(),normalizeAudioSettings());
  assert.equal(inventory.input[0].id,'fixture-input');assert.equal(cameras[0].id,'fixture-camera');
  assert(calls.filter(c=>c[0]==='input').every(c=>c[1]==='disabled'));assert(calls.filter(c=>c[0]==='camera').every(c=>c[1]==='disabled'));
  applyAudioSettings(engine,normalizeAudioSettings({inputId:'fixture-input'}),{captureInput:true});
  assert.deepEqual(calls.filter(c=>c[0]==='input').at(-1),['input','fixture-input']);
  const inputCalls=calls.filter(c=>c[0]==='input').length;await native.listDevices(factory);await native.listCameras(factory);
  assert.equal(calls.filter(c=>c[0]==='input').length,inputCalls,'enumeration preserves an already authorized active input');
});
test('only Discord secure endpoints are admitted; userinfo, paths, and local addresses rejected',()=>{assert.equal(p.endpointURL('fixture.discord.media:443'),'wss://fixture.discord.media/?v=8');for(const bad of ['localhost','127.0.0.1','evil.test','discord.media.evil.test','user@fixture.discord.media','fixture.discord.media/path','fixture.discord.media?q=x','ws://fixture.discord.media'])assert.throws(()=>p.endpointURL(bad));});
test('binary framing preserves sequence and transition id without leaking credentials',()=>{const packet=Buffer.from([0,42,30,0,9,7,8]);const frame=p.parseVoiceFrame(packet);assert.equal(frame.seq,42);assert.equal(frame.op,30);assert.deepEqual(p.transitionPayload(frame.d),{id:9,data:Buffer.from([7,8])});assert.deepEqual(p.clientBinary(26,Buffer.from([7,8])),Buffer.from([26,7,8]));assert.throws(()=>p.parseVoiceFrame(Buffer.from([1,2])));});
test('deprecated transport encryption is not selected',()=>{assert.equal(p.chooseEncryption(['aead_xchacha20_poly1305_rtpsize'],['aead_xchacha20_poly1305_rtpsize']),'aead_xchacha20_poly1305_rtpsize');assert.throws(()=>p.chooseEncryption(['xsalsa20_poly1305'],['xsalsa20_poly1305']));});
test('native volume uses a linear ratio, not UI percentage',()=>{assert.equal(p.volumeScale(100),1);assert.equal(p.volumeScale(50),.5);assert.equal(p.volumeScale(400),4);assert.throws(()=>p.volumeScale(Infinity));assert.throws(()=>p.volumeScale(-1));});
test('stream uses the distinct RTC channel for Identify and MLS, without losing the actual guild channel',()=>{const s=p.credentials({...params,rtcServerId:'9007199254740999'},true);assert.equal(s.channelId,'9007199254740998');assert.equal(s.originalChannelId,'789');assert.equal(p.credentials({...params,rtcServerId:'9007199254740999',streamChannelId:'800'},true).channelId,'800');});
function fixture(audioSettings,dependencies={},optionOverrides={}){
  const calls=[];let connectCallback,prepareCallback,videoCallback,speakingCallback,voiceCallback,sourceEnded,soundshareCallback,soundshareFailed;let socket;
  class WS extends EventEmitter{constructor(){super();this.readyState=1;this.sent=[];socket=this;}addEventListener(event,fn){this.on(event,fn);}send(data){this.sent.push(data);}close(){this.readyState=3;}}
  class Conn{constructor(id,options,cb){calls.push(['construct',options]);connectCallback=cb;}destroy(){calls.push(['destroy']);}setSelfMute(x){calls.push(['mute',x]);}setSelfDeafen(x){calls.push(['deaf',x]);}setPTTActive(x){calls.push(['ptt',x]);}setTransportOptions(x){calls.push(['options',x]);}setOnSpeakingCallback(cb){speakingCallback=cb;}setOnVideoCallback(cb){videoCallback=cb;}setVideoBroadcast(x){calls.push(['broadcast',x]);}clearDesktopSource(){calls.push(['clear-source']);}setDesktopSourceWithOptions(x){calls.push(['source',x]);}setOnConnectionFailedCallback(){}setOnMLSFailureCallback(){}setOnDesktopSourceEnded(cb){sourceEnded=cb;}setOnSoundshare(cb){soundshareCallback=cb;}setOnSoundshareFailed(cb){soundshareFailed=cb;}setPingInterval(){}mergeUsers(x){calls.push(['merge',x]);}destroyUser(){}setRemoteUserSpeakingStatus(){}getEncryptionModes(cb){cb(['aead_aes256_gcm_rtpsize']);}prepareSecureFramesEpoch(...x){calls.push(['epoch',...x]);}getMLSKeyPackage(cb){cb(Buffer.from([1,2]));}prepareSecureFramesTransition(id,version,cb){prepareCallback=cb;}executeSecureFramesTransition(id){calls.push(['execute',id]);}}
  const m={SupportedSecureFramesProtocolVersion:1,VoiceConnection:Conn,DegradationPreference:{MAINTAIN_FRAMERATE:'maintain-framerate'},getCodecCapabilities:cb=>cb([{codec:'H264',encode:true}]),setVideoInputDevice:id=>calls.push(['camera-device',id]),setInputDevice:id=>calls.push(['input-device',id]),setOnVoiceCallback:cb=>voiceCallback=cb,setEmitVADLevel2:active=>calls.push(['vad',active])};
  const session=new VoiceSession({...params,...optionOverrides,audioSettings},{engineFactory:()=>m,WebSocketClass:WS,...dependencies});const states=[];session.on('state',x=>states.push(x));
  return {session,calls,states,engine:m,get socket(){return socket;},connect:()=>connectCallback(null,{protocol:'udp',address:'203.0.113.10',port:5000}),prepared:()=>prepareCallback(),video:(...args)=>videoCallback(...args),speaking:(...args)=>speakingCallback(...args),vad:(...args)=>voiceCallback?.(...args),sourceEnded:()=>sourceEnded?.(),soundshare:active=>soundshareCallback?.(active),soundshareFail:()=>soundshareFailed?.()};
}
test('a new encrypted voice transport applies the saved PTT mode before it can unmute',async()=>{
  const f=fixture({pushToTalk:true});f.session.start();
  try{
    const ready=f.session.handleFrame({op:2,d:{ip:'203.0.113.1',port:5000,ssrc:1,modes:['aead_aes256_gcm_rtpsize']}});f.connect();await ready;
    assert(f.calls.some(c=>c[0]==='options'&&c[1].inputMode===2));assert(!f.calls.some(c=>c[0]==='mute'&&c[1]===false));assert.equal(f.session.connected,false);
  }finally{f.session.close();}
});
test('UDP and Session Description alone never produce connected or unmute; DAVE transition is required',async()=>{
  const f=fixture();f.session.start();f.socket.emit('open');const identify=JSON.parse(f.socket.sent[0]);assert.equal(identify.op,0);assert.equal(identify.d.max_dave_protocol_version,1);
  const ready=f.session.handleFrame({op:2,d:{ip:'203.0.113.1',port:5000,ssrc:1,modes:['aead_aes256_gcm_rtpsize']}});f.connect();await ready;
  assert.equal(f.session.connected,false);assert(!f.states.some(s=>s.state==='connected'));
  assert(!f.calls.some(x=>x[0]==='input-device'&&x[1]!=='disabled'));
  await f.session.handleFrame({op:4,d:{mode:'aead_aes256_gcm_rtpsize',secret_key:Array(32).fill(7),audio_codec:'opus',dave_protocol_version:1}});
  assert.equal(f.session.connected,false);assert(!f.calls.some(x=>x[0]==='mute'&&x[1]===false));
  assert(f.calls.some(x=>x[0]==='input-device'&&x[1]==='default'));assert(f.session.localReady);assert.equal(f.states.at(-1).state,'waiting-peer');assert.equal(f.states.at(-1).encryptionReady,false);
  await f.session.handleFrame({op:21,d:{transition_id:0,protocol_version:1}});f.prepared();assert.equal(f.session.connected,true);assert(f.calls.some(x=>x[0]==='execute'&&x[1]===0));assert(f.calls.some(x=>x[0]==='mute'&&x[1]===false));
  assert(f.calls.some(x=>x[0]==='input-device'&&x[1]==='default'));
  f.session.close();assert.equal(f.session.options.voiceToken,'');assert.equal(f.session.options.sessionId,'');assert.deepEqual(f.calls.filter(x=>x[0]==='input-device').at(-1),['input-device','disabled']);
});
test('DAVE downgrade fails and native error content is never exposed',async()=>{const f=fixture();f.session.start();await assert.rejects(f.session.handleFrame({op:21,d:{transition_id:0,protocol_version:0}}),{code:'DAVE_REQUIRED'});const e=p.safeError(new Error('fixture-voice-secret'));assert(!JSON.stringify(e).includes('fixture-voice-secret'));f.session.close();});
function fakeClock(){let now=0,id=0;const jobs=new Map();return{setTimeoutFn(fn,delay){const key=++id;jobs.set(key,{fn,at:now+delay});return key;},clearTimeoutFn(key){jobs.delete(key);},advance(ms){now+=ms;for(const[key,job]of [...jobs])if(job.at<=now){jobs.delete(key);job.fn();}},get count(){return jobs.size;}};}
async function localVoiceFixture(audioSettings,clock=fakeClock()){
  const f=fixture(audioSettings,clock);f.session.start();f.socket.emit('open');
  const ready=f.session.handleFrame({op:2,d:{ip:'203.0.113.1',port:5000,ssrc:1,modes:['aead_aes256_gcm_rtpsize']}});f.connect();await ready;
  await f.session.handleFrame({op:4,d:{mode:'aead_aes256_gcm_rtpsize',secret_key:Array(32).fill(7),audio_codec:'opus',dave_protocol_version:1}});return{...f,clock};
}
test('a sole call stays locally ready for over 30 seconds without declaring encrypted connected or sending speaking',async()=>{
  const f=await localVoiceFixture(),events=[];f.session.on('speaking',value=>events.push(value));
  try{
    f.clock.advance(120000);assert.equal(f.session.closed,false);assert.equal(f.session.localReady,true);assert.equal(f.session.connected,false);assert.equal(f.clock.count,0);
    assert.equal(f.states.at(-1).state,'waiting-peer');assert.equal(f.states.at(-1).transportReady,true);assert.equal(f.states.at(-1).encryptionReady,false);
    f.vad(-20,true);f.vad(-80,false);assert.deepEqual(events.map(e=>e.active),[true,false]);assert(!f.socket.sent.some(frame=>typeof frame==='string'&&JSON.parse(frame).op===5));
    assert(!f.calls.some(call=>call[0]==='mute'&&call[1]===false));
    await assert.rejects(f.session.startCamera('fixture-camera-guid'),{code:'CAMERA_UNAVAILABLE'});assert.throws(()=>f.session.share('screen:1'),{code:'SHARE_NOT_READY'});
  }finally{f.session.close();}
});
test('first peer arms bounded negotiation and nonzero transition must actually execute before send unmutes',async()=>{
  const f=await localVoiceFixture();
  try{
    await f.session.handleFrame({op:11,d:{user_ids:['456']}});assert.equal(f.states.at(-1).state,'negotiating');assert.equal(f.states.at(-1).transportReady,true);assert.equal(f.clock.count,1);
    f.clock.advance(29999);assert.equal(f.session.closed,false);
    await f.session.handleFrame({op:21,d:{transition_id:7,protocol_version:1}});f.prepared();assert.equal(f.session.connected,false);assert(!f.calls.some(call=>call[0]==='mute'&&call[1]===false));assert(f.socket.sent.some(frame=>typeof frame==='string'&&JSON.parse(frame).op===23&&JSON.parse(frame).d.transition_id===7));
    await f.session.handleFrame({op:22,d:{transition_id:7}});assert.equal(f.session.connected,true);assert.equal(f.states.at(-1).encryptionReady,true);assert(f.calls.some(call=>call[0]==='mute'&&call[1]===false));assert.deepEqual(f.calls.filter(c=>c[0]==='vad').at(-1),['vad',false]);
    f.clock.advance(120000);assert.equal(f.session.closed,false);assert.equal(f.clock.count,0);
  }finally{f.session.close();}
});
test('peer departure cancels pending group timeout while a peer that never negotiates produces an error',async()=>{
  const f=await localVoiceFixture();
  try{
    f.session.addUser('456',2);f.clock.advance(1000);await f.session.handleFrame({op:13,d:{user_id:'456'}});assert.equal(f.clock.count,0);f.clock.advance(30000);assert.equal(f.session.closed,false);assert.equal(f.states.at(-1).state,'waiting-peer');
    assert.equal(f.calls.filter(c=>c[0]==='input-device'&&c[1]==='default').length,1);
    f.session.addUser('789',3);f.clock.advance(30001);assert.equal(f.session.closed,true);assert.equal(f.states.at(-1).state,'error');assert.equal(f.states.at(-1).reason.code,'VOICE_ENCRYPTION_TIMEOUT');assert(!f.calls.some(call=>call[0]==='mute'&&call[1]===false));
  }finally{f.session.close();}
});
test('sole-member epoch reset withdraws encrypted readiness and stale native callbacks cannot re-enable it',async()=>{
  const f=await localVoiceFixture();
  try{
    await f.session.handleFrame({op:21,d:{transition_id:0,protocol_version:1}});f.prepared();assert.equal(f.session.connected,true);
    await f.session.handleFrame({op:21,d:{transition_id:9,protocol_version:1}});
    await f.session.handleFrame({op:24,d:{epoch:1,protocol_version:1}});assert.equal(f.session.connected,false);assert.equal(f.session.daveReady,false);assert.equal(f.states.at(-1).state,'waiting-peer');assert.deepEqual(f.calls.filter(c=>c[0]==='mute').at(-1),['mute',true]);
    f.prepared();assert.equal(f.session.connected,false);assert.equal(f.session.pendingTransitions.size,0);
    await f.session.handleFrame({op:21,d:{transition_id:0,protocol_version:1}});f.prepared();assert.equal(f.session.connected,true);
  }finally{f.session.close();}
});
test('local VAD respects mute, deafen and held push to talk without sending network speaking before DAVE',async()=>{
  const f=await localVoiceFixture({pushToTalk:true}),events=[];f.session.on('speaking',value=>events.push(value));
  try{
    f.vad(-20,true);assert.equal(events.at(-1)?.active,false);f.session.ptt(true);f.vad(-20,true);assert.equal(events.at(-1).active,true);
    f.session.mute(true);assert.equal(events.at(-1).active,false);f.vad(-20,true);assert.equal(events.at(-1).active,false);f.session.mute(false);f.vad(-20,true);assert.equal(events.at(-1).active,true);
    f.session.deafen(true);assert.equal(events.at(-1).active,false);f.session.deafen(false);f.vad(-20,true);assert.equal(events.at(-1).active,true);f.session.ptt(false);assert.equal(events.at(-1).active,false);
    f.session.ptt(true);await f.session.handleFrame({op:21,d:{transition_id:0,protocol_version:1}});f.prepared();assert.equal(f.session.connected,true);assert(f.calls.some(c=>c[0]==='ptt'&&c[1]===true));
    assert(!f.socket.sent.some(frame=>typeof frame==='string'&&JSON.parse(frame).op===5));
  }finally{f.session.close();}
});
async function streamFixture(){
  const clock=fakeClock(),f=fixture(undefined,{stream:true,...clock},{rtcServerId:'9007199254740999'});f.session.start();f.socket.emit('open');
  const ready=f.session.handleFrame({op:2,d:{ip:'203.0.113.1',port:5000,ssrc:1,modes:['aead_aes256_gcm_rtpsize'],streams:[{rid:'100',ssrc:2,rtx_ssrc:3,quality:100}]}});f.connect();await ready;
  await f.session.handleFrame({op:4,d:{mode:'aead_aes256_gcm_rtpsize',secret_key:Array(32).fill(7),audio_codec:'opus',video_codec:'H264',dave_protocol_version:1}});await f.session.handleFrame({op:21,d:{transition_id:0,protocol_version:1}});f.prepared();return{...f,clock};
}
test('screen rekey pauses capture first and resumes only the same explicit source after genuine DAVE readiness',async()=>{
  const f=await streamFixture();
  try{
    f.session.share('window:900',{height:720,fps:30},{enabled:true,loopback:true,pid:0});assert.equal(f.session.captureActive,true);assert.equal(f.session.soundshareRequested,true);
    const start=f.calls.length;await f.session.handleFrame({op:24,d:{epoch:1,protocol_version:1}});const changed=f.calls.slice(start);
    assert.equal(f.session.connected,false);assert.equal(f.session.captureActive,false);assert.equal(f.session.suspendedShare.sourceId,'window:900');assert(f.states.some(s=>s.state==='paused'));
    assert(changed.findIndex(c=>c[0]==='clear-source')<changed.findIndex(c=>c[0]==='epoch'));assert.deepEqual(changed.find(c=>c[0]==='broadcast'),['broadcast',false]);
    assert.equal(f.session.soundshareRequested,false);assert(changed.findIndex(c=>c[0]==='options'&&c[1].soundsharePid===0&&c[1].soundshareLoopback===false)<changed.findIndex(c=>c[0]==='epoch'));
    f.sourceEnded();assert.equal(f.session.closed,false,'a delayed intentional-clear callback cannot terminate the pause');
    f.soundshare(true);f.soundshareFail();assert.equal(f.session.closed,false);assert.equal(f.session.soundshareAttached,false,'late audio callbacks cannot reactivate a paused capture');
    assert.equal(f.calls.filter(c=>c[0]==='source').length,1);await f.session.handleFrame({op:21,d:{transition_id:8,protocol_version:1}});f.prepared();assert.equal(f.calls.filter(c=>c[0]==='source').length,1);
    await f.session.handleFrame({op:22,d:{transition_id:8}});assert.equal(f.session.captureActive,true);assert.equal(f.calls.filter(c=>c[0]==='source').length,2);assert.equal(f.calls.filter(c=>c[0]==='source').at(-1)[1].sourceId,'900');assert.equal(f.states.at(-1).state,'capture-requested');assert.equal(f.session.soundshareRequested,true);assert.equal(f.calls.filter(c=>c[0]==='options'&&c[1].soundshareLoopback===true).length,2);
    f.sourceEnded();assert.equal(f.session.closed,true);assert.equal(f.session.suspendedShare,null);
    assert(!f.calls.some(c=>c[0]==='input-device'||c[0]==='vad'),'a separate stream never touches the voice microphone or its VAD callback');
  }finally{f.session.close();}
});
test('stopping or disconnecting during a screen pause cancels automatic resume',async()=>{
  for(const disconnect of [false,true]){
    const f=await streamFixture();
    try{
      f.session.share('screen:1',{height:720,fps:30},{enabled:false});await f.session.handleFrame({op:24,d:{epoch:1,protocol_version:1}});assert(f.session.suspendedShare);
      if(disconnect)f.session.close();else f.session.stopShare();assert.equal(f.session.suspendedShare,null);
      await f.session.handleFrame({op:21,d:{transition_id:0,protocol_version:1}});if(!disconnect)f.prepared();assert.equal(f.calls.filter(c=>c[0]==='source').length,1);assert.equal(f.session.captureActive,false);
    }finally{f.session.close();}
  }
});
test('a stale MLS proposal callback cannot upload a commit after a group reset',async()=>{
  const f=await localVoiceFixture();let complete;
  try{
    f.session.conn.processMLSProposals=(input,callback)=>complete=callback;await f.session.handleFrame({op:27,d:Buffer.from([0,1])});
    await f.session.handleFrame({op:24,d:{epoch:1,protocol_version:1}});complete(Buffer.from([7,8]));assert(!f.socket.sent.some(frame=>Buffer.isBuffer(frame)&&frame[0]===28));
  }finally{f.session.close();}
});
test('stats projects native counters and discards every address, id, title and arbitrary string',()=>{
  const report=sanitizeNativeStats({transport:{localAddress:'fixture-private-address',secret_key:'fixture-key',secureFramesProtocolVersion:1,bytesSent:100,bytesReceived:200,rtt:24},outbound:{audio:{codecName:'opus',bytesSent:80,packetsSent:2,encryptSuccessCount:1},videos:[{codecName:'H264',inputFrameRate:30,encodeFrameRate:29,mediaBitrate:400000,substreams:[{ssrc:999,isRTX:false,width:1280,height:720,frameCounts:{keyFrames:1,deltaFrames:29},rtpStats:{headerBytes:10,payloadBytes:90,packets:3}}]}]},inbound:[{id:'fixture-user-id',name:'fixture-window-title',audio:{codecName:'opus',bytesReceived:150,packetsReceived:4,decryptSuccessCount:3}}],token:'fixture-token'},{state:'connected',daveNegotiated:true});
  assert.equal(report.outbound.video[0].width,1280);assert.equal(report.outbound.video[0].height,720);assert.equal(report.outbound.video[0].fpsEncode,29);assert.equal(report.outbound.video[0].bytesSent,100);assert.equal(report.outbound.video[0].framesSent,30);assert.equal(report.inbound.audio[0].crypto.success,3);assert.equal(report.encryption.nativeDaveEnabled,true);
  const json=JSON.stringify(report);for(const privateText of ['fixture-','ssrc','localAddress','secret_key','token'])assert(!json.includes(privateText));
});
test('missing native values remain null; no requested quality or negotiated codec is used as measured data',()=>{
  const report=sanitizeNativeStats({transport:{secureFramesProtocolVersion:undefined,rtt:NaN},outbound:{audio:{codecName:'private-text',bytesSent:'100'},video:{codecName:'H264',encodeFrameRate:Infinity}}});
  assert.equal(report.encryption.nativeDaveEnabled,null);assert.equal(report.transport.rttMs,null);assert.equal(report.outbound.audio.codec,null);assert.equal(report.outbound.audio.bytes,null);assert.equal(report.outbound.video[0].width,null);assert.equal(report.outbound.video[0].fpsEncode,null);assert.equal(report.interval.sentBitrateBps,null);
});
test('stats does not initialize the engine when there are no media sessions',async()=>{const result=await readStats(null,null);assert.equal(result.sanitized,true);assert.equal(result.voice,null);assert.equal(result.share,null);assert(Number.isFinite(Date.parse(result.sampledAtUtc)));assert(!Object.keys(require.cache).some(path=>path.endsWith('discord_voice.node')));});

async function cameraFixture(){
  const f=fixture();f.session.start();f.socket.emit('open');
  const ready=f.session.handleFrame({op:2,d:{ip:'203.0.113.1',port:5000,ssrc:1,modes:['aead_aes256_gcm_rtpsize'],streams:[{rid:'100',ssrc:2,rtx_ssrc:3,quality:100}]}});f.connect();await ready;
  await f.session.handleFrame({op:4,d:{mode:'aead_aes256_gcm_rtpsize',secret_key:Array(32).fill(7),audio_codec:'opus',video_codec:'H264',dave_protocol_version:1}});
  await f.session.handleFrame({op:21,d:{transition_id:0,protocol_version:1}});f.prepared();return f;
}
test('voice negotiates inactive video without opening or broadcasting a camera',async()=>{
  const f=await cameraFixture();try{const identify=JSON.parse(f.socket.sent[0]);assert.equal(identify.d.video,true);assert.equal(identify.d.streams[0].active,false);assert(!f.calls.some(x=>['camera-device','broadcast'].includes(x[0])));assert.equal(f.states.at(-1).cameraAvailable,true);assert.equal(f.session.connected,true);}finally{f.session.close();}
});
test('explicit camera activation waits for native self video; stop disables device and announces inactive',async()=>{
  const f=await cameraFixture();try{
    const states=[];f.session.on('camera',s=>states.push(s.state));let completed=false;
    const pending=f.session.startCamera('fixture-camera-guid').then(r=>{completed=true;return r;});await Promise.resolve();
    assert.equal(completed,false);assert.equal(f.session.cameraActive,false);assert(f.calls.some(x=>x[0]==='camera-device'&&x[1]==='fixture-camera-guid'));assert(f.calls.some(x=>x[0]==='broadcast'&&x[1]===true));
    f.video('123',2,'fixture-local-stream',[{rid:'100',ssrc:2,rtxSsrc:3,active:true}]);assert.equal((await pending).state,'active');assert.equal(f.session.cameraActive,true);
    f.session.stopCamera();assert.equal(f.session.cameraActive,false);assert(f.calls.some(x=>x[0]==='camera-device'&&x[1]==='disabled'));
    const frames=f.socket.sent.filter(x=>typeof x==='string').map(JSON.parse).filter(x=>x.op===12);assert.equal(frames[0].d.video_ssrc,2);assert.equal(frames.at(-1).d.video_ssrc,0);assert(frames.at(-1).d.streams.every(s=>!s.active));assert(states.includes('preparing')&&states.includes('active')&&states.includes('stopped'));
  }finally{f.session.close();}
});
test('an epoch reset stops the active camera and real encryption recovery does not reopen it',async()=>{
  const f=await cameraFixture();try{
    const pending=f.session.startCamera('fixture-camera-guid');f.video('123',2,'fixture-local-stream',[{rid:'100',ssrc:2,rtxSsrc:3,active:true}]);await pending;
    await f.session.handleFrame({op:24,d:{epoch:1,protocol_version:1}});
    assert.equal(f.session.connected,false);assert.equal(f.session.cameraActive,false);assert.equal(f.session.cameraRequested,false);
    assert.deepEqual(f.calls.filter(c=>c[0]==='camera-device').at(-1),['camera-device','disabled']);assert.deepEqual(f.calls.filter(c=>c[0]==='broadcast').at(-1),['broadcast',false]);
    const cameraStarts=f.calls.filter(c=>c[0]==='camera-device'&&c[1]!=='disabled').length;
    await f.session.handleFrame({op:21,d:{transition_id:0,protocol_version:1}});f.prepared();
    assert.equal(f.session.connected,true);assert.equal(f.session.cameraActive,false);assert.equal(f.calls.filter(c=>c[0]==='camera-device'&&c[1]!=='disabled').length,cameraStarts);
  }finally{f.session.close();}
});
test('camera initialization failure disables device and preserves encrypted voice',async()=>{
  const f=await cameraFixture();try{f.engine.setVideoInputDevice=id=>{f.calls.push(['camera-device',id]);if(id!=='disabled')throw new Error('synthetic private device error');};await assert.rejects(f.session.startCamera('fixture-camera-guid'));assert.equal(f.session.cameraRequested,false);assert.equal(f.session.cameraActive,false);assert.equal(f.session.connected,true);assert(f.calls.some(x=>x[0]==='camera-device'&&x[1]==='disabled'));}finally{f.session.close();}
});
test('active camera losing its SSRC stops instead of preparing forever, including reentrant native callback',async()=>{
  const f=await cameraFixture();try{
    const states=[];f.session.on('camera',s=>states.push(s));
    const pending=f.session.startCamera('fixture-camera-guid');f.video('123',2,'fixture-local-stream',[{rid:'100',ssrc:2,rtxSsrc:3,active:true}]);await pending;
    let stopCalls=0;f.session.conn.setVideoBroadcast=value=>{if(!value){stopCalls++;f.video('123',0,'',[]);}};
    f.video('123',0,'',[]);
    assert.equal(stopCalls,1);assert.equal(f.session.cameraRequested,false);assert.equal(f.session.cameraActive,false);assert.equal(f.session.connected,true);
    assert.equal(states.at(-1).state,'error');assert.equal(states.at(-1).reason.code,'CAMERA_SOURCE_ENDED');
    assert(!states.slice(states.findIndex(s=>s.state==='active')+1).some(s=>s.state==='preparing'));
    assert(f.calls.some(x=>x[0]==='camera-device'&&x[1]==='disabled'));
    const video=f.socket.sent.filter(x=>typeof x==='string').map(JSON.parse).filter(x=>x.op===12);assert.equal(video.at(-1).d.video_ssrc,0);
  }finally{f.session.close();}
});
test('disconnect cancels a pending camera and releases device before destroying transport',async()=>{
  const f=await cameraFixture();const pending=f.session.startCamera('fixture-camera-guid');const rejected=assert.rejects(pending,{code:'CAMERA_STOPPED'});f.session.close();await rejected;const stop=f.calls.findIndex(x=>x[0]==='camera-device'&&x[1]==='disabled'),destroy=f.calls.findIndex(x=>x[0]==='destroy');assert(stop>=0&&destroy>stop);assert.equal(f.session.closed,true);
});
test('camera cannot activate before encryption, without negotiation, or in Go Live session',async()=>{
  const f=fixture();await assert.rejects(f.session.startCamera('fixture-camera-guid'),{code:'CAMERA_UNAVAILABLE'});assert(!f.calls.length);const ready=await cameraFixture();try{ready.session.stream=true;await assert.rejects(ready.session.startCamera('fixture-camera-guid'),{code:'CAMERA_UNAVAILABLE'});assert(!ready.calls.some(x=>x[0]==='camera-device'));}finally{ready.session.close();}
});
test('participant and speaking feedback reflects live events and suppresses repeated speaking state',async()=>{
  const f=await cameraFixture();try{const people=[],speaking=[];f.session.on('participant',p=>people.push(p));f.session.on('speaking',s=>speaking.push(s));await f.session.handleFrame({op:11,d:{user_ids:['222']}});f.speaking('222',1);f.speaking('222',1);f.speaking('222',0);assert.deepEqual(speaking.map(s=>s.active),[true,false]);assert(speaking.every(s=>s.userId==='222'&&!s.self));await f.session.handleFrame({op:12,d:{user_id:'222',audio_ssrc:50,video_ssrc:51}});await f.session.handleFrame({op:13,d:{user_id:'222'}});assert(people.some(p=>p.present&&p.video));assert.equal(people.at(-1).present,false);assert.equal(f.session.members.size,0);}finally{f.session.close();}
});
