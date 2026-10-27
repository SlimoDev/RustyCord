//! Frozen previews of media actually displayed in a live message this session.
//! This cache only holds existing GPU textures: no network, file access,
//! decoding, animation clocks, readback, or persistent deleted-content store.
use super::*;
use notdiscord_core::{models::ChatMessage, parsing};

const MAX_PREVIEWS: usize = 64;
const PREVIEW_BUDGET: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct MessageScope {
    channel: String,
    message: String,
    deleted: bool,
}
struct Preview {
    channel: String,
    message: String,
    urls: [String; 2],
    texture: TextureHandle,
    bytes: usize,
    used: u64,
}
#[derive(Default)]
pub(super) struct Cache {
    scopes: Vec<Option<MessageScope>>,
    previews: Vec<Preview>,
    bytes: usize,
    clock: u64,
}
impl Cache {
    fn purge(&mut self, channel: &str, message: &str) {
        self.previews.retain(|preview| preview.channel != channel || preview.message != message);
        self.bytes = self.previews.iter().map(|preview| preview.bytes).sum();
    }
    fn begin(&mut self, message: &ChatMessage) {
        self.scopes.push((parsing::valid_id(&message.id) && parsing::valid_id(&message.channel_id))
            .then(|| MessageScope {channel:message.channel_id.clone(),message:message.id.clone(),deleted:message.deleted}));
    }
    fn matches(preview: &Preview, scope: &MessageScope, url: &str, poster: &str) -> bool {
        preview.channel==scope.channel && preview.message==scope.message
            && preview.urls.iter().any(|known| parsing::same_media_resource(known,url)
                || !poster.is_empty() && parsing::same_media_resource(known,poster))
    }
    fn remember(&mut self, url: &str, poster: &str, texture: &TextureHandle) {
        let Some(scope)=self.scopes.last().cloned().flatten().filter(|scope|!scope.deleted) else { return };
        if url.len()>4096 || poster.len()>4096 || parsing::media_identity(url).is_none() { return; }
        self.clock=self.clock.wrapping_add(1);
        if let Some(previous)=self.previews.iter_mut().find(|preview|Self::matches(preview,&scope,url,poster)) {
            // Retain the first actual image/frame shown, not every animated
            // frame. The same texture remains static after the deletion.
            previous.used=self.clock;
            return;
        }
        let size=texture.size();
        let Some(bytes)=size[0].checked_mul(size[1]).and_then(|pixels|pixels.checked_mul(4)) else { return };
        if bytes==0 || bytes>PREVIEW_BUDGET { return; }
        while self.previews.len()>=MAX_PREVIEWS || self.bytes.saturating_add(bytes)>PREVIEW_BUDGET {
            let Some(index)=self.previews.iter().enumerate().min_by_key(|(_,preview)|preview.used).map(|(index,_)|index) else { break };
            let previous=self.previews.swap_remove(index);
            self.bytes=self.bytes.saturating_sub(previous.bytes);
        }
        self.previews.push(Preview {channel:scope.channel,message:scope.message,
            urls:[url.to_owned(),poster.to_owned()],texture:texture.clone(),bytes,used:self.clock});
        self.bytes+=bytes;
    }
    fn frozen(&mut self, url: &str, poster: &str) -> Option<TextureHandle> {
        let scope=self.scopes.last().cloned().flatten().filter(|scope|scope.deleted)?;
        self.clock=self.clock.wrapping_add(1);
        let preview=self.previews.iter_mut().find(|preview|Self::matches(preview,&scope,url,poster))?;
        preview.used=self.clock;
        Some(preview.texture.clone())
    }
}
impl Pictures {
    pub(in crate::ui) fn purge_deleted_message_preview(&mut self, channel: &str, message: &str) {
        self.deleted_previews.purge(channel, message);
    }
    pub(in crate::ui) fn begin_message_preview(&mut self, message: &ChatMessage) { self.deleted_previews.begin(message); }
    pub(in crate::ui) fn end_message_preview(&mut self) { self.deleted_previews.scopes.pop(); }
    pub(in crate::ui) fn remember_message_preview(&mut self, url: &str, poster: &str, texture: &TextureHandle) {
        self.deleted_previews.remember(url,poster,texture);
    }
    /// A miss is final for this surface: never call texture()/animated() as a
    /// fallback, because those methods can start or resume remote work.
    pub(in crate::ui) fn deleted_message_preview(&mut self, url: &str, poster: &str) -> Option<TextureHandle> {
        self.deleted_previews.frozen(url,poster)
    }
}
