//! Received bot/link content. Rendering never submits interactions or bot
//! commands; only ordinary HTTP(S) links can open after an explicit click.
use super::*;
const INTERACTION_NOTE: &str = "Las interacciones de bots no están implementadas en NotDiscord. Utiliza Discord para este control.";

fn browser_link(value: &str) -> bool {
    value.len() <= 2048 && !value.chars().any(char::is_control)
        && url::Url::parse(value).is_ok_and(|url| matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some() && url.username().is_empty() && url.password().is_none())
}
fn gif_url(value: &str) -> bool {
    notdiscord_core::parsing::gif_page(value) || notdiscord_core::parsing::direct_animation(value)
}
/// Link-preview text and the media card come from separate bounded parsers.
/// Suppress only the redundant wrapper for a recognized GIF that has its own
/// media representation. An article which happens to contain a GIF keeps its
/// ordinary title, description and fields.
fn redundant_gif_embed(embed: &ChatEmbed, message: &ChatMessage) -> bool {
    gif_url(&embed.url) && embed.fields.is_empty()
        && (!message.components_v2 && message.attachments.iter().any(|attachment|
            (attachment.animated || attachment.content_type.as_deref() == Some("image/gif")
                || gif_url(&attachment.url))
            && notdiscord_core::parsing::same_media_resource(&attachment.url, &embed.url))
            || message.components.iter().any(|component|
                matches!(component.kind, 11..=13) && gif_url(&component.url)
                    && notdiscord_core::parsing::same_media_resource(&component.url, &embed.url)))
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
            if redundant_gif_embed(embed, message) { continue; }
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
                                if gif_url(&component.url) {
                                    let duplicate = shown_media.iter().any(|known|
                                        notdiscord_core::parsing::same_media_resource(known, &component.url));
                                    if duplicate { return; }
                                    shown_media.push(&component.url);
                                    if component.spoiler {
                                        ui.collapsing("Mostrar contenido oculto", |ui| self.component_media(ui, component));
                                    } else { self.component_media(ui, component); }
                                } else {
                                    let name = if component.text.is_empty() { "Archivo adjunto" } else { &component.text };
                                    self.received_link(ui, name, &component.url, false);
                                }
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
            if redundant_gif_embed(embed, message) { continue; }
            for text in [&embed.author, &embed.title, &embed.description, &embed.footer] {
                if !text.is_empty() { selectable_message_label(ui, RichText::new(readable_mentions(text, &message.mentions, state)).size(self.prefs.chat_size).color(red)); }
            }
            self.draw_deleted_text_previews(ui, &embed.description, message);
            for field in embed.fields.iter().take(25) {
                for text in [&field.name, &field.value] {
                    if !text.is_empty() { selectable_message_label(ui, RichText::new(readable_mentions(text, &message.mentions, state)).size(self.prefs.chat_size).color(red)); }
                }
                self.draw_deleted_text_previews(ui, &field.value, message);
            }
        }
        for component in message.components.iter().take(40).filter(|component| !component.text.is_empty()) {
            selectable_message_label(ui, RichText::new(readable_mentions(&component.text, &message.mentions, state)).size(self.prefs.chat_size).color(red));
        }
        let mut shown_media: Vec<&str> = Vec::new();
        for component in message.components.iter().take(40).filter(|component|
            matches!(component.kind, 11..=13) && resources::is_image_url(&component.url)) {
            let duplicate = message.attachments.iter().any(|attachment|
                notdiscord_core::parsing::same_media_resource(&attachment.url, &component.url)
                    || attachment.preview_url.as_deref().is_some_and(|poster|
                        notdiscord_core::parsing::same_media_resource(poster, &component.url)))
                || shown_media.iter().any(|known|
                    notdiscord_core::parsing::same_media_resource(known, &component.url));
            if duplicate { continue; }
            shown_media.push(&component.url);
            let name = if gif_url(&component.url) { "GIF" } else { "Imagen" };
            let mut draw = |ui: &mut egui::Ui| {
                if !self.draw_deleted_media_card(ui, &component.url, &component.url, name, None)
                    && component.text.is_empty() {
                    selectable_message_label(ui, RichText::new("Contenido multimedia eliminado · vista previa no disponible en esta sesión")
                        .size(self.prefs.chat_size).color(red));
                }
            };
            if component.spoiler { ui.collapsing("Mostrar contenido oculto", draw); }
            else { draw(ui); }
        }
    }
    fn component_media(&mut self, ui: &mut egui::Ui, component: &ChatComponent) {
        let gif = gif_url(&component.url);
        if !gif && !component.text.is_empty() { selectable_message_label(ui, RichText::new(&component.text).size(self.prefs.chat_size)); }
        if gif || notdiscord_core::parsing::trusted_media(&component.url) && resources::is_image_url(&component.url) {
            self.media_card(ui, &component.url, &component.url, gif, if gif { "GIF" } else { "Imagen" }, None);
        } else if browser_link(&component.url) { self.received_link(ui, "Abrir contenido multimedia", &component.url, false); }
        else { ui.small(RichText::new("El servicio no proporcionó un enlace multimedia compatible.").color(self.theme.muted)); }
    }
}

#[cfg(test)]
mod gif_wrapper_tests {
    use super::*;

    #[test]
    fn gif_wrapper_requires_matching_media_and_keeps_ordinary_link_information() {
        let page = "https://klipy.com/gifs/synthetic-greeting";
        let gif = "https://static.klipy.com/synthetic/greeting.gif";
        let parse = |url| notdiscord_core::parsing::message(&serde_json::json!({
            "id":"300", "channel_id":"200", "author":{"id":"100","username":"Synthetic"},
            "content":url, "embeds":[{"url":url,"title":"Synthetic greeting","provider":{"name":"Klipy"},
                "image":{"url":gif},"thumbnail":{"url":gif}}]
        }));
        let message = parse(page);
        assert!(redundant_gif_embed(&message.embeds[0], &message));
        let article = parse("https://example.test/greeting-article");
        assert!(!redundant_gif_embed(&article.embeds[0], &article));
        let mut missing = message.clone();
        missing.attachments.clear();
        assert!(!redundant_gif_embed(&missing.embeds[0], &missing));
        let mut components_only = message.clone();
        components_only.components_v2 = true;
        assert!(!redundant_gif_embed(&components_only.embeds[0], &components_only));
        let mut rich = message;
        rich.embeds[0].fields.push(ChatEmbedField { name:"Bot field".into(),value:"Preserved content".into(),..Default::default() });
        assert!(!redundant_gif_embed(&rich.embeds[0], &rich));
    }
}
