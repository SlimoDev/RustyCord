//! Received-message presentation; all source ranges come from the same pure
//! parser used by the composer. The stored/editable message is never rewritten.
use super::*;
use notdiscord_core::markdown::{Block, BlockKind, Style};

impl NotDiscordUi {
    pub(super) fn message_markdown(&mut self, ui: &mut egui::Ui, source: &str,
        message: &ChatMessage, state: &AppSnapshot) {
        let document = super::super::markdown_cache::document(ui.ctx(), source, &[]);
        let mut media = Vec::<String>::new();
        for (index, block) in document.blocks.iter().enumerate() {
            ui.push_id(("message-markdown", index), |ui| {
                let code = matches!(&block.kind, BlockKind::CodeBlock { .. });
                let (size, color) = match &block.kind {
                    BlockKind::Heading { level } => (self.prefs.chat_size * match level { 1 => 1.6, 2 => 1.35, _ => 1.15 }, self.theme.ink),
                    BlockKind::Subtext => ((self.prefs.chat_size * 0.85).max(11.), self.theme.muted),
                    _ => (self.prefs.chat_size, self.theme.ink),
                };
                let row = ui.horizontal_top(|ui| {
                    if block.quote_depth > 0 { ui.add_space(f32::from(block.quote_depth.min(8)) * 9.); }
                    if let BlockKind::ListItem { ordered, number, depth } = &block.kind {
                        ui.add_space(f32::from(*depth) * 12.);
                        let bullet = if *ordered { format!("{number}. ") } else { "• ".into() };
                        selectable_message_label(ui, theme::markdown_text(bullet, size, color, Style::default()));
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.;
                        if code {
                            let text = source.get(block.content.clone()).unwrap_or("");
                            egui::Frame::new().fill(self.theme.surface).corner_radius(4)
                                .inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
                                    selectable_message_label(ui, theme::markdown_text(text, size, color,
                                        Style { code: true, ..Default::default() }));
                                });
                        } else {
                            self.markdown_block_lines(ui, source, block, message, state, size, color, &mut media);
                        }
                    });
                });
                if block.quote_depth > 0 {
                    ui.painter().vline(row.response.rect.left() + 2., row.response.rect.y_range(),
                        egui::Stroke::new(2_f32, self.theme.line));
                }
            });
        }
        for url in media {
            let gif = notdiscord_core::parsing::direct_gif(&url) || notdiscord_core::parsing::gif_page(&url);
            self.media_card(ui, &url, &url, gif, if gif { "GIF" } else { "Imagen" }, None);
        }
    }

    fn markdown_block_lines(&mut self, ui: &mut egui::Ui, source: &str, block: &Block,
        message: &ChatMessage, state: &AppSnapshot, size: f32, color: Color32, media: &mut Vec<String>) {
        let content = source.get(block.content.clone()).unwrap_or("");
        let mut offset = block.content.start;
        let spoiler_scope = ui.id().with(("message-spoilers", &message.channel_id, &message.id,
            state.notification_generation, state.current_user.as_ref().map(|person| person.id.as_str())));
        let mut spoiler_groups = HashMap::<usize, usize>::new();
        let mut spoiler_group = None;
        for span in &block.spans {
            if span.syntax && span.style.spoiler && source.get(span.range.clone()) == Some("||") {
                spoiler_group = if spoiler_group.is_some() { None } else { Some(span.range.start) };
            } else if span.style.spoiler && !span.syntax {
                spoiler_groups.insert(span.range.start, spoiler_group.unwrap_or(span.range.start));
            }
        }
        if content.is_empty() { ui.add_space(size * 1.45); return; }
        for line in content.split_inclusive('\n') {
            let body = line.trim_end_matches(['\r', '\n']);
            let line_range = offset..offset + body.len();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0., 0.);
                ui.set_min_height(size * 1.45);
                let mut hidden_groups = HashSet::new();
                for span in block.spans.iter().filter(|span| !span.syntax) {
                    let start = span.range.start.max(line_range.start);
                    let end = span.range.end.min(line_range.end);
                    if start >= end { continue; }
                    let Some(text) = source.get(start..end) else { continue; };
                    let mut style = span.style;
                    if matches!(&block.kind, BlockKind::Heading { .. }) { style.bold = true; }
                    if style.spoiler {
                        let group = spoiler_groups.get(&span.range.start).copied().unwrap_or(span.range.start);
                        let key = spoiler_scope.with(group);
                        let revealed = ui.ctx().data(|data| data.get_temp::<bool>(key)).unwrap_or(false);
                        if !revealed {
                            if hidden_groups.insert(group) && ui.add(egui::Label::new(RichText::new("Contenido oculto").size(size)
                                .background_color(self.theme.selected)).sense(egui::Sense::click()))
                                .on_hover_text("Mostrar contenido oculto").clicked() {
                                ui.ctx().data_mut(|data| data.insert_temp(key, true));
                            }
                            continue;
                        }
                        style.spoiler = false;
                    }
                    if style.code {
                        selectable_message_label(ui, theme::markdown_text(text, size, color, style));
                    } else if let Some(url) = span.link.as_ref().and_then(|range| source.get(range.clone())) {
                        if history_link_activated(&selectable_link_label(ui,
                            theme::markdown_text(text, size, self.theme.accent, style), url).on_hover_text(url)) {
                            self.open_message_link(ui.ctx(), url);
                        }
                    } else {
                        self.message_inline(ui, text, source, start, message, state, style, size, color, media);
                    }
                }
            });
            offset += line.len();
        }
    }
}
