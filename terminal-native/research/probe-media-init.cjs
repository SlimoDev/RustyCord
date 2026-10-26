'use strict';
// Explicit fake audio/video devices. No connections, camera, microphone, or screen capture.
const fs=require('node:fs'),path=require('node:path');
const {nativeEngine}=require('../media-helper/native.cjs');
const dir=path.join(__dirname,'fake-media-state');fs.mkdirSync(dir,{recursive:true});
const m=nativeEngine(),report={utc:new Date().toISOString(),node:process.versions.node,fakeCapture:true,connectionsCreated:0};
const timer=setTimeout(()=>{report.error='codec callback timeout';save(1);},10000);
function save(code){clearTimeout(timer);fs.writeFileSync(path.join(__dirname,'media-init.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));process.exit(code);}
try {
  m.setImageDataAllocator((width,height)=>({width,height,data:new Uint8ClampedArray(width*height*4)}));
  m.initialize({audioSubsystem:'experimental',logLevel:-1,dataDirectory:dir,logDirectory:dir,maxLogBytes:65536,offloadAdmControls:true,asyncVideoInputDeviceInit:true,useFakeAudioCapture:true,useFakeVideoCapture:true});
  report.initialized=true;
  m.getCodecCapabilities(values=>{report.codecs=values;report.rssMiB=process.memoryUsage().rss/1048576;save(0);});
} catch(error){report.error=error.message;save(1);}
