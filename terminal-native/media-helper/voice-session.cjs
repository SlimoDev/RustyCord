'use strict';
const {EventEmitter}=require('node:events');
const {MediaError,credentials,parseVoiceFrame,clientBinary,transitionPayload,chooseEncryption,safeError}=require('./protocol.cjs');
const {initialize,callbackPromise}=require('./native.cjs');
const {normalizeAudioSettings,applyInputMode}=require('./audio-settings.cjs');
const AUDIO={type:'audio',name:'opus',priority:1000,payload_type:120};
class VoiceSession extends EventEmitter {
  constructor(params,{stream=false,engineFactory=initialize,WebSocketClass=global.WebSocket,setTimeoutFn=setTimeout,clearTimeoutFn=clearTimeout}={}) {
    super();this.options=credentials(params,stream);this.stream=stream;this.engineFactory=engineFactory;this.WebSocketClass=WebSocketClass;this.audioSettings=normalizeAudioSettings(params.audioSettings);
    this.closed=false;this.connected=false;this.localReady=false;this.sessionDescription=false;this.daveReady=false;this.seq=-1;this.members=new Map();this.localVolumes=new Map();this.speakingStates=new Map();this.queue=Promise.resolve();this.pendingMessages=0;this.failedMLS=0;this.pendingTransitions=new Set();this.cameraRequested=false;this.cameraActive=false;this.epochGeneration=0;this.pttActive=false;this.setTimeout=setTimeoutFn;this.clearTimeout=clearTimeoutFn;
  }
  start() {
    this.engine=this.engineFactory();
    if(!(this.engine.SupportedSecureFramesProtocolVersion>=1))throw new MediaError('DAVE_UNAVAILABLE','The media engine does not support required DAVE encryption.');
    this.emitState('connecting');
    this.socket=new this.WebSocketClass(this.options.endpoint);this.socket.binaryType='arraybuffer';
    this.socket.addEventListener('open',()=>{if(this.closed)return;this.emitStage('websocket-open');const p=this.options;this.send(0,{server_id:p.serverId,channel_id:p.channelId,user_id:p.userId,session_id:p.sessionId,token:p.voiceToken,max_dave_protocol_version:1,video:true,streams:[{type:'video',rid:'100',quality:100,active:false}]});});
    this.socket.addEventListener('message',ev=>{
      if(this.closed)return;
      if(++this.pendingMessages>128)return this.fail(new MediaError('VOICE_QUEUE_LIMIT','Voice signaling queue exceeded its limit.'));
      this.queue=this.queue.then(()=>this.handleFrame(parseVoiceFrame(ev.data))).catch(e=>this.fail(e)).finally(()=>this.pendingMessages--);
    });
    this.socket.addEventListener('error',()=>this.fail(new MediaError('VOICE_SOCKET_ERROR','The voice WebSocket failed.')));
    this.socket.addEventListener('close',ev=>{if(!this.closed)this.fail(new MediaError('VOICE_SOCKET_CLOSED',`The voice WebSocket closed (${Number(ev.code)||0}). Rejoin the call to obtain a fresh session.`));});
    this.connectTimeout=this.setTimeout(()=>this.fail(new MediaError('VOICE_CONNECT_TIMEOUT','The voice transport did not finish in time.')),30000);
    return {state:'connecting',verifiedLive:false};
  }
  emitState(state,reason) {this.emit('state',{state,...(reason?{reason}:{}),kind:this.stream?'share':'voice',transportReady:this.localReady,encryptionReady:this.connected,...(!this.stream?{cameraAvailable:this.connected&&!!this.videoCodec&&!!this.streamParameters?.length}:{})});}
  emitStage(stage) {this.emit('stage',{stage,kind:this.stream?'share':'voice'});}
  send(op,d){if(this.socket?.readyState===1)this.socket.send(JSON.stringify({op,d}));}
  sendBinary(op,d){if(this.socket?.readyState===1)this.socket.send(clientBinary(op,d));}
  beat(){if(this.heartbeatPending&&Date.now()-this.heartbeatPending>this.heartbeatInterval*2)return this.fail(new MediaError('VOICE_HEARTBEAT_TIMEOUT','The voice server stopped acknowledging heartbeats.'));this.heartbeatPending=Date.now();this.send(3,{t:this.heartbeatPending,seq_ack:this.seq});}
  async handleFrame(p) {
    if(this.closed)return;
    if(Number.isInteger(p.seq))this.seq=p.seq;
    const d=p.d;
    switch(p.op){
      case 8:{const interval=Number(d?.heartbeat_interval);if(!Number.isFinite(interval)||interval<1000)throw new MediaError('INVALID_HELLO','Invalid voice heartbeat interval.');clearInterval(this.heartbeat);this.heartbeatInterval=Math.min(interval,15000);this.heartbeat=setInterval(()=>this.beat(),this.heartbeatInterval);this.beat();break;}
      case 3:this.beat();break;
      case 6:this.heartbeatPending=0;break;
      case 2:await this.ready(d);break;
      case 4:this.describe(d);break;
      case 5:this.addUser(d.user_id,d.ssrc);this.conn?.setRemoteUserSpeakingStatus(d.user_id,Number(d.speaking)||0);this.speaking(d.user_id,Number(d.speaking)||0);break;
      case 11:for(const id of d.user_ids||[])this.addUser(id,0);break;
      case 12:this.addUser(d.user_id,d.audio_ssrc);if(!this.stream)this.emit('participant',{userId:d.user_id,present:true,video:Number(d.video_ssrc)>0});break;
      case 13:if(typeof d.user_id==='string'){this.members.delete(d.user_id);this.speakingStates.delete(d.user_id);this.conn?.destroyUser(d.user_id);if(!this.stream)this.emit('participant',{userId:d.user_id,present:false});this.updateReadiness();}break;
      case 14:if(d.audio_codec&&d.audio_codec!=='opus')throw new MediaError('UNSUPPORTED_CODEC','The voice server selected an unsupported audio codec.');break;
      case 15:if(this.stream&&this.conn)this.conn.setTransportOptions({remoteSinkWantsMaxFramerate:this.shareQuality?.fps||30});break;
      case 21:this.prepareTransition(d);break;
      case 22:this.executeTransition(d.transition_id);break;
      case 24:this.prepareEpoch(d.epoch,d.protocol_version);break;
      case 25:this.needConnection().updateMLSExternalSender(new Uint8Array(d));break;
      case 27:{const generation=this.epochGeneration;this.needConnection().processMLSProposals(new Uint8Array(d),out=>{if(!this.closed&&generation===this.epochGeneration&&out?.byteLength)this.sendBinary(28,out);});break;}
      case 29:case 30:{const t=transitionPayload(d),conn=this.needConnection(),generation=this.epochGeneration;const callback=(ok,version)=>{if(this.closed||generation!==this.epochGeneration)return;if(!ok){if(++this.failedMLS>=5)return this.fail(new MediaError('MLS_VALIDATION_FAILED','Repeated encrypted group validation failure.'));this.send(31,{transition_id:t.id});this.prepareEpoch('1',1);return;}if(version!==1)return this.fail(new MediaError('DAVE_VERSION_UNSUPPORTED','The encrypted group selected an unsupported protocol.'));this.failedMLS=0;if(t.id===0){this.daveReady=true;this.checkConnected();}else{this.pendingTransitions.add(t.id);this.send(23,{transition_id:t.id});}};if(p.op===29)conn.prepareMLSCommitTransition(t.id,new Uint8Array(t.data),callback);else conn.processMLSWelcome(t.id,new Uint8Array(t.data),callback);break;}
      // Unknown non-critical diagnostic opcodes do not affect transport state.
    }
  }
  needConnection(){if(!this.conn)throw new MediaError('INVALID_VOICE_ORDER','Voice encryption arrived before native transport creation.');return this.conn;}
  async ready(d) {
    this.emitStage('ready-received');
    if(this.conn)throw new MediaError('DUPLICATE_READY','Unexpected duplicate voice transport.');
    if(typeof d.ip!=='string'||!Number.isInteger(d.port)||d.port<1||d.port>65535||!Number.isInteger(d.ssrc)||!Array.isArray(d.modes))throw new MediaError('INVALID_READY','The voice server sent an invalid transport description.');
    this.emitState('negotiating');this.ssrc=d.ssrc;
    this.streamParameters=d.streams?.length?d.streams.filter(s=>Number.isInteger(s.ssrc)&&s.ssrc>0).map(s=>({type:'video',rid:String(s.rid||'100'),ssrc:s.ssrc,rtxSsrc:s.rtx_ssrc,quality:s.quality||100,active:false})):(this.stream?[{type:'video',rid:'100',ssrc:d.ssrc+1,rtxSsrc:d.ssrc+2,quality:100,active:false}]:[]);
    const options={ssrc:d.ssrc,address:d.ip,port:d.port,context:this.options.context,...(this.stream?{streamUserId:this.options.userId}:{}),experiments:[],qosEnabled:true,streamParameters:[{type:'audio',ssrc:d.ssrc,rid:'',maxBitrate:64000,soundshare:this.stream},...this.streamParameters]};
    this.emitStage('native-connection-before');
    const info=await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(new MediaError('UDP_TIMEOUT','The native UDP transport did not connect.')),15000);try{this.conn=new this.engine.VoiceConnection(this.options.userId,options,(error,transport)=>{clearTimeout(timeout);if(error||!transport)reject(new MediaError('UDP_FAILED','The native voice transport could not connect.'));else resolve(transport);});this.emitStage('native-connection-created');}catch(e){clearTimeout(timeout);reject(e);}});
    if(this.closed){this.conn?.destroy();return;}
    this.emitStage('udp-ready');
    const c=this.conn;
    c.setSelfMute(true);c.setSelfDeafen(this.options.selfDeaf);
    c.setTransportOptions({selfMute:true,inputMode:1,inputModeOptions:{vadThreshold:-40,vadAutoThreshold:3,vadUseKrisp:false,vadLeading:5,vadTrailing:25},fec:true,packetLossRate:.3,qos:true,encodingVoiceBitRate:64000,callBitRate:600000,callMinBitRate:16000,callMaxBitRate:10000000,attenuation:false,minimumJitterBufferLevel:0,postponeDecodeLevel:100});
    this.emitStage('transport-configured');
    if(!this.stream)applyInputMode(c,this.audioSettings);
    c.setOnSpeakingCallback((id,flags)=>{if(this.closed||!this.connected)return;if(id===this.options.userId)this.send(5,{speaking:typeof flags==='boolean'?(flags?1:0):Number(flags)||0,delay:0,ssrc:this.ssrc});this.speaking(id,flags);});
    if(!this.stream)c.setOnVideoCallback?.((id,ssrc,streamId,streams)=>this.onVideo(id,ssrc,streams));
    c.setOnConnectionFailedCallback?.(()=>this.fail(new MediaError('UDP_FAILED','The media transport failed.')));
    c.setOnMLSFailureCallback?.(()=>this.fail(new MediaError('MLS_FAILED','The media engine reported an encryption failure.')));
    c.setSecureFramesStateUpdateCallback?.(()=>{if(!this.closed)this.emitStage('mls-state-updated');});
    c.setOnDesktopSourceEnded?.(()=>{if(this.stream&&this.captureActive&&!this.clearingShareSource){this.suspendedShare=null;this.fail(new MediaError('SOURCE_ENDED','The selected screen or window is no longer available.'));}});
    c.setOnSoundshare?.(attached=>{if(this.stream&&this.soundshareRequested&&attached&&!this.closed){this.soundshareAttached=true;this.emitState('audio-attached');}});
    c.setOnSoundshareFailed?.(()=>{if(this.stream&&this.soundshareRequested&&!this.closed)this.fail(new MediaError('SCREEN_AUDIO_FAILED','The requested screen audio source could not be attached.'));});
    c.setPingInterval?.(5000);
    for(const user of this.members.values())this.merge(user);
    const modes=await callbackPromise(cb=>c.getEncryptionModes(cb));
    this.emitStage('encryption-modes-ready');
    this.mode=chooseEncryption(d.modes,modes||[]);
    this.codecs=[AUDIO];
    if(this.stream||this.streamParameters.length){try{const raw=await callbackPromise(cb=>this.engine.getCodecCapabilities(cb));const available=typeof raw==='string'?JSON.parse(raw):raw;const names=(available||[]).filter(x=>x.encode&&['H264','VP8','VP9'].includes(x.codec||x.name));if(!names.length)throw new MediaError('NO_VIDEO_ENCODER','No supported video encoder was found.');this.codecs.push(...names.map((x,i)=>({type:'video',name:x.codec||x.name,priority:(i+2)*1000,payload_type:101+i*2,rtx_payload_type:102+i*2,encode:true,decode:false})));}catch(error){if(this.stream)throw error;this.streamParameters=[];}}
    this.send(1,{protocol:info.protocol||'udp',data:{address:info.address,port:info.port,mode:this.mode},codecs:this.codecs});
  }
  describe(d) {
    this.emitStage('session-description');
    const c=this.needConnection();
    if(d.mode!==this.mode||!Array.isArray(d.secret_key)||d.secret_key.length!==32||!d.secret_key.every(n=>Number.isInteger(n)&&n>=0&&n<256))throw new MediaError('INVALID_SESSION','The voice server returned an invalid encryption session.');
    if(d.dave_protocol_version!==1)throw new MediaError('DAVE_REQUIRED','This client requires DAVE v1 for calls. Unencrypted downgrade is refused.');
    if(d.audio_codec&&d.audio_codec!=='opus')throw new MediaError('UNSUPPORTED_CODEC','The selected audio codec is unsupported.');
    c.setTransportOptions({encryptionSettings:{mode:d.mode,secretKey:d.secret_key},audioEncoder:{type:120,name:'opus',freq:48000,pacsize:960,channels:this.stream?2:1,rate:64000},audioDecoders:[{type:120,name:'opus',freq:48000,channels:2,params:{stereo:'1'}}]});
    if(this.stream||d.video_codec){const codec=this.codecs.find(x=>x.type==='video'&&x.name===d.video_codec);if(!codec&&this.stream)throw new MediaError('UNSUPPORTED_CODEC','The selected screen sharing codec is unsupported.');if(codec){this.videoCodec=codec;const params=codec.name==='H264'?{'level-asymmetry-allowed':'1','packetization-mode':'1','profile-level-id':'4d0033'}:{};c.setTransportOptions({videoEncoder:{name:codec.name,type:codec.payload_type,rtxType:codec.rtx_payload_type,params},videoDecoders:[]});}}
    this.sessionDescription=true;this.prepareEpoch('1',d.dave_protocol_version);this.localReady=true;this.clearTimeout(this.connectTimeout);this.updateReadiness();
  }
  prepareEpoch(epoch,version){if(version!==1)throw new MediaError('DAVE_REQUIRED','An unsupported encryption transition was refused.');this.emitStage('mls-epoch');if(String(epoch)==='1'){++this.epochGeneration;this.daveReady=this.connected=false;this.pendingTransitions.clear();this.conn?.setSelfMute(true);if(this.cameraRequested)this.stopCamera();this.pauseShareForRekey();}this.needConnection().prepareSecureFramesEpoch(String(epoch),version,this.options.channelId);if(String(epoch)==='1'){const generation=this.epochGeneration;this.emitStage('mls-key-package-before');this.conn.getMLSKeyPackage(data=>{if(!this.closed&&generation===this.epochGeneration){this.emitStage('mls-key-package-created');this.sendBinary(26,data);}});this.updateReadiness();}}
  prepareTransition(d){if(d.protocol_version!==1)throw new MediaError('DAVE_REQUIRED','An unencrypted protocol transition was refused.');const id=d.transition_id,generation=this.epochGeneration;if(!Number.isInteger(id)||id<0||id>65535)throw new MediaError('INVALID_TRANSITION','Invalid encryption transition.');this.needConnection().prepareSecureFramesTransition(id,d.protocol_version,()=>{if(this.closed||generation!==this.epochGeneration)return;this.pendingTransitions.add(id);if(id===0)this.executeTransition(id);else this.send(23,{transition_id:id});});}
  executeTransition(id){if(!this.pendingTransitions.delete(id))throw new MediaError('INVALID_TRANSITION','An unprepared encryption transition was refused.');this.needConnection().executeSecureFramesTransition(id);this.daveReady=true;this.checkConnected();}
  updateReadiness(){
    if(this.closed||!this.localReady||this.connected)return;
    // A sole member has a local MLS group but cannot receive an established
    // group's Commit/Welcome until another participant joins. Keep send muted;
    // expose only local input/VAD and audio controls while that is legitimate.
    this.conn.setSelfMute(true);
    if(!this.stream)this.startLocalVad();
    if(this.members.size>0){if(!this.daveTimeout)this.daveTimeout=this.setTimeout(()=>this.fail(new MediaError('VOICE_ENCRYPTION_TIMEOUT','The encrypted participant group did not finish in time.')),30000);this.emitState('negotiating');}
    else{this.clearTimeout(this.daveTimeout);this.daveTimeout=null;this.emitState('waiting-peer');}
  }
  startLocalVad(){if(this.localVad||this.stream)return;this.engine.setInputDevice(this.audioSettings.inputId);this.localVad=true;this.engine.setOnVoiceCallback?.((level,active)=>{if(!this.localVad||this.closed||!this.localReady||this.connected)return;this.speaking(this.options.userId,!!active&&!this.options.selfMute&&!this.options.selfDeaf&&(!this.audioSettings.pushToTalk||this.pttActive));});this.engine.setEmitVADLevel2?.(true);}
  stopLocalVad(){if(!this.localVad)return;this.localVad=false;this.engine.setEmitVADLevel2?.(false);this.speaking(this.options.userId,false);}
  checkConnected(){if(this.closed||this.connected||!this.sessionDescription||!this.daveReady)return;this.localReady=true;if(!this.stream)this.engine.setInputDevice(this.audioSettings.inputId);this.connected=true;this.clearTimeout(this.connectTimeout);this.clearTimeout(this.daveTimeout);this.daveTimeout=null;this.stopLocalVad();this.conn.setSelfMute(this.stream||this.options.selfMute||this.options.selfDeaf);this.emitStage('connected');this.emitState('connected');this.emit('connected');const resume=this.suspendedShare;this.suspendedShare=null;if(resume&&!this.closed){try{this.share(resume.sourceId,resume.quality,resume.audio);this.emitState('capture-requested');}catch(error){this.fail(error);}}}
  addUser(id,ssrc){if(typeof id!=='string'||!/^\d{1,20}$/.test(id)||id===this.options.userId)return;const previous=this.members.get(id);const user={id,ssrc:Number(ssrc)||previous?.ssrc||0,videoSsrc:0,videoSsrcs:[],rtxSsrc:0,mute:false,volume:this.localVolumes.get(id)??previous?.volume??1};this.members.set(id,user);this.merge(user);if(!this.stream&&!previous)this.emit('participant',{userId:id,present:true});this.updateReadiness();}
  speaking(id,flags){if(this.stream||typeof id!=='string'||!/^\d{1,20}$/.test(id)||(!this.speakingStates.has(id)&&this.speakingStates.size>=256))return;const active=!!flags;if(this.speakingStates.get(id)===active)return;this.speakingStates.set(id,active);this.emit('speaking',{userId:id,self:id===this.options.userId,active});}
  onVideo(id,ssrc,streams){
    if(this.closed)return;
    if(id!==this.options.userId){if(!this.stream)this.emit('participant',{userId:id,present:true,video:Number(ssrc)>0});return;}
    const active=Number(ssrc)>0&&(this.stream||this.cameraRequested);
    // A camera may disappear after its initial activation promise has completed.
    // Stop first: stopCamera clears the flags before calling the addon, so its
    // synchronous zero-SSRC callback cannot re-enter this loss branch.
    if(!this.stream&&this.cameraRequested&&this.cameraActive&&!active){
      this.stopCamera();
      this.emit('camera',{state:'error',reason:{code:'CAMERA_SOURCE_ENDED',message:'The camera stopped delivering video. Select it again to reconnect.'}});
      return;
    }
    const updated=this.streamParameters.map((p,i)=>{const actual=Array.isArray(streams)?streams.find(s=>s.rid===p.rid):null;return {...p,ssrc:actual?.ssrc??(i===0&&active?ssrc:p.ssrc),rtxSsrc:actual?.rtxSsrc??p.rtxSsrc,active:active&&(actual?!!actual.active:i===0)};});
    this.streamParameters=updated;
    this.send(12,{audio_ssrc:this.ssrc,video_ssrc:active?ssrc:0,rtx_ssrc:active?(updated[0]?.rtxSsrc||0):0,streams:updated.map(s=>({type:s.type,rid:s.rid,ssrc:s.ssrc,rtx_ssrc:s.rtxSsrc,quality:s.quality,active:s.active}))});
    if(!this.stream){this.cameraActive=active;this.emit('camera',{state:active?'active':this.cameraRequested?'preparing':'stopped'});if(active)this.cameraResolve?.();}
  }
  async startCamera(deviceId){
    if(this.stream||!this.connected||!this.videoCodec||!this.streamParameters?.length)throw new MediaError('CAMERA_UNAVAILABLE','This encrypted voice session did not negotiate camera video. Rejoin the call.');
    if(this.cameraRequested)throw new MediaError('CAMERA_BUSY','The camera is already active or preparing.');
    this.cameraRequested=true;this.emit('camera',{state:'preparing'});
    try{
      await new Promise((resolve,reject)=>{
        const timer=setTimeout(()=>reject(new MediaError('CAMERA_TIMEOUT','The camera did not begin delivering native video in time.')),10000);
        this.cameraResolve=()=>{clearTimeout(timer);resolve();};this.cameraReject=error=>{clearTimeout(timer);reject(error);};
        const streams=this.streamParameters.map((s,i)=>({...s,active:i===0,maxResolution:{type:'fixed',width:1280,height:720},maxFrameRate:30,maxBitrate:2500000}));
        try{this.conn.setTransportOptions({streamParameters:streams,encodingVideoWidth:1280,encodingVideoHeight:720,encodingVideoFrameRate:30,captureVideoFrameRate:30,encodingVideoBitRate:600000,encodingVideoMinBitRate:200000,encodingVideoMaxBitRate:2500000});this.engine.setVideoInputDevice(deviceId);this.conn.setVideoBroadcast(true);}catch(error){this.cameraReject(error);}
      });
      return {state:'active',verifiedReceiver:false,preview:false};
    }catch(error){this.stopCamera();this.emit('camera',{state:'error',reason:safeError(error)});throw error;}
    finally{this.cameraResolve=null;this.cameraReject=null;}
  }
  stopCamera(){if(this.stream)return {state:'stopped'};const wasRequested=this.cameraRequested;this.cameraRequested=this.cameraActive=false;this.cameraReject?.(new MediaError('CAMERA_STOPPED','Camera stopped.'));this.cameraReject=null;this.cameraResolve=null;try{if(wasRequested)this.conn?.setVideoBroadcast(false);}finally{if(wasRequested)this.engine?.setVideoInputDevice('disabled');}if(wasRequested){this.onVideo(this.options.userId,0,[]);this.emit('camera',{state:'stopped'});}return {state:'stopped'};}
  merge(user){this.conn?.mergeUsers([user]);}
  setUserVolume(id,scale){this.localVolumes.set(id,scale);const user=this.members.get(id);if(user)user.volume=scale;this.needConnection().setLocalVolume(id,scale);}
  mute(value){this.options.selfMute=!!value;this.conn?.setSelfMute(this.stream||!this.connected||!!value||this.options.selfDeaf);if(value&&!this.connected)this.speaking(this.options.userId,false);return {muted:!!value};}
  ptt(value){this.pttActive=!!value;if(this.conn&&this.localReady)this.conn.setPTTActive?.(this.pttActive,false,false);if(!this.pttActive&&!this.connected)this.speaking(this.options.userId,false);return {active:this.pttActive};}
  deafen(value){this.options.selfDeaf=!!value;this.conn?.setSelfDeafen(!!value);if(value&&!this.connected)this.speaking(this.options.userId,false);return {deafened:!!value};}
  share(sourceId,quality={},audio={}) {
    if(!this.stream||!this.connected)throw new MediaError('SHARE_NOT_READY','The separate Go Live media session is not encrypted and connected yet.');
    if(typeof sourceId!=='string'||! /^(screen|screen-handle|window):[^:\r\n]+(?::[^:\r\n]+)?$/.test(sourceId))throw new MediaError('INVALID_SOURCE','Choose a source returned by listSources.');
    const [type,id]=sourceId.split(':');const height=Number(quality.height)||720,fps=Number(quality.fps)||30;
    if(![480,720,1080,1440].includes(height)||![15,30,60].includes(fps))throw new MediaError('INVALID_QUALITY','Unsupported screen sharing quality.');
    const width=Math.round(height*16/9),bitrate=height>720||fps>30?9000000:3500000;
    this.shareQuality={width,height,fps};
    if(audio.enabled){const loopback=audio.loopback===true;const pid=Number(audio.pid);if(!Number.isSafeInteger(pid)||pid<0||pid>0xffffffff||(!loopback&&pid===0))throw new MediaError('INVALID_AUDIO_SOURCE','Window audio requires a valid source process; system audio must be explicitly selected.');this.soundshareRequested=true;this.conn.setTransportOptions({soundsharePid:pid,soundshareEventDriven:true,soundshareLoopback:loopback,encodingVoiceBitRate:128000});}
    const streams=this.streamParameters.map((s,i)=>({...s,active:i===0,maxResolution:{type:'fixed',width,height},maxFrameRate:fps,maxBitrate:bitrate}));
    this.conn.setTransportOptions({streamParameters:streams,encodingVideoWidth:width,encodingVideoHeight:height,encodingVideoFrameRate:fps,captureVideoFrameRate:fps,encodingVideoBitRate:600000,encodingVideoMinBitRate:500000,encodingVideoMaxBitRate:bitrate,encodingVideoDegradationPreference:this.engine.DegradationPreference.MAINTAIN_FRAMERATE,remoteSinkWantsMaxFramerate:fps,remoteSinkWantsPixelCount:width*height});
    this.captureActive=true;this.conn.setDesktopSourceWithOptions({type,sourceId:id,useVideoHook:false,useGraphicsCapture:true,useGraphicsCaptureApiLevel:0,useCaptureDeviceForEncode:true});
    this.conn.setVideoBroadcast(true);
    this.send(12,{audio_ssrc:this.ssrc,video_ssrc:streams[0].ssrc,rtx_ssrc:streams[0].rtxSsrc,streams:streams.map(s=>({type:s.type,rid:s.rid,ssrc:s.ssrc,rtx_ssrc:s.rtxSsrc,quality:s.quality,active:s.active,max_bitrate:s.maxBitrate,max_framerate:s.maxFrameRate,max_resolution:s.maxResolution}))});
    this.shareSelection={sourceId,quality:{height,fps},audio:audio.enabled?{enabled:true,loopback:audio.loopback===true,pid:Number(audio.pid)}:{enabled:false}};
    return {state:'capture-requested',quality:this.shareQuality,verifiedLive:false};
  }
  clearShareCapture(){this.captureActive=this.soundshareRequested=this.soundshareAttached=false;this.clearingShareSource=true;try{try{this.conn?.setVideoBroadcast(false);this.conn?.setTransportOptions({soundsharePid:0,soundshareEventDriven:true,soundshareLoopback:false});}finally{this.conn?.clearDesktopSource();}}finally{this.clearingShareSource=false;}}
  pauseShareForRekey(){if(!this.stream||!this.shareSelection)return;this.suspendedShare=this.shareSelection;this.shareSelection=null;this.clearShareCapture();this.emitState('paused');}
  stopShare(){this.shareSelection=this.suspendedShare=null;this.clearShareCapture();}
  fail(error){if(this.closed)return;this.emitState('error',safeError(error));this.close(false);}
  close(notify=true){if(this.closed)return;try{this.stopCamera();}catch{}if(this.stream)try{this.stopShare();}catch{}try{this.stopLocalVad();}catch{}if(!this.stream)try{this.engine?.setInputDevice?.('disabled');}catch{}this.closed=true;this.clearTimeout(this.connectTimeout);this.clearTimeout(this.daveTimeout);clearInterval(this.heartbeat);try{this.socket?.close(1000);}catch{}try{this.conn?.destroy();}catch{}this.conn=null;this.options.voiceToken='';this.options.sessionId='';this.members.clear();this.localVolumes.clear();this.speakingStates.clear();this.connected=this.localReady=false;if(notify)this.emitState('disconnected');}
}
module.exports={VoiceSession};
