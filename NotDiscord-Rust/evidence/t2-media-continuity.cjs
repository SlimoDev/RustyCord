'use strict';
// Account-free N-API fakes only: never import discord_voice, Electron or run().
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
if (process.argv.length !== 4) throw new Error('Pass the freshly built diagnostic .node and output JSON paths.');
delete process.env.NOTDISCORD_VIDEO_PIPE;
delete process.env.NOTDISCORD_VIDEO_NAMESPACE;
const addon = require(path.resolve(process.argv[2]));
const report = addon.probeReceiveLifecycle();
report.DiagnosticAddonSha256 = crypto.createHash('sha256').update(fs.readFileSync(path.resolve(process.argv[2]))).digest('hex');
report.ObservedSourceHashes = Object.fromEntries(['host.rs', 'session.rs', 'receive_probe.rs', 'host/voice_socket.rs'].map(file => [file,
  crypto.createHash('sha256').update(fs.readFileSync(path.resolve(__dirname, '../crates/media-addon/src', file))).digest('hex')]));
fs.writeFileSync(path.resolve(process.argv[3]), JSON.stringify(report, null, 2) + '\n');
if (report.Success !== true || report.CameraScreenMicrophoneOwnershipCycles !== 8
    || report.SeparateInputAndSanitizedNativeOutputTelemetry !== true
    || report.RemoteMicrophoneAudibilityVerified !== false) {
  throw new Error('Synthetic microphone/video continuity contract failed.');
}
console.log(JSON.stringify({ Success: report.Success, Cycles: report.CameraScreenMicrophoneOwnershipCycles,
  NativeEngineLoaded: report.NativeEngineLoaded, PhysicalDevicesOpened: report.PhysicalDevicesOpened,
  NetworkConnections: report.NetworkConnections, RemoteAudibilityVerified: report.RemoteMicrophoneAudibilityVerified }));
