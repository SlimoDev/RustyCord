'use strict';
// All capture is synthetic; the sole UDP destination is our own loopback listener.
// No websocket, credentials, external endpoint, real microphone, camera, or screen.
const fs=require('node:fs'),path=require('node:path');
const mls=process.argv.includes('--mls'),mlsCallback=process.argv.includes('--mls-callback');
if(process.argv.includes('--child')){
  const emit=(stage,details={})=>fs.writeSync(1,JSON.stringify({stage,...details})+'\n');
  const udp=require('node:dgram').createSocket('udp4');
  let conn,finished=false,timer;
  const finish=(success)=>{if(finished)return;finished=true;clearTimeout(timer);try{emit('destroy-before');conn?.destroy();emit('destroy-after');}catch{emit('destroy-exception');}udp.close(()=>{});setTimeout(()=>process.exit(success?0:1),70);};
  try{
    const m=require('../media-helper/native.cjs').nativeEngine();
    const dir=path.join(__dirname,'fake-media-state');fs.mkdirSync(dir,{recursive:true});
    m.setImageDataAllocator((width,height)=>({width,height,data:new Uint8ClampedArray(width*height*4)}));
    emit('initialize-before');
    m.initialize({audioSubsystem:'experimental',logLevel:-1,dataDirectory:dir,logDirectory:dir,maxLogBytes:65536,offloadAdmControls:true,asyncVideoInputDeviceInit:true,useFakeAudioCapture:true,useFakeVideoCapture:true});
    m.setTransportOptions({builtInEchoCancellation:true,echoCancellation:true,noiseSuppression:true,automaticGainControl:true,noiseCancellation:false,idleJitterBufferFlush:true,ducking:false});
    m.setInputDevice('default');m.setOutputDevice('default');m.setVideoInputDevice('disabled');m.setInputVolume(1);m.setOutputVolume(1);
    emit('initialize-after');
    udp.on('error',()=>{emit('loopback-socket-error');finish(false);});
    udp.on('message',(data,peer)=>{
      if(peer.address!=='127.0.0.1')return;
      // Minimal IP discovery response. Nothing is forwarded beyond loopback.
      if(data.length===74&&data.readUInt16BE(0)===1&&data.readUInt16BE(2)===70){
        const response=Buffer.alloc(74);response.writeUInt16BE(2,0);response.writeUInt16BE(70,2);data.copy(response,4,4,8);response.write('127.0.0.1',8,'ascii');response.writeUInt16BE(peer.port,72);
        emit('loopback-discovery');udp.send(response,peer.port,'127.0.0.1');
      }else emit('loopback-other-packet',{bytes:data.length});
    });
    udp.bind(0,'127.0.0.1',()=>{
      timer=setTimeout(()=>{emit('timeout');finish(false);},2500);
      const options={ssrc:12345,address:'127.0.0.1',port:udp.address().port,context:'default',experiments:[],qosEnabled:true,streamParameters:[{type:'audio',ssrc:12345,rid:'',maxBitrate:64000,soundshare:false}]};
      emit('native-connection-before');
      try{
        conn=new m.VoiceConnection('100000000000000001',options,(error,info)=>{
          emit('native-callback',{success:!error&&!!info});
          if(error||!info)return finish(false);
          setImmediate(()=>{
            try{
              emit('connection-setters-before');
              conn.setSelfMute(true);conn.setSelfDeafen(true);
              conn.setTransportOptions({selfMute:true,inputMode:1,inputModeOptions:{vadThreshold:-40,vadAutoThreshold:3,vadUseKrisp:false,vadLeading:5,vadTrailing:25},fec:true,packetLossRate:.3,qos:true,encodingVoiceBitRate:64000,callBitRate:600000,callMinBitRate:16000,callMaxBitRate:10000000,attenuation:false,minimumJitterBufferLevel:0,postponeDecodeLevel:100});
              conn.setOnSpeakingCallback(()=>{});conn.setOnConnectionFailedCallback(()=>{});conn.setOnMLSFailureCallback(()=>{});conn.setOnDesktopSourceEnded(()=>{});conn.setOnSoundshare(()=>{});conn.setOnSoundshareFailed(()=>{});conn.setPingInterval(5000);
              emit('connection-setters-after');
              conn.getEncryptionModes(modes=>{
                emit('encryption-modes',{array:Array.isArray(modes),count:Array.isArray(modes)?modes.length:null});
                if(!mls)return finish(true);
                try{
                  if(mlsCallback){emit('mls-state-callback-before');conn.setSecureFramesStateUpdateCallback(()=>emit('mls-state-callback'));emit('mls-state-callback-after');}
                  emit('mls-epoch-before');conn.prepareSecureFramesEpoch('1',1,'100000000000000002');emit('mls-epoch-after');
                  emit('mls-key-package-before');conn.getMLSKeyPackage(data=>{emit('mls-key-package-callback',{success:!!data?.byteLength,bytes:data?.byteLength??0});setTimeout(()=>finish(true),150);});emit('mls-key-package-requested');
                }catch(e){emit('mls-exception',{type:e?.name==='TypeError'?'TypeError':'Exception'});finish(false);}
              });
            }catch(e){emit('connection-setters-exception',{type:e?.name==='TypeError'?'TypeError':'Exception'});finish(false);}
          });
        });
        emit('native-connection-created');
      }catch(e){emit('constructor-exception',{type:e?.name==='TypeError'?'TypeError':'Exception'});finish(false);}
    });
  }catch(e){emit('initialize-exception',{type:e?.name==='TypeError'?'TypeError':'Exception'});process.exit(1);}
}else{
  const {spawnSync}=require('node:child_process');
  const result=spawnSync(process.execPath,[__filename,'--child',...(mls?['--mls']:[]),...(mlsCallback?['--mls-callback']:[])],{encoding:'utf8',windowsHide:true,timeout:4000,maxBuffer:1024*1024});
  const stages=[];let unknownStdoutLines=0,unknownStdoutBytes=0;
  for(const line of (result.stdout||'').split(/\r?\n/)){if(!line)continue;try{const item=JSON.parse(line);if(typeof item.stage==='string'&&/^[a-z-]+$/.test(item.stage)){stages.push(item);continue;}}catch{}unknownStdoutLines++;unknownStdoutBytes+=Buffer.byteLength(line);}
  const fatalSummary=(result.stderr||'').split(/\r?\n/).filter(x=>/^FATAL ERROR:|^Assertion failed:|^# Fatal error|^# Check failed|^# Assertion/.test(x)).map(x=>x.replace(/[A-Z]:\\[^\s]+/gi,'<path>').replace(/0x[0-9a-f]+/gi,'<address>').slice(0,400));
  const report={utc:new Date().toISOString(),fakeCapture:true,loopbackOnly:true,mls,mlsCallback,connectionsCreated:stages.some(s=>s.stage==='native-connection-created')?1:0,exitCode:result.status,signal:result.signal,stages,unknownStdoutLines,unknownStdoutBytes,stderrBytes:Buffer.byteLength(result.stderr||''),fatalSummary,timedOut:result.error?.code==='ETIMEDOUT'};
  const filename=mls?mlsCallback?'voice-mls-callback-loopback.json':'voice-mls-loopback.json':'voice-connection-loopback.json';
  if(mls&&result.status!==0)fs.writeFileSync(path.join(__dirname,'voice-mls-synthetic-stderr.txt'),result.stderr||'');
  fs.writeFileSync(path.join(__dirname,filename),JSON.stringify(report,null,2));process.stdout.write(JSON.stringify(report,null,2)+'\n');process.exitCode=result.status===0?0:1;
}
