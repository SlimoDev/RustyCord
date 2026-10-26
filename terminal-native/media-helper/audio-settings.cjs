'use strict';
const {MediaError,requireString,volumeScale}=require('./protocol.cjs');
const DEFAULTS=Object.freeze({inputId:'default',outputId:'default',inputVolume:100,outputVolume:100,noiseSuppression:true,echoCancellation:true,pushToTalk:false});
function normalizeAudioSettings(p={}){
  const value={...DEFAULTS};
  for(const key of ['inputId','outputId'])if(p[key]!==undefined)value[key]=requireString(p[key],key,512);
  for(const key of ['inputVolume','outputVolume'])if(p[key]!==undefined){volumeScale(p[key]);value[key]=Number(p[key]);}
  if(value.inputVolume>200)throw new MediaError('INVALID_VOLUME','Input volume must not exceed 200%.');
  for(const key of ['noiseSuppression','echoCancellation','pushToTalk'])if(p[key]!==undefined){if(typeof p[key]!=='boolean')throw new MediaError('INVALID_AUDIO_SETTING','Audio switches must be booleans.');value[key]=p[key];}
  return value;
}
function processingOptions(value){return{builtInEchoCancellation:true,echoCancellation:value.echoCancellation,noiseSuppression:value.noiseSuppression,automaticGainControl:true,noiseCancellation:false};}
function configureInitialDevices(engine){
  engine.setInputDevice('disabled');engine.setOutputDevice('default');engine.setVideoInputDevice('disabled');
  engine.setInputVolume(1);engine.setOutputVolume(1);
}
function applyAudioSettings(engine,value,{captureInput=false}={}){
  // Enumeration and settings are not permission to activate a microphone.
  // Only a connected call or an explicit local test can select a real input.
  engine.setInputDevice(captureInput?value.inputId:'disabled');engine.setOutputDevice(value.outputId);
  engine.setInputVolume(volumeScale(value.inputVolume));engine.setOutputVolume(volumeScale(value.outputVolume));
  engine.setTransportOptions(processingOptions(value));
}
function applyInputMode(conn,value){
  conn.setPTTActive?.(false,false,false);
  conn.setTransportOptions({inputMode:value.pushToTalk?2:1,inputModeOptions:value.pushToTalk?{pttReleaseDelay:0}:{vadThreshold:-40,vadAutoThreshold:3,vadUseKrisp:false,vadLeading:5,vadTrailing:25}});
}
class LocalMicTest{
  constructor(engineFactory,emit,{maxDurationMs=30000}={}){this.factory=engineFactory;this.emit=emit;this.maxDurationMs=maxDurationMs;this.active=false;}
  start(settings){
    this.stop();const engine=this.engine=this.factory();this.lastLevel=0;this.active=true;
    try{
      applyAudioSettings(engine,settings,{captureInput:true});
      engine.setOnVoiceCallback((level,speaking)=>{
        if(!this.active||Date.now()-this.lastLevel<75)return;this.lastLevel=Date.now();
        const numeric=Number(level);const normalized=Number.isFinite(numeric)?Math.max(0,Math.min(1,numeric<0?Math.pow(10,numeric/20):numeric)):0;
        this.emit({active:true,level:normalized,speaking:!!speaking});
      });
      engine.setLoopback(true,processingOptions(settings));engine.setEmitVADLevel2?.(true);
      this.timer=setTimeout(()=>this.stop(),this.maxDurationMs);this.emit({active:true,level:0,speaking:false});
      return{active:true,localOnly:true,maximumSeconds:this.maxDurationMs/1000};
    }catch(error){this.stop();throw error;}
  }
  stop(){
    clearTimeout(this.timer);const active=this.active;this.active=false;
    if(this.engine){const engine=this.engine;this.engine=null;try{engine.setLoopback(false,{});}finally{try{engine.setEmitVADLevel2?.(false);}finally{engine.setInputDevice('disabled');}}}
    if(active)this.emit({active:false,level:0,speaking:false});return{active:false,localOnly:true};
  }
}
module.exports={DEFAULTS,normalizeAudioSettings,processingOptions,configureInitialDevices,applyAudioSettings,applyInputMode,LocalMicTest};
