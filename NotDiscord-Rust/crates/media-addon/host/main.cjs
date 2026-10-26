'use strict';
// Declarative ABI bootstrap only. IPC, protocol, timers, state and settings are Rust.
const path = require('node:path');
const { app } = require('electron');
const root = process.env.NOTDISCORD_NATIVE_ROOT;
const addon = process.env.NOTDISCORD_RUST_ADDON;
const pipe = process.env.NOTDISCORD_MEDIA_PIPE;
const data = process.env.NOTDISCORD_MEDIA_DATA;
delete process.env.NOTDISCORD_MEDIA_PIPE;
delete process.env.NODE_OPTIONS;
if (!root || !addon || !pipe) app.exit(1);
else {
  const moduleDirectory = path.join(root, 'resources/standalone_modules/discord_voice');
  process.env.PATH = `${root};${moduleDirectory};${process.env.PATH || ''}`;
  app.setPath('userData', path.resolve(data || 'data/media-host'));
  app.whenReady().then(() => {
    try {
      const rust = require(addon);
      const engine = require(path.join(moduleDirectory, 'discord_voice.node'));
      if (process.env.NOTDISCORD_MEDIA_PROBE === 'MLS') rust.probeNativeMls(engine, data);
      else if (process.env.NOTDISCORD_MEDIA_PROBE === 'VIDEO') rust.probeNativeVideo(engine, data);
      else if (process.env.NOTDISCORD_MEDIA_PROBE === 'OWNED_SCREEN') {rust.prepareOwnedPreview();rust.run(engine, pipe, data);}
      else rust.run(engine, pipe, data);
    }
    catch { app.exit(1); }
  });
}
