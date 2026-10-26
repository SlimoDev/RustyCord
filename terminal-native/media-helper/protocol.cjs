'use strict';
const MAX_FRAME = 1024 * 1024;
class MediaError extends Error { constructor(code, message) { super(message); this.code = code; } }
function requireString(value, field, max = 2048) {
  if (typeof value !== 'string' || !value.length || value.length > max || /[\r\n\0]/.test(value)) throw new MediaError('INVALID_ARGUMENT', `Invalid ${field}.`);
  return value;
}
function snowflake(value, field) {
  if (typeof value !== 'string' || !/^\d{1,20}$/.test(value)) throw new MediaError('INVALID_ARGUMENT', `Invalid ${field}.`);
  return value;
}
function endpointURL(endpoint) {
  requireString(endpoint, 'endpoint', 256);
  let url;
  try { url = new URL(endpoint.startsWith('wss://') ? endpoint : `wss://${endpoint}`); } catch { throw new MediaError('INVALID_ENDPOINT', 'Invalid voice endpoint.'); }
  if (url.protocol !== 'wss:' || url.username || url.password || !/^[a-z0-9.-]+\.(discord\.media|discord\.gg|discordapp\.com)$/i.test(url.hostname) || !['', '/'].includes(url.pathname) || url.search || url.hash) throw new MediaError('INVALID_ENDPOINT', 'The endpoint is not an allowed Discord voice host.');
  url.search = '?v=8'; return url.href;
}
function credentials(p, stream = false) {
  const serverId=snowflake(stream?p.rtcServerId:(p.guildId||p.channelId),'serverId');
  const originalChannelId=snowflake(p.channelId,'channelId');
  // StreamRTCConnection uses its RTC channel, not the guild voice channel, for Identify and MLS.
  const channelId=stream?snowflake(p.streamChannelId||(BigInt(serverId)-1n).toString(),'streamChannelId'):originalChannelId;
  return {userId:snowflake(p.userId,'userId'),channelId,originalChannelId,serverId,sessionId:requireString(p.sessionId,'sessionId',512),voiceToken:requireString(p.voiceToken,'voiceToken'),endpoint:endpointURL(p.endpoint),selfMute:Boolean(p.selfMute),selfDeaf:Boolean(p.selfDeaf),context:stream?'stream':'default'};
}
function parseVoiceFrame(data) {
  if (typeof data === 'string') {
    if (Buffer.byteLength(data)>MAX_FRAME) throw new MediaError('OVERSIZE_FRAME','Voice frame too large.');
    const p=JSON.parse(data);
    if (!Number.isInteger(p.op)) throw new MediaError('INVALID_FRAME','Invalid voice opcode.');
    return {op:p.op,d:p.d,seq:p.seq};
  }
  const b=Buffer.from(data);
  if (b.length<3||b.length>MAX_FRAME) throw new MediaError('INVALID_FRAME','Invalid binary voice frame.');
  return {seq:b.readUInt16BE(0),op:b[2],d:b.subarray(3)};
}
function clientBinary(op,payload) {
  const b=Buffer.from(payload);
  if(b.length>MAX_FRAME-1) throw new MediaError('OVERSIZE_FRAME','Voice frame too large.');
  return Buffer.concat([Buffer.from([op]),b]);
}
function transitionPayload(payload) {
  if(!Buffer.isBuffer(payload)||payload.length<3) throw new MediaError('INVALID_FRAME','Invalid MLS transition.');
  return {id:payload.readUInt16BE(0),data:payload.subarray(2)};
}
function chooseEncryption(server,native) {
  const mode=['aead_aes256_gcm_rtpsize','aead_xchacha20_poly1305_rtpsize'].find(x=>server.includes(x)&&native.includes(x));
  if(!mode) throw new MediaError('UNSUPPORTED_ENCRYPTION','No supported authenticated transport encryption mode.');
  return mode;
}
function volumeScale(value) {
  if(!Number.isFinite(value)||value<0||value>400)throw new MediaError('INVALID_VOLUME','Volume must be from 0 to 400.');
  return value/100;
}
function safeError(error) { return error instanceof MediaError?{code:error.code,message:error.message}:{code:'MEDIA_FAILED',message:'The media operation failed. No credentials were logged.'}; }
module.exports={MediaError,MAX_FRAME,requireString,snowflake,endpointURL,credentials,parseVoiceFrame,clientBinary,transitionPayload,chooseEncryption,volumeScale,safeError};
