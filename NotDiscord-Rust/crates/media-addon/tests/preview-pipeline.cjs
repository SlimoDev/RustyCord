'use strict';
// Only the Rust diagnostic addon and its N-API fakes. No Electron/native media
// engine, sockets, accounts, physical devices, windows or capture APIs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
if (process.argv.length !== 4) throw new Error('Pass the diagnostic .node and output JSON paths.');
delete process.env.NOTDISCORD_VIDEO_PIPE;
delete process.env.NOTDISCORD_VIDEO_NAMESPACE;
const addonPath = path.resolve(process.argv[2]);
const addon = require(addonPath);
const report = addon.probeReceiveLifecycle();
assert.equal(report.Success, true);
assert.equal(report.NativeEngineLoaded, false);
assert.equal(report.NativeInitialization, false);
assert.equal(report.PhysicalDevicesOpened, false);
assert.equal(report.NetworkConnections, 0);
assert.equal(report.RemoteMicrophoneAudibilityVerified, false);
for (const key of ['PreviewPendingLeaseSurvivesTimeoutAndSelection',
  'PreviewLateCallbackReleasesOnlyItsQuery', 'PreviewLateFramesDroppedCurrentRgbaVerified']) {
  assert.equal(report[key], true, key);
}
report.DiagnosticAddonSha256 = crypto.createHash('sha256').update(fs.readFileSync(addonPath)).digest('hex');
report.ObservedSourceHashes = Object.fromEntries(['preview.rs', 'video.rs', 'receive_probe.rs'].map(file => [file,
  crypto.createHash('sha256').update(fs.readFileSync(path.resolve(__dirname, '../src', file))).digest('hex')]));
fs.writeFileSync(path.resolve(process.argv[3]), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({Success: true, PreviewLeaseVerified: true, NativeEngineLoaded: false,
  PhysicalDevicesOpened: false, NetworkConnections: 0, RemoteAudibilityVerified: false}));
