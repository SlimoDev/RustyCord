//! One explicit metadata read for an expired attachment link of a known message.
use super::*;

impl Controller {
    fn attachment_reply(&self, action: &UiAction, url: &str, error: &str) {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        self.data("attachment-url", json!({"operation":extra["operation"],"url":url,"error":error}));
    }
    pub(super) fn refresh_attachment(&mut self, mut action: UiAction) -> Result<(), String> {
        let channel = Self::valid(&action.channel_id)?.to_owned();
        let message = Self::valid(&action.id)?.to_owned();
        if self.transport_suspended() {
            self.attachment_reply(&action, "", super::connection::OFFLINE);
            return Ok(());
        }
        let access = if channel == self.state.selected_channel_id { Some(self.state.channel_access.clone()) }
            else { self.channels.get(&channel).map(|value| self.channel_access(value)) };
        if access.is_some_and(|access| access.known && (!access.can_view || !access.can_read_history)) {
            self.attachment_reply(&action, "", "Ya no puedes leer este canal.");
            return Ok(());
        }
        let api = self.api()?;
        action.kind = "__attachment-refresh".into();
        self.launch(action, None, async move {
            api.get_once(&format!("channels/{channel}/messages?around={message}&limit=1")).await
                .map_err(|error| error.to_string())
        });
        Ok(())
    }
    pub(super) fn complete_attachment_refresh(&self, action: &UiAction, value: &Value) {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        let url = items(value).iter().find(|message| s(message, "id") == action.id)
            .and_then(|message| items(&message["attachments"]).iter()
                .find(|attachment| s(attachment, "id") == s(&extra, "attachment"))
                .map(|attachment| s(attachment, "url")));
        match url {
            Some(url) if !url.is_empty() => self.attachment_reply(action, &url, ""),
            _ => self.attachment_reply(action, "", "El mensaje ya no contiene este adjunto."),
        }
    }
    pub(super) fn attachment_refresh_failed(&self, action: &UiAction) {
        self.attachment_reply(action, "", "No se pudo actualizar el enlace. Puedes reabrir el adjunto más tarde.");
    }
}
