use super::*;
use notdiscord_core::guild_settings::{self, GuildSettings};
use serde_json::json;

const PERMISSIONS: &[(u64, &str)] = &[
    (1 << 10, "Ver canales"),
    (1 << 16, "Leer el historial"),
    (1 << 11, "Enviar mensajes"),
    (1 << 15, "Adjuntar archivos"),
    (1 << 14, "Insertar enlaces"),
    (1 << 6, "Añadir reacciones"),
    (1 << 17, "Mencionar @everyone y @here"),
    (1 << 20, "Conectar a voz"),
    (1 << 21, "Hablar"),
    (1 << 9, "Vídeo"),
    (1 << 25, "Usar detección de voz"),
    (1 << 22, "Silenciar miembros"),
    (1 << 23, "Ensordecer miembros"),
    (1 << 24, "Mover miembros"),
    (1 << 26, "Cambiar apodo propio"),
    (1 << 27, "Gestionar apodos"),
    (1 << 4, "Gestionar canales"),
    (1 << 28, "Gestionar roles"),
    (1 << 5, "Gestionar servidor"),
    (1 << 13, "Gestionar mensajes"),
    (8, "Administrador"),
];

#[derive(Clone, Default)]
struct Editor {
    revision: u64,
    section: u8,
    name: String,
    description: String,
    verification: i32,
    filter: i32,
    notifications: i32,
    role: String,
    role_name: String,
    role_color: String,
    role_bits: u64,
    role_hoist: bool,
    role_mentionable: bool,
    channel: String,
    channel_name: String,
    channel_topic: String,
    channel_kind: i32,
    channel_nsfw: bool,
    channel_limit: i32,
    channel_bitrate: i32,
    channel_slowmode: i32,
    channel_parent: String,
    overwrite_role: String,
    overwrite_allow: u64,
    overwrite_deny: u64,
    overwrite_key: String,
    nickname_user: String,
    nickname: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn harness() -> (NotDiscordUi, tokio::sync::mpsc::UnboundedReceiver<UiAction>) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let (_, updates) = std::sync::mpsc::channel();
        (
            NotDiscordUi::from_context(
                &egui::Context::default(),
                std::env::temp_dir()
                    .join(format!("notdiscord-guild-editor-{}", uuid::Uuid::new_v4())),
                sender,
                updates,
            ),
            receiver,
        )
    }
    fn text(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text(shape, label)),
            _ => None,
        }
    }
    fn label(output: &egui::FullOutput, name: &str) -> Option<egui::Rect> {
        output
            .shapes
            .iter()
            .find_map(|shape| text(&shape.shape, name))
    }
    fn frame(
        app: &mut NotDiscordUi,
        state: &AppSnapshot,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 900.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.draw_guild_settings(ui, state));
            },
        )
    }
    fn click(app: &mut NotDiscordUi, state: &AppSnapshot, point: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                app,
                state,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    #[test]
    fn guild_editor_loading_error_save_and_permissions_are_visible_and_effective() {
        let (mut app, mut actions) = harness();
        let mut state = AppSnapshot {
            signed_in: true,
            current_user: Some(Person {
                id: "1".into(),
                ..Default::default()
            }),
            guild_settings: Some(GuildSettings {
                guild_id: "10".into(),
                name: "Remote guild".into(),
                loading: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let output = frame(&mut app, &state, vec![]);
        assert!(label(&output, "Cargando los valores y permisos del servidor…").is_some());
        assert!(label(&output, "Guardar configuración general").is_none());
        let settings = state.guild_settings.as_mut().unwrap();
        settings.loading = false;
        settings.error = "Error sintético de permisos".into();
        settings.revision = 1;
        let output = frame(&mut app, &state, vec![]);
        assert!(label(&output, "Error sintético de permisos").is_some());
        let save = label(&output, "Guardar configuración general")
            .unwrap()
            .center();
        click(&mut app, &state, save);
        assert!(actions.try_recv().is_err());
        state.guild_settings.as_mut().unwrap().can_manage_guild = true;
        let output = frame(&mut app, &state, vec![]);
        let save = label(&output, "Guardar configuración general")
            .unwrap()
            .center();
        click(&mut app, &state, save);
        let action = actions.try_recv().unwrap();
        assert_eq!(action.kind, "guild-settings-save");
        assert_eq!(action.id, "10");
        assert_eq!(action.extra, "general");
        assert_eq!(
            serde_json::from_str::<Value>(&action.text).unwrap()["name"],
            "Remote guild"
        );
        state.guild_settings.as_mut().unwrap().saving = true;
        let output = frame(&mut app, &state, vec![]);
        assert!(label(&output, "Guardando y comprobando los cambios…").is_some());
        if let Some(save) = label(&output, "Guardar configuración general") {
            click(&mut app, &state, save.center());
        }
        assert!(actions.try_recv().is_err());
    }
}
impl Editor {
    fn loaded(settings: &GuildSettings, me: &str) -> Self {
        Self {
            revision: settings.revision,
            name: settings.name.clone(),
            description: settings.description.clone(),
            verification: settings.verification_level,
            filter: settings.explicit_content_filter,
            notifications: settings.default_message_notifications,
            nickname_user: me.into(),
            nickname: settings.own_nickname.clone(),
            channel_bitrate: 64000,
            ..Default::default()
        }
    }
    fn select_role(&mut self, settings: &GuildSettings, id: &str) {
        self.role = id.into();
        if let Some(role) = settings.roles.iter().find(|role| role.id == id) {
            self.role_name = role.name.clone();
            self.role_color = format!("{:06X}", role.color);
            self.role_bits = role.permissions;
            self.role_hoist = role.hoist;
            self.role_mentionable = role.mentionable;
        } else {
            self.role_name = "Nuevo rol".into();
            self.role_color = "000000".into();
            self.role_bits = 0;
            self.role_hoist = false;
            self.role_mentionable = false;
        }
    }
    fn select_channel(&mut self, settings: &GuildSettings, id: &str) {
        self.channel = id.into();
        self.overwrite_key.clear();
        if let Some(channel) = settings.channels.iter().find(|channel| channel.id == id) {
            self.channel_name = channel.name.clone();
            self.channel_topic = channel.topic.clone();
            self.channel_kind = channel.kind;
            self.channel_parent = channel.parent_id.clone();
            self.channel_nsfw = channel.nsfw;
            self.channel_limit = channel.user_limit;
            self.channel_bitrate = channel.bitrate.max(8000);
            self.channel_slowmode = channel.slowmode;
        } else {
            self.channel_name = "nuevo-canal".into();
            self.channel_topic.clear();
            self.channel_kind = 0;
            self.channel_parent.clear();
            self.channel_nsfw = false;
            self.channel_limit = 0;
            self.channel_bitrate = 64000;
            self.channel_slowmode = 0;
        }
    }
}
fn permission_checks(ui: &mut egui::Ui, bits: &mut u64, available: u64) {
    for &(bit, label) in PERMISSIONS {
        let mut enabled = *bits & bit != 0;
        if ui
            .add_enabled(
                available & bit != 0 || enabled,
                egui::Checkbox::new(&mut enabled, label),
            )
            .changed()
        {
            if enabled {
                *bits |= bit;
            } else {
                *bits &= !bit;
            }
        }
    }
}
fn write_action(guild: &str, kind: &str, target: &str, body: Value) -> UiAction {
    let mut action = UiAction::id("guild-settings-save", guild);
    action.extra = kind.into();
    action.channel_id = target.into();
    action.text = body.to_string();
    action
}
impl NotDiscordUi {
    fn guild_write(&mut self, guild: &str, kind: &str, target: &str, body: Value) {
        self.send("guild-settings-save", guild, body.to_string(), kind, target);
    }
    pub(super) fn draw_guild_settings(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        ui.horizontal(|ui| {
            if ui.button("← Volver al chat").clicked() {
                self.simple("guild-settings-close");
            }
            ui.heading("Configuración del servidor");
        });
        ui.separator();
        let Some(settings) = state.guild_settings.as_ref() else {
            ui.label("Selecciona un servidor para cargar su configuración.");
            return;
        };
        ui.label(RichText::new(&settings.name).strong());
        if settings.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Cargando los valores y permisos del servidor…");
            });
        }
        if settings.saving {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Guardando y comprobando los cambios…");
            });
        }
        if !settings.error.is_empty() {
            ui.colored_label(Color32::from_rgb(242, 100, 100), &settings.error);
        }
        if !settings.note.is_empty() {
            ui.label(&settings.note);
        }
        if ui
            .add_enabled(
                !settings.loading && !settings.saving,
                egui::Button::new("Actualizar desde el servidor"),
            )
            .clicked()
        {
            self.send("guild-settings-refresh", &settings.guild_id, "", "", "");
        }
        if settings.loading {
            return;
        }
        let me = state
            .current_user
            .as_ref()
            .map(|person| person.id.as_str())
            .unwrap_or("");
        let key = egui::Id::new(("guild-editor", &settings.guild_id));
        let mut editor = ui
            .ctx()
            .data_mut(|data| data.get_temp::<Editor>(key))
            .unwrap_or_else(|| Editor::loaded(settings, me));
        if editor.revision != settings.revision {
            let section = editor.section;
            editor = Editor::loaded(settings, me);
            editor.section = section;
        }
        ui.horizontal_wrapped(|ui| {
            for (index, label) in [
                "General",
                "Roles y permisos",
                "Canales",
                "Apodos",
                "Notificaciones e invitaciones",
            ]
            .iter()
            .enumerate()
            {
                ui.selectable_value(&mut editor.section, index as u8, *label);
            }
        });
        ui.separator();
        theme::vertical_scroll(ui)
            .id_salt("guild-editor-scroll")
            .show(ui, |ui| {
                ui.add_enabled_ui(!settings.saving, |ui| match editor.section {
                    0 => self.guild_general(ui, settings, &mut editor),
                    1 => self.guild_roles_editor(ui, settings, &mut editor),
                    2 => self.guild_channels_editor(ui, settings, &mut editor),
                    4 => {
                        if let Some(guild) = state
                            .guilds
                            .iter()
                            .find(|guild| guild.id == settings.guild_id)
                        {
                            self.guild_notification_options(ui, guild, state);
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                settings.channels.iter().any(|channel| channel.can_invite),
                                egui::Button::new("Invitar a personas…"),
                            )
                            .clicked()
                        {
                            self.send("guild-invite", &settings.guild_id, "", "", "");
                        }
                    }
                    _ => self.guild_nickname_editor(ui, state, settings, &mut editor, me),
                });
            });
        ui.ctx().data_mut(|data| data.insert_temp(key, editor));
    }
    fn guild_general(&mut self, ui: &mut egui::Ui, settings: &GuildSettings, editor: &mut Editor) {
        ui.label(format!("Propietario · {}", settings.owner_id));
        if !settings.can_manage_guild {
            ui.label(
                "Puedes consultar estos valores. Para modificarlos necesitas Gestionar servidor.",
            );
        }
        ui.add_enabled_ui(settings.can_manage_guild,|ui|{
            ui.label("Nombre del servidor");ui.add(bordered(egui::TextEdit::singleline(&mut editor.name).char_limit(100).desired_width(450.)));
            ui.label("Descripción");ui.add(bordered(egui::TextEdit::multiline(&mut editor.description).char_limit(120).desired_width(600.).desired_rows(3)));
            egui::ComboBox::from_label("Nivel de verificación").selected_text(match editor.verification{0=>"Ninguno",1=>"Bajo",2=>"Medio",3=>"Alto",_=>"Máximo"}).show_ui(ui,|ui|{
                for (value,name) in ["Ninguno","Bajo","Medio","Alto","Máximo"].iter().enumerate(){ui.selectable_value(&mut editor.verification,value as i32,*name);}
            });
            egui::ComboBox::from_label("Filtro de contenido explícito").selected_text(match editor.filter{0=>"No analizar",1=>"Miembros sin rol",_=>"Todos los miembros"}).show_ui(ui,|ui|{
                for (value,name) in ["No analizar","Miembros sin rol","Todos los miembros"].iter().enumerate(){ui.selectable_value(&mut editor.filter,value as i32,*name);}
            });
            egui::ComboBox::from_label("Notificaciones predeterminadas").selected_text(if editor.notifications==0{"Todos los mensajes"}else{"Solo menciones"}).show_ui(ui,|ui|{ui.selectable_value(&mut editor.notifications,0,"Todos los mensajes");ui.selectable_value(&mut editor.notifications,1,"Solo menciones");});
            if ui.button("Guardar configuración general").clicked(){self.guild_write(&settings.guild_id,"general","",json!({"name":editor.name.trim(),"description":editor.description,"verification_level":editor.verification,"explicit_content_filter":editor.filter,"default_message_notifications":editor.notifications}));}
        });
    }
    fn guild_roles_editor(
        &mut self,
        ui: &mut egui::Ui,
        settings: &GuildSettings,
        editor: &mut Editor,
    ) {
        let selected = settings
            .roles
            .iter()
            .find(|role| role.id == editor.role)
            .map(|role| role.name.as_str())
            .unwrap_or("Selecciona un rol");
        let mut chosen = editor.role.clone();
        egui::ComboBox::from_id_salt("guild-role")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for role in &settings.roles {
                    ui.selectable_value(
                        &mut chosen,
                        role.id.clone(),
                        format!(
                            "{}{}",
                            role.name,
                            if role.managed { " · integración" } else { "" }
                        ),
                    );
                }
            });
        if chosen != editor.role {
            editor.select_role(settings, &chosen);
        }
        if settings.can_manage_roles && ui.button("Crear rol").clicked() {
            editor.select_role(settings, "");
        }
        let role = settings.roles.iter().find(|role| role.id == editor.role);
        if role.is_none() && editor.role_name.is_empty() {
            return;
        }
        let editable = role
            .map(|role| role.editable)
            .unwrap_or(settings.can_manage_roles);
        if !editable {
            ui.label(
                "Este rol es de una integración o está fuera de tu jerarquía de administración.",
            );
        }
        ui.add_enabled_ui(editable,|ui|{
            ui.label("Nombre del rol");ui.add(bordered(egui::TextEdit::singleline(&mut editor.role_name).char_limit(100)));
            ui.label("Color RGB (6 dígitos hexadecimales)");ui.add(bordered(egui::TextEdit::singleline(&mut editor.role_color).char_limit(6)));
            ui.checkbox(&mut editor.role_hoist,"Mostrar por separado en la lista de miembros");ui.checkbox(&mut editor.role_mentionable,"Permitir mencionar este rol");
            ui.collapsing("Permisos del rol",|ui|permission_checks(ui,&mut editor.role_bits,settings.effective_permissions));
            ui.horizontal(|ui|{
                if ui.button(if editor.role.is_empty(){"Crear este rol"}else{"Guardar rol"}).clicked(){
                    if let Some(color)=u32::from_str_radix(editor.role_color.trim_start_matches('#'),16).ok().filter(|color|*color<=0xffffff){
                        self.guild_write(&settings.guild_id,if editor.role.is_empty(){"role-create"}else{"role"},&editor.role,json!({"name":editor.role_name.trim(),"color":color,"hoist":editor.role_hoist,"mentionable":editor.role_mentionable,"permissions":editor.role_bits.to_string()}));
                    }else{self.error="El color debe contener 6 dígitos hexadecimales.".into();}
                }
                if !editor.role.is_empty()&&editor.role!=settings.guild_id&&ui.button("Eliminar rol…").clicked(){self.confirm("Eliminar rol",&format!("¿Eliminar el rol {}? Sus asignaciones y permisos se perderán.",editor.role_name),write_action(&settings.guild_id,"role-delete",&editor.role,json!({})));}
            });
        });
    }
    fn guild_channels_editor(
        &mut self,
        ui: &mut egui::Ui,
        settings: &GuildSettings,
        editor: &mut Editor,
    ) {
        let selected = settings
            .channels
            .iter()
            .find(|channel| channel.id == editor.channel)
            .map(|channel| channel.name.as_str())
            .unwrap_or("Selecciona un canal");
        let mut chosen = editor.channel.clone();
        egui::ComboBox::from_id_salt("guild-channel")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for channel in &settings.channels {
                    if matches!(channel.kind, 0 | 2 | 4 | 5) {
                        ui.selectable_value(&mut chosen, channel.id.clone(), &channel.name);
                    }
                }
            });
        if chosen != editor.channel {
            editor.select_channel(settings, &chosen);
        }
        if settings.can_manage_channels && ui.button("Crear canal").clicked() {
            editor.select_channel(settings, "");
        }
        let channel = settings
            .channels
            .iter()
            .find(|channel| channel.id == editor.channel);
        if channel.is_none() && editor.channel_name.is_empty() {
            return;
        }
        let editable = channel
            .map(|channel| channel.editable)
            .unwrap_or(settings.can_manage_channels);
        if !editable {
            ui.label("Necesitas Gestionar canales en este canal para modificarlo.");
        }
        ui.add_enabled_ui(editable, |ui| {
            ui.label("Nombre del canal");
            ui.add(bordered(
                egui::TextEdit::singleline(&mut editor.channel_name).char_limit(100),
            ));
            if channel.is_none() {
                egui::ComboBox::from_label("Tipo")
                    .selected_text(match editor.channel_kind {
                        2 => "Voz",
                        4 => "Categoría",
                        _ => "Texto",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut editor.channel_kind, 0, "Texto");
                        ui.selectable_value(&mut editor.channel_kind, 2, "Voz");
                        ui.selectable_value(&mut editor.channel_kind, 4, "Categoría");
                    });
            }
            if editor.channel_kind != 4 {
                egui::ComboBox::from_label("Categoría")
                    .selected_text(
                        settings
                            .channels
                            .iter()
                            .find(|channel| channel.id == editor.channel_parent)
                            .map(|channel| channel.name.as_str())
                            .unwrap_or("Sin categoría"),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut editor.channel_parent,
                            String::new(),
                            "Sin categoría",
                        );
                        for category in settings.channels.iter().filter(|channel| channel.kind == 4)
                        {
                            ui.selectable_value(
                                &mut editor.channel_parent,
                                category.id.clone(),
                                &category.name,
                            );
                        }
                    });
            }
            if matches!(editor.channel_kind, 0 | 5) {
                ui.label("Tema del canal");
                ui.add(bordered(
                    egui::TextEdit::multiline(&mut editor.channel_topic)
                        .char_limit(1024)
                        .desired_width(600.)
                        .desired_rows(3),
                ));
                ui.checkbox(&mut editor.channel_nsfw, "Canal con restricción de edad");
                ui.add(
                    egui::Slider::new(&mut editor.channel_slowmode, 0..=21600)
                        .text("Modo lento · segundos"),
                );
            }
            if editor.channel_kind == 2 {
                ui.add(
                    egui::Slider::new(&mut editor.channel_limit, 0..=99)
                        .text("Límite de participantes (0 sin límite)"),
                );
                ui.add(
                    egui::Slider::new(&mut editor.channel_bitrate, 8000..=96000)
                        .text("Tasa de audio · bits/s"),
                );
            }
            ui.horizontal(|ui| {
                if ui
                    .button(if channel.is_none() {
                        "Crear este canal"
                    } else {
                        "Guardar canal"
                    })
                    .clicked()
                {
                    let mut body = json!({"name":editor.channel_name.trim()});
                    if channel.is_none() {
                        body["type"] = json!(editor.channel_kind);
                    }
                    if editor.channel_kind != 4 {
                        body["parent_id"] = if editor.channel_parent.is_empty() {
                            Value::Null
                        } else {
                            json!(editor.channel_parent)
                        };
                    }
                    if matches!(editor.channel_kind, 0 | 5) {
                        body["topic"] = json!(editor.channel_topic);
                        body["nsfw"] = json!(editor.channel_nsfw);
                        body["rate_limit_per_user"] = json!(editor.channel_slowmode);
                    }
                    if editor.channel_kind == 2 {
                        body["user_limit"] = json!(editor.channel_limit);
                        body["bitrate"] = json!(editor.channel_bitrate);
                    }
                    self.guild_write(
                        &settings.guild_id,
                        if channel.is_none() {
                            "channel-create"
                        } else {
                            "channel"
                        },
                        &editor.channel,
                        body,
                    );
                }
                if channel.is_some() && ui.button("Eliminar canal…").clicked() {
                    self.confirm(
                        "Eliminar canal",
                        &format!(
                            "¿Eliminar {} y su historial? Esta acción no se puede deshacer.",
                            editor.channel_name
                        ),
                        write_action(
                            &settings.guild_id,
                            "channel-delete",
                            &editor.channel,
                            json!({}),
                        ),
                    );
                }
            });
        });
        if let Some(channel) = channel {
            ui.separator();
            ui.heading("Permisos del canal");
            if !channel.permissions_editable {
                ui.label(
                    "Necesitas Gestionar roles en este canal para modificar sus sobrescrituras.",
                );
            }
            let mut role = editor.overwrite_role.clone();
            egui::ComboBox::from_label("Rol")
                .selected_text(
                    settings
                        .roles
                        .iter()
                        .find(|r| r.id == role)
                        .map(|r| r.name.as_str())
                        .unwrap_or("Selecciona un rol"),
                )
                .show_ui(ui, |ui| {
                    for entry in &settings.roles {
                        ui.selectable_value(&mut role, entry.id.clone(), &entry.name);
                    }
                });
            editor.overwrite_role = role;
            let overwrite_key = format!("{}:{}", editor.channel, editor.overwrite_role);
            if editor.overwrite_key != overwrite_key {
                editor.overwrite_key = overwrite_key;
                let overwrite = channel
                    .permission_overwrites
                    .iter()
                    .find(|entry| entry.id == editor.overwrite_role && entry.kind == 0);
                editor.overwrite_allow = overwrite.map(|entry| entry.allow).unwrap_or(0);
                editor.overwrite_deny = overwrite.map(|entry| entry.deny).unwrap_or(0);
            }
            ui.add_enabled_ui(channel.permissions_editable&&!editor.overwrite_role.is_empty(),|ui|{
                ui.label("Heredar conserva los permisos del servidor; denegar o permitir crea una sobrescritura para este canal.");
                for &(bit,label) in PERMISSIONS{
                    if bit==8{continue;}
                    let mut value=if editor.overwrite_allow & bit!=0 {1}else if editor.overwrite_deny & bit!=0 {-1}else{0};
                    let old=value;
                    ui.horizontal(|ui|{ui.label(label);ui.selectable_value(&mut value,-1,"Denegar");ui.selectable_value(&mut value,0,"Heredar");ui.selectable_value(&mut value,1,"Permitir");});
                    if value!=old{editor.overwrite_allow&=!bit;editor.overwrite_deny&=!bit;if value==1{editor.overwrite_allow|=bit;}else if value== -1{editor.overwrite_deny|=bit;}}
                }
                ui.horizontal(|ui|{
                    if ui.button("Guardar permisos del canal").clicked(){self.guild_write(&settings.guild_id,"overwrite",&editor.overwrite_role,json!({"channel_id":editor.channel,"type":0,"allow":editor.overwrite_allow.to_string(),"deny":editor.overwrite_deny.to_string()}));}
                    if ui.button("Eliminar sobrescritura…").clicked(){self.confirm("Heredar permisos del servidor","¿Eliminar la sobrescritura de este rol en el canal?",write_action(&settings.guild_id,"overwrite-delete",&editor.overwrite_role,json!({"channel_id":editor.channel})));}
                });
            });
        }
    }
    fn guild_nickname_editor(
        &mut self,
        ui: &mut egui::Ui,
        state: &AppSnapshot,
        settings: &GuildSettings,
        editor: &mut Editor,
        me: &str,
    ) {
        ui.label(
            "Apodo visible únicamente en este servidor. Déjalo vacío para volver al nombre global.",
        );
        let own = editor.nickname_user == me;
        let members = if state.selected_guild_id == settings.guild_id {
            state.guild_members.as_slice()
        } else {
            &[]
        };
        if settings.can_manage_nicknames {
            let old = editor.nickname_user.clone();
            egui::ComboBox::from_label("Persona")
                .selected_text(if own {
                    "Mi apodo"
                } else {
                    members
                        .iter()
                        .find(|member| member.user.id == editor.nickname_user)
                        .map(|member| guild_settings::visible_name(&member.user, &member.nickname))
                        .unwrap_or("Otra persona")
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut editor.nickname_user, me.to_owned(), "Mi apodo");
                    for member in members.iter().filter(|member| member.user.id != me) {
                        ui.selectable_value(
                            &mut editor.nickname_user,
                            member.user.id.clone(),
                            guild_settings::visible_name(&member.user, &member.nickname),
                        );
                    }
                    for member in state.guild_voice_members.iter().filter(|member| {
                        member.guild_id == settings.guild_id
                            && member.user.id != me
                            && !members.iter().any(|known| known.user.id == member.user.id)
                    }) {
                        ui.selectable_value(
                            &mut editor.nickname_user,
                            member.user.id.clone(),
                            guild_settings::visible_name(&member.user, &member.nickname),
                        );
                    }
                });
            ui.label("ID de otra persona del servidor");
            ui.add(bordered(
                egui::TextEdit::singleline(&mut editor.nickname_user).char_limit(24),
            ));
            if old != editor.nickname_user {
                editor.nickname = if editor.nickname_user == me {
                    settings.own_nickname.clone()
                } else {
                    members
                        .iter()
                        .find(|member| member.user.id == editor.nickname_user)
                        .map(|member| member.nickname.clone())
                        .or_else(|| {
                            state
                                .guild_voice_members
                                .iter()
                                .find(|member| {
                                    member.guild_id == settings.guild_id
                                        && member.user.id == editor.nickname_user
                                })
                                .map(|member| member.nickname.clone())
                        })
                        .unwrap_or_default()
                };
            }
        }
        let can_edit = if editor.nickname_user == me {
            settings.can_change_nickname
        } else {
            settings.can_manage_nicknames
        };
        if !can_edit {
            ui.label("No tienes permiso para cambiar este apodo.");
        }
        ui.add_enabled_ui(can_edit, |ui| {
            ui.label("Apodo");
            ui.add(bordered(
                egui::TextEdit::singleline(&mut editor.nickname).char_limit(32),
            ));
            if ui.button("Guardar apodo").clicked() {
                self.guild_write(
                    &settings.guild_id,
                    "nickname",
                    &editor.nickname_user,
                    json!({"nick":editor.nickname.trim()}),
                );
            }
        });
        ui.small("Al guardar se comprueban de nuevo la membresía y la jerarquía de roles.");
    }
    pub(super) fn draw_guild_voice_members(
        &mut self,
        ui: &mut egui::Ui,
        channel: &NavItem,
        state: &AppSnapshot,
    ) {
        if !matches!(channel.kind.as_str(),"voice"|"stage") || state.selected_guild_id.is_empty() {
            return;
        }
        for member in state.guild_voice_members.iter().filter(|member| {
            member.guild_id == state.selected_guild_id && member.channel_id == channel.id
        }) {
            let game=super::shell::guild_voice_game(state,member);
            let height=super::shell::guild_voice_member_height(state,member);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(),height), egui::Sense::click());
            let response=response.on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.hovered(){ui.painter().rect_filled(rect,4.,self.theme.surface);}
            let avatar=egui::Rect::from_min_size(rect.left_top()+egui::vec2(34.,5.),egui::vec2(22.,22.));
            self.pictures.paint_avatar(ui,avatar,&member.user);
            let active=state.call_guild_id==member.guild_id&&state.call_channel_id==member.channel_id&&(state.voice_connected||state.voice_connecting);
            let participant=active.then(||state.call_participants.iter().find(|person|person.user.id==member.user.id)).flatten();
            if participant.is_some_and(|person|person.speaking)&&state.voice_connected {
                ui.painter().circle_stroke(avatar.center(),12.,egui::Stroke::new(1.5_f32,Color32::from_rgb(35,165,90)));
            }
            let right=super::shell::draw_member_voice_flags(ui,rect,state,&member.guild_id,&member.user.id,Some(&member.channel_id));
            let left=avatar.right()+8.;
            let name_rect=egui::Rect::from_min_max(egui::pos2(left,rect.top()+3.),egui::pos2(right.max(left),rect.top()+25.));
            let name=guild_settings::visible_name(&member.user,&member.nickname);
            super::shell::fitted_text(ui,name_rect,name,14.,self.theme.ink);
            theme::fitted_link_feedback(ui,&response,name_rect,name,14.,self.theme.ink);
            if !game.is_empty(){
                super::shell::fitted_text(ui,egui::Rect::from_min_max(egui::pos2(left,rect.top()+26.),egui::pos2(rect.right()-5.,rect.bottom()-3.)),&game,11.,self.theme.muted);
            }
            if response.clicked() {
                self.send(
                    "profile",
                    &member.user.id,
                    "",
                    "",
                    &state.selected_channel_id,
                );
            }
            response.context_menu(|ui| self.people_menu(ui, &member.user, state));
            ui.advance_cursor_after_rect(rect);
        }
    }
}
