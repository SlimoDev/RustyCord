use super::*;

pub(super) struct Editor {
    owner: String,
    load_token: String,
    loading: bool,
    ready: bool,
    bio: String,
    status: String,
    emoji: Value,
    bio_pending: Option<String>,
    status_pending: Option<String>,
    note: String,
    failed: bool,
}
impl Editor {
    fn new(owner: String) -> Self {
        Self {
            owner,
            load_token: uuid::Uuid::new_v4().to_string(),
            loading: true,
            ready: false,
            bio: String::new(),
            status: String::new(),
            emoji: Value::Null,
            bio_pending: None,
            status_pending: None,
            note: String::new(),
            failed: false,
        }
    }
    fn update(&mut self, kind: &str, data: &Value) {
        if s(data, "owner") != self.owner {
            return;
        }
        if kind == "profile-editor-loaded" {
            if s(data, "token") != self.load_token {
                return;
            }
            self.loading = false;
            if let Some(error) = data.get("error").and_then(Value::as_str) {
                self.note = error.into();
                self.failed = true;
                return;
            }
            let value = &data["value"];
            self.bio = s(value, "bio").into();
            self.status = s(&value["custom_status"], "text").into();
            self.emoji = value["custom_status"].clone();
            self.ready = true;
            self.note.clear();
            self.failed = false;
            return;
        }
        let bio = s(data, "kind") == "bio";
        let pending = if bio {
            &mut self.bio_pending
        } else {
            &mut self.status_pending
        };
        if pending.as_deref() != Some(s(data, "token")) {
            return;
        }
        *pending = None;
        if let Some(error) = data.get("error").and_then(Value::as_str) {
            self.note = error.into();
            self.failed = true;
            return;
        }
        if bio {
            self.bio = s(&data["value"], "bio").into();
        } else {
            self.status = s(&data["value"]["custom_status"], "text").into();
            self.emoji = data["value"]["custom_status"].clone();
        }
        self.note = if bio {
            "Biografía guardada y confirmada."
        } else {
            "Estado guardado y confirmado."
        }
        .into();
        self.failed = false;
    }
}

