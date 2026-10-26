'use strict';
const fs=require('node:fs'),path=require('node:path');
const {MediaError}=require('./protocol.cjs');
const {configureInitialDevices}=require('./audio-settings.cjs');
const runtime=path.resolve(__dirname,'../../native-candidate/runtime');
const moduleDirectory=path.join(runtime,'resources/standalone_modules/discord_voice');
let engine,initialized=false,stageListener=null;
function setStageListener(listener){stageListener=typeof listener==='function'?listener:null;}
function stage(name){stageListener?.(name);}
function nativeEngine() {
  if(engine) return engine;
  // Scoped to this helper process; the Windows loader needs both native DLL directories.
  process.env.PATH=`${runtime};${moduleDirectory};${process.env.PATH||''}`;
  try{engine=require(path.join(moduleDirectory,'discord_voice.node'));}
  catch{throw new MediaError('NATIVE_MODULE_UNAVAILABLE','The local Discord voice module or its DLL dependencies could not be loaded.');}
  return engine;
}
function initialize() {
  // This shipped binding creates MLS ArrayBuffers from libuv callbacks. Stock Node
  // lacks the outer HandleScope provided by its matching Electron main-process host.
  if(!process.versions.electron)throw new MediaError('INCOMPATIBLE_MEDIA_HOST','The native media module requires the dedicated Electron main-process host for encrypted voice.');
  const m=nativeEngine();if(initialized)return m;
  const dataDirectory=path.resolve(__dirname,'../data/media');fs.mkdirSync(dataDirectory,{recursive:true});
  m.setImageDataAllocator((width,height)=>({width,height,data:new Uint8ClampedArray(width*height*4)}));
  stage('initialize-before');
  m.initialize({audioSubsystem:'experimental',logLevel:-1,dataDirectory,logDirectory:dataDirectory,maxLogBytes:65536,offloadAdmControls:true,asyncVideoInputDeviceInit:true});
  stage('initialize-after');
  m.setTransportOptions({builtInEchoCancellation:true,echoCancellation:true,noiseSuppression:true,automaticGainControl:true,noiseCancellation:false,idleJitterBufferFlush:true,ducking:false});
  configureInitialDevices(m);
  stage('audio-configured');
  initialized=true;return m;
}
function callbackPromise(call,timeout=8000) {
  return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new MediaError('NATIVE_TIMEOUT','The native media engine did not reply in time.')),timeout);try{call(value=>{clearTimeout(timer);resolve(value);});}catch(error){clearTimeout(timer);reject(error);}});
}
async function listDevices(engineFactory=initialize) {
  const m=engineFactory();
  const compact=(values,type)=>(values||[]).map((d,i)=>({type,id:String(d.guid??d.id??i),name:String(d.name??'Device'),index:Number(d.index??i)}));
  const [input,output]=await Promise.all([callbackPromise(cb=>m.getInputDevices(cb)),callbackPromise(cb=>m.getOutputDevices(cb))]);
  return {input:compact(input,'audioinput'),output:compact(output,'audiooutput')};
}
async function listCameras(engineFactory=initialize) {
  const m=engineFactory();
  const values=await callbackPromise(cb=>m.getVideoInputDevices(cb));
  return (values||[]).map((d,i)=>({id:String(d.guid||d.name||''),name:String(d.name||`Camera ${i+1}`)})).filter(d=>d.id&&d.id!=='disabled');
}
async function listSources() {
  const m=initialize();
  // Enumerate only on an explicit request. Preview image payloads never cross IPC.
  const [screens,windows]=await Promise.all([callbackPromise(cb=>m.getScreenPreviews(1,1,cb)),callbackPromise(cb=>m.getWindowPreviews(1,1,cb))]);
  const compact=(values,type)=>(values||[]).map((d,i)=>({id:String(d.id??''),name:String(d.name??(type==='screen'?`Screen ${i+1}`:'Window')),type})).filter(d=>d.id);
  return [...compact(screens,'screen'),...compact(windows,'window')];
}
function capabilities() {
  const m=nativeEngine(),compatibleHost=!!process.versions.electron;
  return {backend:compatibleHost?'electron-main-napi':'node-napi',electronHost:compatibleHost,chromiumRenderer:false,originalUiLoaded:false,browserWindows:0,voice:compatibleHost,dave:Number(m.SupportedSecureFramesProtocolVersion)||0,screenShare:compatibleHost,screenReceive:false,camera:compatibleHost&&typeof m.getVideoInputDevices==='function'&&typeof m.setVideoInputDevice==='function',cameraPreview:false,verifiedLive:false,reason:compatibleHost?'Camera sending requires explicit device selection and negotiated video. Incoming video and local preview are not implemented.':'Stock Node cannot host this native module’s asynchronous DAVE ArrayBuffer callbacks. Use the dedicated Electron main-process host.'};
}
module.exports={nativeEngine,initialize,callbackPromise,listDevices,listCameras,listSources,capabilities,setStageListener};
