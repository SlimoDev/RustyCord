'use strict';
// Strict field projection: never return native frames, addresses, IDs, labels or error strings.
const {performance}=require('node:perf_hooks');
const {callbackPromise}=require('./native.cjs');
const {MediaError}=require('./protocol.cjs');
const previous=new WeakMap();
const codecNames=new Map(['opus','H264','H265','VP8','VP9','AV1','AV1X','rtx','red'].map(x=>[x.toLowerCase(),x]));
const number=(value,max=Number.MAX_SAFE_INTEGER)=>typeof value==='number'&&Number.isFinite(value)&&value>=0&&value<=max?value:null;
const bool=value=>typeof value==='boolean'?value:null;
const codec=value=>typeof value==='string'?(codecNames.get(value.toLowerCase())||null):null;
const array=value=>Array.isArray(value)?value:[];
const object=value=>value&&typeof value==='object'&&!Array.isArray(value)?value:null;
function sum(values){const finite=values.filter(v=>v!==null);return finite.length?number(finite.reduce((a,b)=>a+b,0)):null;}
function pair(a,b){return a===null||b===null?null:number(a+b);}
function rtpBytes(stats){if(!object(stats))return null;return sum([stats,stats.fec,stats.retransmitted,stats.transmitted].filter(object).flatMap(x=>[number(x.headerBytes),number(x.payloadBytes),number(x.paddingBytes)]));}
function rtpPackets(stats){if(!object(stats))return null;return sum([stats,stats.fec,stats.retransmitted,stats.transmitted].filter(object).map(x=>number(x.packets)));}
function crypt(stats,sending){return sending?{success:number(stats.encryptSuccessCount),failure:number(stats.encryptFailureCount),missingKey:number(stats.encryptMissingKeyCount),passthrough:number(stats.passthroughCount)}:{success:number(stats.decryptSuccessCount),failure:number(stats.decryptFailureCount),missingKey:number(stats.decryptMissingKeyCount),invalidNonce:number(stats.decryptInvalidNonceCount),passthrough:number(stats.passthroughCount)};}
function audio(stats,sending){
  if(!object(stats))return null;
  return {codec:codec(stats.codecName),bytes:sending?number(stats.bytesSent):number(stats.bytesReceived),packets:sending?number(stats.packetsSent):number(stats.packetsReceived),packetsLost:number(stats.packetsLost),bitrateBps:sending?number(stats.mediaBitrate):null,sampleRateHz:number(stats.currentSampleRate,384000),audioDetected:bool(stats.speaking),framesCaptured:sending?number(stats.framesCaptured):null,framesRendered:sending?number(stats.framesRendered):null,crypto:crypt(stats,sending)};
}
function outboundVideo(stats){
  if(!object(stats))return null;
  const substreams=array(stats.substreams),primary=substreams.find(x=>object(x)&&!x.isRTX&&!x.isFlexFEC)||null;
  return {codec:codec(stats.codecName),width:number(primary?.width,32768),height:number(primary?.height,32768),fpsInput:number(stats.inputFrameRate,1000),fpsEncode:number(stats.encodeFrameRate,1000),framesSent:pair(number(primary?.frameCounts?.keyFrames),number(primary?.frameCounts?.deltaFrames)),framesEncoded:number(stats.framesEncoded),bytesSent:sum(substreams.map(x=>rtpBytes(x?.rtpStats))),packetsSent:sum(substreams.map(x=>rtpPackets(x?.rtpStats))),packetsLost:number(primary?.rtcpStats?.packetsLost),bitrateBps:number(stats.mediaBitrate),targetBitrateBps:number(stats.targetMediaBitrate),cpuLimitedResolution:bool(stats.cpuLimitedResolution),bandwidthLimitedResolution:bool(stats.bwLimitedResolution),crypto:crypt(stats,true)};
}
function inboundVideo(stats){
  if(!object(stats))return null;
  return {codec:codec(stats.codecName),width:number(stats.width,32768),height:number(stats.height,32768),fpsNetwork:number(stats.networkFrameRate,1000),fpsDecode:number(stats.decodeFrameRate,1000),fpsRender:number(stats.renderFrameRate,1000),framesReceived:pair(number(stats.frameCounts?.keyFrames),number(stats.frameCounts?.deltaFrames)),framesDecoded:number(stats.framesDecoded),bytesReceived:rtpBytes(stats.rtpStats),packetsReceived:rtpPackets(stats.rtpStats),packetsLost:number(stats.rtpStats?.packetsLost??stats.rtcpStats?.packetsLost),bitrateBps:number(stats.totalBitrate),crypto:crypt(stats,false)};
}
function sanitizeNativeStats(raw,{state='unknown',daveNegotiated=false}={}){
  const stats=object(raw)||{},transport=object(stats.transport)||{},out=object(stats.outbound)||{};
  const inbound=array(stats.inbound).filter(object);
  const inAudio=inbound.map(v=>audio(v.audio,false)).filter(Boolean);
  const inVideo=inbound.flatMap(v=>Array.isArray(v.videos)?v.videos:[v.video]).map(inboundVideo).filter(Boolean);
  const outVideo=(Array.isArray(out.videos)?out.videos:[out.video]).map(outboundVideo).filter(Boolean);
  const version=number(transport.secureFramesProtocolVersion,65535);
  return {
    sanitized:true,available:true,state:['connecting','connected','disconnected','negotiating','waiting-peer','unknown'].includes(state)?state:'unknown',
    encryption:{negotiatedByHelper:!!daveNegotiated,nativeDaveProtocolVersion:version,nativeDaveEnabled:version===null?null:version>0},
    transport:{bytesSent:number(transport.bytesSent),bytesReceived:number(transport.bytesReceived),packetsSent:number(transport.packetsSent),packetsReceived:number(transport.packetsReceived),rttMs:number(transport.rtt),sendBandwidthBps:number(transport.sendBandwidth),outboundEstimateBps:number(transport.outboundBitrateEstimate),inboundEstimateBps:number(transport.inboundBitrateEstimate),decryptionFailures:number(transport.decryptionFailures)},
    outbound:{audio:audio(out.audio,true),video:outVideo},inbound:{audio:inAudio,video:inVideo},
    interval:{durationMs:null,sentBitrateBps:null,receivedBitrateBps:null,sentPacketsPerSecond:null,receivedPacketsPerSecond:null},
  };
}
function stateOf(session){return session.closed?'disconnected':session.connected?'connected':session.localReady&&session.members?.size===0?'waiting-peer':session.conn?'negotiating':'connecting';}
function rate(current,before,seconds,factor=1){return current===null||before===null||current<before?null:number((current-before)*factor/seconds);}
async function sessionStats(session){
  if(!session)return null;
  const conn=session.conn;
  const unavailable=reason=>({sanitized:true,available:false,state:stateOf(session),reason,transport:null,encryption:null,outbound:null,inbound:null,interval:null});
  if(!conn||session.closed)return unavailable('NO_NATIVE_CONNECTION');
  try{
    const raw=await callbackPromise(cb=>{if(typeof conn.getFilteredStats==='function')conn.getFilteredStats(7,cb);else if(typeof conn.getStats==='function')conn.getStats(cb);else throw new MediaError('STATS_UNAVAILABLE','The native connection does not expose counters.');},2000);
    if(session.closed||session.conn!==conn)return unavailable('CONNECTION_CHANGED');
    if(typeof raw==='string'&&Buffer.byteLength(raw)>1048576)return unavailable('NATIVE_STATS_TOO_LARGE');
    const value=typeof raw==='string'?JSON.parse(raw):raw;
    if(!object(value))return unavailable('INVALID_NATIVE_STATS');
    const report=sanitizeNativeStats(value,{state:stateOf(session),daveNegotiated:session.daveReady});
    const now=performance.now(),last=previous.get(session);
    if(last){const seconds=(now-last.at)/1000;if(seconds>=.1&&seconds<=300)report.interval={durationMs:Math.round(seconds*1000),sentBitrateBps:rate(report.transport.bytesSent,last.transport.bytesSent,seconds,8),receivedBitrateBps:rate(report.transport.bytesReceived,last.transport.bytesReceived,seconds,8),sentPacketsPerSecond:rate(report.transport.packetsSent,last.transport.packetsSent,seconds),receivedPacketsPerSecond:rate(report.transport.packetsReceived,last.transport.packetsReceived,seconds)};}
    // Retain only four sanitized counters; never retain the native JSON.
    previous.set(session,{at:now,transport:{bytesSent:report.transport.bytesSent,bytesReceived:report.transport.bytesReceived,packetsSent:report.transport.packetsSent,packetsReceived:report.transport.packetsReceived}});
    return report;
  }catch{return unavailable('NATIVE_STATS_FAILED');}
}
async function readStats(voice,share){const [voiceReport,shareReport]=await Promise.all([sessionStats(voice),sessionStats(share)]);return {sanitized:true,sampledAtUtc:new Date().toISOString(),voice:voiceReport,share:shareReport};}
module.exports={sanitizeNativeStats,readStats};
