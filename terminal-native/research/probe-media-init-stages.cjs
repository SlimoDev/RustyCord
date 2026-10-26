'use strict';
// Synthetic capture only: no VoiceConnection, network, microphone, camera, or screen source.
const fs=require('node:fs'),path=require('node:path');
if(process.argv.includes('--child')){
  const stage=(name,phase)=>fs.writeSync(1,JSON.stringify({stage:name,phase})+'\n');
  const run=(name,fn)=>{stage(name,'before');fn();stage(name,'after');};
  try{
    let m;
    run('load',()=>{m=require('../media-helper/native.cjs').nativeEngine();});
    const dir=path.join(__dirname,'fake-media-state');
    fs.mkdirSync(dir,{recursive:true});
    run('allocator',()=>m.setImageDataAllocator((width,height)=>({width,height,data:new Uint8ClampedArray(width*height*4)})));
    run('initialize-fake',()=>m.initialize({audioSubsystem:'experimental',logLevel:-1,dataDirectory:dir,logDirectory:dir,maxLogBytes:65536,offloadAdmControls:true,asyncVideoInputDeviceInit:true,useFakeAudioCapture:true,useFakeVideoCapture:true}));
    run('transport-options',()=>m.setTransportOptions({builtInEchoCancellation:true,echoCancellation:true,noiseSuppression:true,automaticGainControl:true,noiseCancellation:false,idleJitterBufferFlush:true,ducking:false}));
    run('input-default',()=>m.setInputDevice('default'));
    run('output-default',()=>m.setOutputDevice('default'));
    run('video-disabled',()=>m.setVideoInputDevice('disabled'));
    run('input-volume',()=>m.setInputVolume(1));
    run('output-volume',()=>m.setOutputVolume(1));
    stage('settle','before');
    setTimeout(()=>{stage('settle','after');process.exit(0);},300);
  }catch(e){stage('caught',e?.name==='TypeError'?'TypeError':'Exception');process.exit(1);}
}else{
  const {spawnSync}=require('node:child_process');
  const result=spawnSync(process.execPath,[__filename,'--child'],{encoding:'utf8',windowsHide:true,timeout:15000,maxBuffer:1024*1024});
  const stages=[];let unknownStdoutLines=0,unknownStdoutBytes=0;
  for(const line of (result.stdout||'').split(/\r?\n/)){if(!line)continue;try{const item=JSON.parse(line);if(typeof item.stage==='string'&&typeof item.phase==='string'&&Object.keys(item).length===2){stages.push(item);continue;}}catch{}unknownStdoutLines++;unknownStdoutBytes+=Buffer.byteLength(line);}
  const report={utc:new Date().toISOString(),fakeCapture:true,connectionsCreated:0,exitCode:result.status,signal:result.signal,stages,unknownStdoutLines,unknownStdoutBytes,stderrBytes:Buffer.byteLength(result.stderr||''),timedOut:result.error?.code==='ETIMEDOUT'};
  fs.writeFileSync(path.join(__dirname,'media-init-stages.json'),JSON.stringify(report,null,2));
  process.stdout.write(JSON.stringify(report,null,2)+'\n');
  process.exitCode=result.status===0?0:1;
}
