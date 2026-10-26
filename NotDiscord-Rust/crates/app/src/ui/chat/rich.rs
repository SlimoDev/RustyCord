//! Received bot/link content. Rendering never submits interactions or bot
//! commands; only ordinary HTTP(S) links can open after an explicit click.
use super::*;
const INTERACTION_NOTE: &str = "Las interacciones de bots no están implementadas en NotDiscord. Utiliza Discord para este control.";

fn browser_link(value: &str) -> bool {
    value.len() <= 2048 && !value.chars().any(char::is_control)
        && url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some() && url.username().is_empty() && url.password().is_none())
}
impl NotDiscordUi {
    fn received_link(&mut self, ui: &mut egui::Ui, label: &str, url: &str, strong: bool) {
        let text = RichText::new(label).size(self.prefs.chat_size).color(if browser_link(url) { self.theme.accent } else { self.theme.ink });
        let text = if strong { text.strong() } else { text };
        if browser_link(url) {
            if history_link_activated(&selectable_link_label(ui, text).on_hover_text(url)) {
                self.open_message_link(ui.ctx(), url);
            }
        } else { selectable_message_label(ui, text); }
    }
    pub(super) fn draw_message_rich(&mut self, ui: &mut egui::Ui, message: &ChatMessage, state: &AppSnapshot) {
        for (index, embed) in message.embeds.iter().take(10).enumerate() {
            ui.push_id(("received-embed", index), |ui| {
                let response = egui::Frame::new().fill(self.theme.surface).corner_radius(5)
                    .inner_margin(egui::Margin::same(11)).show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(540.));
                        if !embed.author.is_empty() { self.received_link(ui, &embed.author, &embed.author_url, false); }
                        if !embed.provider.is_empty() { ui.small(RichText::new(&embed.provider).color(self.theme.muted)); }
                        if !embed.title.is_empty() { self.received_link(ui, &embed.title, &embed.url, true); }
                        if !embed.description.is_empty() { self.message_text(ui, &embed.description, message, state); }
                        for field in embed.fields.iter().take(25) {
                            ui.add_space(4.);
                            if !field.name.is_empty() { selectable_message_label(ui, RichText::new(&field.name).size(self.prefs.chat_size).strong()); }
                            if !field.value.is_empty() { self.message_text(ui, &field.value, message, state); }
                        }
                        if !embed.footer.is_empty() { ui.add_space(4.); selectable_message_label(ui, RichText::new(&embed.footer).size(11.).color(self.theme.muted)); }
                    }).response;
                let color = if embed.color > 0 { Color32::from_rgb((embed.color >> 16) as u8, (embed.color >> 8) as u8, embed.color as u8) } else { self.theme.line };
                ui.painter().line_segment([response.rect.left_top() + egui::vec2(2., 3.), response.rect.left_bottom() + egui::vec2(2., -3.)], egui::Stroke::new(3_f32, color));
            });
        }
        let mut shown_media: Vec<&str> = Vec::new();
        for (index, component) in message.components.iter().take(40).enumerate() {
            ui.push_id(("received-component", index), |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(f32::from(component.depth.min(4)) * 7.);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        match component.kind {
                            1 | 9 | 17 | 18 => {
                                if component.kind == 17 { ui.separator(); }
                                if !component.text.is_empty() { self.message_text(ui, &component.text, message, state); }
                            }
                            10 => self.message_text(ui, &component.text, message, state),
                            2 => {
                                let label = if component.text.is_empty() { "Botón del bot" } else { &component.text };
                                if browser_link(&component.url) {
                                    if ui.button(label).on_hover_text(&component.url).clicked() {
                                        if let Err(error) = crate::platform::open_link(&component.url) { self.error = error; }
                                    }
                                } else { ui.add_enabled(false, egui::Button::new(label)).on_disabled_hover_text(INTERACTION_NOTE); }
                            }
                            3 | 5..=8 | 19 | 21..=23 => {
                                let label = component.text.lines().next().filter(|label| !label.is_empty()).unwrap_or("Seleccionar una opción");
                                ui.add_enabled(false, egui::Button::new(format!("{label} ▾")))
                                    .on_disabled_hover_text(format!("{INTERACTION_NOTE}\n{}", component.text));
                            }
                            11 | 12 => {
                                let duplicate = shown_media.iter().any(|known| notdiscord_core::parsing::same_media_resource(known, &component.url));
                                if !component.url.is_empty() { shown_media.push(&component.url); }
                                if duplicate { return; }
                                if component.spoiler {
                                    ui.collapsing("Mostrar contenido oculto", |ui| self.component_media(ui, component));
                                } else { self.component_media(ui, component); }
                            }
                            13 => {
                                let name = if component.text.is_empty() { "Archivo adjunto" } else { &component.text };
                                self.received_link(ui, name, &component.url, false);
                            }
                            14 => { ui.separator(); }
                            _ => {
                                if !component.text.is_empty() { self.message_text(ui, &component.text, message, state); }
                                ui.add_enabled(false, egui::Button::new(format!("Componente del bot (tipo {})", component.kind)))
                                    .on_disabled_hover_text(INTERACTION_NOTE);
                            }
                        }
                    });
                });
            });
        }
        if (message.author.is_bot || !message.webhook_id.is_empty()) && message.content.is_empty()
            && message.embeds.is_empty() && message.components.is_empty() && message.attachments.is_empty() && message.stickers.is_empty() {
            ui.small(RichText::new("El servicio no proporcionó contenido para este mensaje.").color(self.theme.muted));
        }
    }
    pub(super) fn draw_deleted_message_rich(&mut self, ui: &mut egui::Ui, message: &ChatMessage, state: &AppSnapshot) {
        // A tombstone may retain observed text; it must never restart media
        // downloads/decoders merely because the deleted row becomes visible.
        let red = Color32::from_rgb(242, 63, 67);
        for embed in message.embeds.iter().take(10) {
            for text in [&embed.author, &embed.title, &embed.description, &embed.footer] {
                if !text.is_empty() { selectable_message_label(ui, RichText::new(readable_mentions(text, &message.mentions, state)).size(self.prefs.chat_size).color(red)); }
            }
            for field in embed.fields.iter().take(25) {
                for text in [&field.name, &field.value] {
                    if !text.is_empty() { selectable_message_label(ui, RichText::new(readable_mentions(text, &message.mentions, state)).size(self.prefs.chat_size).color(red)); }
                }
            }
        }
        for component in message.components.iter().take(40).filter(|component| !component.text.is_empty()) {
            selectable_message_label(ui, RichText::new(readable_mentions(&component.text, &message.mentions, state)).size(self.prefs.chat_size).color(red));
        }
    }
    fn component_media(&mut self, ui: &mut egui::Ui, component: &ChatComponent) {
        if !component.text.is_empty() { selectable_message_label(ui, RichText::new(&component.text).size(self.prefs.chat_size)); }
        if notdiscord_core::parsing::trusted_media(&component.url) && resources::is_image_url(&component.url) {
            let gif = notdiscord_core::parsing::direct_gif(&component.url) || notdiscord_core::parsing::gif_page(&component.url);
            self.media_card(ui, &component.url, &component.url, gif, if gif { "GIF" } else { "Imagen" }, None);
        } else if browser_link(&component.url) { self.received_link(ui, "Abrir contenido multimedia", &component.url, false); }
        else { ui.small(RichText::new("El servicio no proporcionó un enlace multimedia compatible.").color(self.theme.muted)); }
    }
}