impl NotDiscordUi {
    fn send_profile_request(&mut self, kind: &str, token: &str, text: &str, extra: String) -> bool {
        if self
            .command
            .send(UiAction {
                kind: kind.into(),
                id: token.into(),
                text: text.into(),
                extra,
                ..Default::default()
            })
            .is_ok()
        {
            true
        } else {
            self.error = "El servicio de la aplicación se cerró. El borrador se conserva.".into();
            false
        }
    }
    pub(super) fn profile_editor_update(&mut self, kind: &str, data: &Value) {
        if let Some(editor) = &mut self.profile_editor {
            editor.update(kind, data);
        }
    }
    fn open_profile_editor(&mut self, owner: &str) {
        let mut editor = Editor::new(owner.into());
        if !self.send_profile_request("profile-editor-load", &editor.load_token, "", String::new())
        {
            editor.loading = false;
            editor.note = self.error.clone();
            editor.failed = true;
        }
        self.profile_editor = Some(editor);
    }
    pub(super) fn draw_profile_editor(
        &mut self,
        ui: &mut egui::Ui,
        state: &AppSnapshot,
        person: &Person,
    ) {
        if state
            .current_user
            .as_ref()
            .is_none_or(|me| me.id != person.id)
        {
            return;
        }
        let active = self
            .profile_editor
            .as_ref()
            .is_some_and(|editor| editor.owner == person.id);
        let pending = self
            .profile_editor
            .as_ref()
            .is_some_and(|editor| editor.bio_pending.is_some() || editor.status_pending.is_some());
        if ui
            .add_enabled(
                !active || !pending,
                egui::Button::new(if active {
                    "Cerrar edición del perfil"
                } else {
                    "Editar estado y biografía"
                }),
            )
            .clicked()
        {
            if active {
                self.profile_editor = None;
            } else {
                self.open_profile_editor(&person.id);
            }
        }
        let Some(mut editor) = self
            .profile_editor
            .take()
            .filter(|editor| editor.owner == person.id)
        else {
            return;
        };
        egui::Frame::new()
            .fill(self.theme.surface)
            .corner_radius(8)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.strong("Mi perfil global");
                ui.horizontal(|ui| {
                    ui.label("Disponibilidad");
                    ui.add_enabled_ui(!state.demo, |ui| {
                        egui::ComboBox::from_id_salt("profile-presence")
                            .selected_text(if state.self_presence=="invisible" {"Invisible"} else {presence_label(&state.self_presence)})
                            .show_ui(ui, |ui| {
                                for (value, label) in [
                                    ("online", "En línea"),
                                    ("idle", "Ausente"),
                                    ("dnd", "No molestar"),
                                    ("invisible", "Invisible"),
                                ] {
                                    if ui
                                        .selectable_label(state.self_presence == value, label)
                                        .clicked()
                                    {
                                        self.send("status", "", value, "", "");
                                    }
                                }
                            });
                    });
                });
                ui.small(
                    "La disponibilidad mostrada cambia cuando el servicio confirma el ajuste.",
                );
                if editor.loading {
                    ui.label("Cargando biografía y estado actuales…");
                }
                let pending = editor.bio_pending.is_some() || editor.status_pending.is_some();
                ui.add_enabled_ui(editor.ready && !editor.loading, |ui| {
                    ui.label("Estado personalizado");
                    ui.add_enabled_ui(editor.status_pending.is_none(), |ui| {
                        ui.add(bordered(
                            egui::TextEdit::singleline(&mut editor.status)
                                .id_salt("profile-status")
                                .char_limit(128)
                                .desired_width(ui.available_width()),
                        ).focus_salt("profile-status").submit_action("profile-status"));
                        if !s(&editor.emoji, "emoji_id").is_empty()
                            || !s(&editor.emoji, "emoji_name").is_empty()
                        {
                            let name = if s(&editor.emoji, "emoji_name").is_empty() {
                                "Emoji personalizado".to_string()
                            } else {
                                s(&editor.emoji, "emoji_name").to_string()
                            };
                            ui.horizontal(|ui| {
                                ui.label(format!("Emoji actual: {name}"));
                                if ui.small_button("Quitar emoji").clicked() {
                                    editor.emoji = Value::Null;
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            ui.small(format!("{} / 128", editor.status.chars().count()));
                            if ui
                                .add_enabled(!state.demo, egui::Button::new("Guardar estado"))
                                .clicked() || !state.demo && super::input_ownership::take_action(ui.ctx(),"profile-status")
                            {
                                let token = uuid::Uuid::new_v4().to_string();
                                if self.send_profile_request(
                                    "custom-status",
                                    &token,
                                    &editor.status,
                                    editor.emoji.to_string(),
                                ) {
                                    editor.status_pending = Some(token);
                                    editor.note.clear();
                                } else {
                                    editor.note = self.error.clone();
                                    editor.failed = true;
                                }
                            }
                            if ui
                                .add_enabled(!state.demo, egui::Button::new("Borrar estado"))
                                .clicked()
                            {
                                let token = uuid::Uuid::new_v4().to_string();
                                if self.send_profile_request(
                                    "custom-status",
                                    &token,
                                    "",
                                    String::new(),
                                ) {
                                    editor.status_pending = Some(token);
                                    editor.note.clear();
                                } else {
                                    editor.note = self.error.clone();
                                    editor.failed = true;
                                }
                            }
                        });
                    });
                    ui.add_space(8.);
                    ui.label("Biografía");
                    ui.add_enabled_ui(editor.bio_pending.is_none(), |ui| {
                        ui.add(bordered(
                            egui::TextEdit::multiline(&mut editor.bio)
                                .id_salt("profile-bio")
                                .char_limit(190)
                                .desired_rows(4)
                                .desired_width(ui.available_width()),
                        ).focus_salt("profile-bio"));
                        let length = editor.bio.encode_utf16().count();
                        ui.horizontal(|ui| {
                            ui.small(format!("{length} / 190"));
                            if ui
                                .add_enabled(
                                    !state.demo && length <= 190,
                                    egui::Button::new("Guardar biografía"),
                                )
                                .clicked()
                            {
                                let token = uuid::Uuid::new_v4().to_string();
                                if self.send_profile_request(
                                    "save-profile-bio",
                                    &token,
                                    &editor.bio,
                                    String::new(),
                                ) {
                                    editor.bio_pending = Some(token);
                                    editor.note.clear();
                                } else {
                                    editor.note = self.error.clone();
                                    editor.failed = true;
                                }
                            }
                        });
                    });
                });
                if pending {
                    ui.label("Guardando… Se mostrará el valor confirmado por el servicio.");
                }
                if !editor.note.is_empty() {
                    ui.colored_label(
                        if editor.failed {
                            Color32::from_rgb(242, 63, 67)
                        } else {
                            self.theme.muted
                        },
                        &editor.note,
                    );
                }
                if ui
                    .add_enabled(
                        !pending && !editor.loading,
                        egui::Button::new("Recargar valores actuales"),
                    )
                    .clicked()
                {
                    editor.load_token = uuid::Uuid::new_v4().to_string();
                    editor.loading = true;
                    editor.note.clear();
                    if !self.send_profile_request(
                        "profile-editor-load",
                        &editor.load_token,
                        "",
                        String::new(),
                    ) {
                        editor.loading = false;
                        editor.note = self.error.clone();
                        editor.failed = true;
                    }
                }
            });
        self.profile_editor = Some(editor);
    }
    pub(super) fn open_profile_avatar(&mut self, person: &Person) {
        if let Some(url) = self.pictures.avatar_viewer_url(&self.ctx, person) {
            self.viewer = Some(Viewer {
                url,
                local_path: None,
                zoom: 1.,
                pan: egui::Vec2::ZERO,
            });
        }
    }
    pub(super) fn profile_avatar_button(
        &mut self,
        ui: &mut egui::Ui,
        person: &Person,
        presence: &str,
        size: f32,
    ) -> egui::Response {
        let response = self.pictures.profile_avatar(ui, person, size);
        if !presence.is_empty() {
            let pos = response.rect.right_bottom() - egui::vec2(size * 0.12, size * 0.12);
            ui.painter()
                .circle_filled(pos, size * 0.14, self.theme.panel);
            ui.painter()
                .circle_filled(pos, size * 0.105, presence_color(presence));
        }
        response.on_hover_text("Ampliar foto de perfil")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_own_profile_exposes_editing_and_drafts_survive_failed_saves() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        let me = Person {
            id: "10".into(),
            name: "Self".into(),
            ..Default::default()
        };
        let state = AppSnapshot {
            signed_in: true,
            current_user: Some(me.clone()),
            ..Default::default()
        };
        let ctx = app.ctx.clone();
        let other = Person {
            id: "20".into(),
            ..Default::default()
        };
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| app.draw_profile_editor(ui, &state, &other));
        });
        assert!(output.shapes.iter().all(|_| app.profile_editor.is_none()));
        assert!(commands.try_recv().is_err());
        app.open_profile_editor(&me.id);
        let action = commands.try_recv().unwrap();
        assert_eq!(action.kind, "profile-editor-load");
        app.profile_editor_update("profile-editor-loaded",&serde_json::json!({"owner":"10","token":action.id,"value":{"bio":"Current","custom_status":{"text":"Hello"}}}));
        let editor = app.profile_editor.as_mut().unwrap();
        assert!(editor.ready);
        editor.bio = "Unsaved".into();
        editor.bio_pending = Some("save".into());
        app.profile_editor_update(
            "profile-edit-result",
            &serde_json::json!({"owner":"10","token":"stale","kind":"bio","value":{"bio":"Old"}}),
        );
        assert_eq!(app.profile_editor.as_ref().unwrap().bio, "Unsaved");
        app.profile_editor_update(
            "profile-edit-result",
            &serde_json::json!({"owner":"10","token":"save","kind":"bio","error":"Offline"}),
        );
        let editor = app.profile_editor.as_ref().unwrap();
        assert_eq!(editor.bio, "Unsaved");
        assert!(editor.bio_pending.is_none());
        assert!(editor.failed);
    }
    #[test]
    fn profile_avatar_opens_same_remote_viewer_without_sending_account_actions() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        app.open_profile_avatar(&Person {
            id: "20".into(),
            avatar_url: Some(
                "https://cdn.discordapp.com/avatars/20/avatar.webp?size=128&hm=keep".into(),
            ),
            ..Default::default()
        });
        let viewer = app.viewer.as_ref().unwrap();
        assert!(viewer.local_path.is_none());
        assert!(viewer.url.contains("size=2048"));
        assert!(viewer.url.contains("hm=keep"));
        assert_eq!(viewer.zoom, 1.);
        assert!(commands.try_recv().is_err());
    }
}
