// Read-only load probe. No initialize(), devices, capture, profiles, or network.
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const root = path.resolve(__dirname, '../../native-candidate/runtime');
const addon = path.join(root, 'resources/standalone_modules/discord_voice/discord_voice.node');
const report = {
  generatedAt: new Date().toISOString(),
  purpose: 'Load native media exports with stock Node, without initializing or connecting',
  node: process.versions.node,
  napi: process.versions.napi,
  pid: process.pid,
  addonPath: addon,
  addonSha256: crypto.createHash('sha256').update(fs.readFileSync(addon)).digest('hex'),
  before: process.memoryUsage(),
};
try {
  const media = require(addon);
  report.afterLoad = process.memoryUsage();
  report.exports = Object.keys(media);
  report.connectionPrototype = Object.getOwnPropertyNames(media.VoiceConnection.prototype);
  report.supportedSecureFramesProtocolVersion = media.SupportedSecureFramesProtocolVersion;
  report.initialized = false;
  report.networkUsed = false;
  report.profileAccess = false;
  report.limitations = [
    'Loading exports does not establish initialization, device, live transport, DAVE, or capture parity.',
    'RSS is one process snapshot, not private working set, sustained CPU, or application comparison.',
    'No methods on the native engine or VoiceConnection instances were invoked.',
    'Runtime ffmpeg.dll and sibling mediapipe.dll must be discoverable by the Windows loader.',
  ];
  fs.writeFileSync(path.join(__dirname, 'voice-addon-load.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ok:true,exports:report.exports.length,connectionMethods:report.connectionPrototype.length,supportedSecureFramesProtocolVersion:report.supportedSecureFramesProtocolVersion,rssBeforeMiB:report.before.rss/1048576,rssAfterMiB:report.afterLoad.rss/1048576}));
  process.exit(0);
} catch (error) {
  report.error = error.message;
  fs.writeFileSync(path.join(__dirname, 'voice-addon-load.json'), JSON.stringify(report, null, 2));
  console.error(JSON.stringify({ok:false,error:error.message}));
  process.exit(1);
}
