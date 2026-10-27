//! Explicit forgetting of observed local tombstones. No Discord request and no
//! history file deletion: the current change journal is session memory only.
use super::*;

impl Controller {
    pub(super) fn purge_deleted_log(&mut self, action: &UiAction) -> Result<(), String> {
        let channel = action.channel_id.as_str();
        let id = action.id.as_str();
        if !parsing::valid_id(channel) || !parsing::valid_id(id) {
            return Err("El registro local no pertenece a un mensaje y canal válidos.".into());
        }
        let cached = self.cache.find(channel, id);
        let purged = self.features.log_is_purged(channel, id);
        let known_deleted = purged || self.features.has_deleted_log(channel, id)
            || cached.as_ref().is_some_and(|message| message.deleted)
            || self.state.messages.iter().chain(&self.state.pinned_messages)
                .any(|message| message.id == id && message.channel_id == channel && message.deleted);
        // An edit snapshot in the history view is a previous version, not proof
        // that a deleted message is live. Current chat/search/pins/cache are.
        let live = cached.as_ref().is_some_and(|message| !message.deleted)
            || self.state.pinned_messages.iter().any(|message|
                message.id == id && message.channel_id == channel && !message.deleted)
            || self.state.view != "history" && self.state.messages.iter().any(|message|
                message.id == id && message.channel_id == channel && !message.deleted);
        if !known_deleted || !purged && live {
            return Err("Sólo se puede retirar el registro local de un mensaje ya eliminado.".into());
        }
        // Reserve the bounded marker before changing any payload. A rejected
        // operation leaves all message/history/preview content intact.
        self.features.purge_deleted_log(channel, id)?;
        self.cache.purge_deleted_log(channel, id);
        self.features.filter_purged(&mut self.state.messages);
        self.features.filter_purged(&mut self.state.pinned_messages);
        self.pending_deltas.retain(|(_, value)|
            s(value, "channel_id") != channel || s(value, "id") != id);
        self.pending_delta_bytes = self.pending_deltas.iter()
            .map(|(_, value)| value.to_string().encode_utf16().count() * 2).sum();
        for item in self.state.direct_messages.iter_mut().chain(&mut self.state.channels) {
            if item.id == channel && item.last_message_id == id { item.message_preview = None; }
        }
        if self.state.focus_message_id == id {
            self.state.focus_message_id.clear();
            self.state.focus_message_version = self.state.focus_message_version.wrapping_add(1);
        }
        self.publish();
        self.flush_snapshot(true);
        // UI frees only the frozen per-message preview scope on this successful
        // acknowledgement, never arbitrary media still used by live messages.
        self.data("deleted-log-purged", json!({"channelId":channel,"messageId":id}));
        Ok(())
    }
}
