use super::*;
use eframe::egui::{self, Color32, RichText};
use serde_json::json;
pub(super) fn call_panel_visible(state:&AppSnapshot)->bool {
    if !(state.voice_connected||state.voice_connecting) || state.call_channel_id.is_empty() {return false;}
    if state.call_guild_id.is_empty() {
        state.selected_guild_id.is_empty() && state.selected_channel_id==state.call_channel_id
    } else { state.selected_guild_id==state.call_guild_id }
}
fn call_chat_scope(state:&AppSnapshot)->Option<egui::Id> {
    let channel_has_call_chat=if state.call_guild_id.is_empty() {
        state.selected_guild_id.is_empty() && matches!(state.selected_channel_kind.as_str(),"dm"|"group")
    } else {
        state.selected_channel_kind=="voice" && state.selected_guild_id==state.call_guild_id
    };
    (state.view=="chat" && channel_has_call_chat && state.selected_channel_id==state.call_channel_id
        && call_panel_visible(state)).then(||egui::Id::new(("call-chat",state.notification_generation,
            state.current_user.as_ref().map(|user|user.id.as_str()),&state.call_guild_id,&state.call_channel_id,state.call_instance_id)))
}
pub(super) fn call_chat_hidden(ctx:&egui::Context,state:&AppSnapshot)->bool {
    call_chat_scope(state).is_some_and(|scope| !ctx.data(|data|
        data.get_temp::<(egui::Id,bool)>(egui::Id::new("call-chat-visibility")))
        .is_some_and(|(stored,shown)|stored==scope&&shown))
}
fn media_selector_window<'a>(ctx:&egui::Context,title:&str,id:&str)->egui::Window<'a> {
    let width=(ctx.content_rect().width()-40.).clamp(1.,604.);
    // Both selectors use the same width and shrink to their actual contents.
    // Keep the viewport height limit and scrolling for small windows/large text.
    dialogs::centered_dialog(ctx,title,id,egui::vec2(620.,650.))
        .auto_sized().min_width(width).max_width(width).vscroll(true)
}
fn media_selector_footer(ui:&mut egui::Ui,label:&str,enabled:bool)->(bool,bool) {
    ui.separator();
    let mut confirm=false;let mut cancel=false;
    ui.horizontal_wrapped(|ui| {
        confirm=ui.add_enabled(enabled,egui::Button::new(label)).clicked();
        cancel=ui.button("Cancelar").clicked();
    });
    (confirm,cancel)
}
impl NotDiscordUi {
    pub(super) fn draw_call(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        self.draw_call_in_height(ui, state, (ui.available_height() * 0.52).max(112.));
    }
    pub(super) fn draw_call_in_height(
        &mut self,
        ui: &mut egui::Ui,
        state: &AppSnapshot,
        max_height: f32,
    ) {
        let mic = ui
            .ctx()
            .data(|data| data.get_temp::<Value>(egui::Id::new("mic-test-state")))
            .unwrap_or(Value::Null);
        let testing = mic["active"] == true || mic["pending"] == true;
        let background = call_background(self.theme.base);
        let ink = call_ink(background);
        let budget = max_height.max(28.);
        let compact = budget < 230.;
        egui::Frame::new().fill(background).corner_radius(8).inner_margin(egui::Margin::same(14)).show(ui,|ui| {
            ui.set_width(ui.available_width());
            let width=ui.available_width();
            let gap=ui.spacing().item_spacing.y;
            let control_height=call_controls_height(ui,width);
            let chat_scope=call_chat_scope(state);
            let title_height=(if compact {ui.fonts_mut(|fonts|fonts.row_height(&egui::FontId::proportional(14.)))}
                else {ui.text_style_height(&egui::TextStyle::Heading)}).max(if chat_scope.is_some(){26.}else{0.});
            let mut header_height=title_height+gap;
            if !compact {header_height+=ui.text_style_height(&egui::TextStyle::Body)+gap;}
            if state.call_guild_id.is_empty()&&!state.call_invite_candidates.is_empty() {
                header_height+=call_tile_button_height(ui,width,"Miembros de la conversación")+gap;
            }
            let testing_notice="Prueba local de micrófono · la llamada está ensordecida y no recibe tu audio de prueba.";
            let preparing_notice="Selección en espera de cifrado · vista previa local, no se envía hasta tener cifrado. Puedes cancelarla.";
            if testing {header_height+=call_tile_text_height(ui,width,testing_notice,&egui::TextStyle::Body)+gap;}
            let preparing=!state.voice_encryption_ready&&(state.camera_preparing||state.screen_preparing);
            if preparing {header_height+=call_tile_text_height(ui,width,preparing_notice,&egui::TextStyle::Body)+gap;}
            let tile_count=call_gallery_tile_count(state);
            let minimum_height=header_height+control_height+gap+if tile_count==0 {0.}else{12.};
            let inner_height=if tile_count==0 {minimum_height}else{(budget-28.).max(minimum_height)};
            let (panel_rect,_)=ui.allocate_exact_size(egui::vec2(width,inner_height),egui::Sense::hover());
            let footer_rect=egui::Rect::from_min_max(egui::pos2(panel_rect.left(),panel_rect.bottom()-control_height),panel_rect.max);
            let content_rect=egui::Rect::from_min_max(panel_rect.min,egui::pos2(panel_rect.right(),(footer_rect.top()-gap).max(panel_rect.top())));
            // The footer has its own allocated rectangle and never belongs to
            // a scroll area. Participant rows cannot move or paint over it.
            let mut content=ui.new_child(egui::UiBuilder::new().id_salt("call-content").max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)));
            content.set_clip_rect(content_rect.intersect(ui.clip_rect()));
            {
                let ui=&mut content;
                let duration=state.call_started_at.map(|start| {
                    let duration=chrono::Utc::now().signed_duration_since(start);
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
                    format!(" · {:02}:{:02}",duration.num_minutes().max(0),duration.num_seconds().max(0)%60)
                }).unwrap_or_default();
                let title=RichText::new(format!("Llamada · {}{}",state.call_channel_name,if compact {duration.as_str()}else{""})).color(ink);
                let title=if compact {title.size(14.).strong()}else{title.heading()};
                ui.horizontal(|ui| {
                    let label_width=(ui.available_width()-if chat_scope.is_some(){26.+ui.spacing().item_spacing.x}else{0.}).max(1.);
                    ui.add_sized([label_width,title_height],egui::Label::new(title).truncate()).on_hover_text(format!("{}{duration}",state.call_status_label));
                    if let Some(scope)=chat_scope {
                        let hidden=call_chat_hidden(ui.ctx(),state);
                        let label=if hidden {"Mostrar chat"}else{"Ocultar chat"};
                        let toggle=super::shell::icon_button(ui,super::shell::Icon::Chat,26.,false,hidden).on_hover_text(label);
                        toggle.widget_info(||egui::WidgetInfo::labeled(egui::WidgetType::Button,true,label));
                        if toggle.clicked() {
                            ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("call-chat-visibility"),(scope,hidden)));
                            if !hidden {super::chat::suspend_interactions(ui.ctx());}
                            ui.ctx().request_repaint();
                        }
                    }
                });
                if !compact {ui.add(egui::Label::new(RichText::new(format!("{}{duration}",state.call_status_label)).color(ink)).truncate());}
                self.draw_call_invite_menu(ui, state);
                if testing {ui.add(egui::Label::new(RichText::new(testing_notice).color(ink)).wrap());}
                if preparing {ui.add(egui::Label::new(RichText::new(preparing_notice).color(ink)).wrap());}
                let body_height=ui.available_height().max(0.);
                let columns=call_gallery_columns(width,ui.spacing().item_spacing.x,tile_count,false);
                let rows=tile_count.div_ceil(columns);
                let account=state.current_user.as_ref().map(|user|user.id.as_str()).unwrap_or("");
                let gallery_salt=("call-gallery",account,state.call_channel_id.as_str(),state.call_instance_id);
                let expanded=ui.ctx().data(|data|data.get_temp::<CallExpansion>(egui::Id::new("call-expanded-video")))
                    .filter(|expanded|state.call_videos.iter().any(|video|expanded.matches(state,video)))
                    .map(|expanded|expanded.id);
                let layout_scope=egui::Id::new((gallery_salt,columns,rows,expanded));
                let reset_scroll=ui.ctx().data_mut(|data| {
                    let key=egui::Id::new("call-gallery-layout-scope");
                    let previous=data.get_temp::<egui::Id>(key);
                    data.insert_temp(key,layout_scope);
                    previous!=Some(layout_scope)
                });
                if reset_scroll {
                    // Clear targets and momentum too: a previous call or row
                    // count must not animate this gallery back to an old offset.
                    let id=ui.make_persistent_id(egui::Id::new(gallery_salt));
                    egui::scroll_area::State::default().store(ui.ctx(),id);
                }
                let gallery=theme::vertical_scroll(ui).id_salt(gallery_salt).auto_shrink([false,true]).stick_to_bottom(false)
                    .min_scrolled_height(0.).max_height(body_height).show(ui,|ui| {
                    if !self.draw_expanded_call_video(ui,state,ink,body_height) {
                        let row_height=(body_height-if state.screen_paused {ui.text_style_height(&egui::TextStyle::Body)+ui.spacing().item_spacing.y}else{0.}).max(4.);
                        self.draw_participant_tiles_in_height(ui,state,ink,240.,compact,Some(row_height));
                    }
                    if state.screen_paused {ui.colored_label(ink,"Pantalla en pausa por conexión cifrada");}
                });
                #[cfg(test)] ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("call-gallery-layout"),
                    (columns,tile_count,gallery.content_size.y>gallery.inner_rect.height()+0.5,gallery.inner_rect,gallery.content_size,gallery.state.offset)));
                #[cfg(not(test))] let _=(columns,gallery);
            }
            let mut footer=ui.new_child(egui::UiBuilder::new().id_salt("call-footer").max_rect(footer_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)));
            footer.set_clip_rect(footer_rect.intersect(ui.clip_rect()));
            self.draw_call_controls(&mut footer,state,testing,false);
            #[cfg(test)] ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("call-panel-overflow-layout"),
                (panel_rect,panel_rect.size(),egui::Vec2::ZERO)));
        });
        ui.add_space(8.);
    }
    fn draw_expanded_call_video(&mut self, ui: &mut egui::Ui, state: &AppSnapshot, ink: Color32, height: f32) -> bool {
        let key = egui::Id::new("call-expanded-video");
        let Some(mut expanded) = ui.ctx().data(|data|data.get_temp::<CallExpansion>(key)) else { return false; };
        let Some(video) = state.call_videos.iter().find(|video| expanded.matches(state, video)) else {
            ui.ctx().data_mut(|data|data.remove::<CallExpansion>(key));
            return false;
        };
        let Some(texture) = self.video_texture(video.id,ui.ctx()) else {
            ui.ctx().data_mut(|data|data.remove::<CallExpansion>(key));
            return false;
        };
        let mut leave = false;
        let mut fit = false;
        ui.horizontal_wrapped(|ui| {
            leave = ui.button("Volver a participantes").clicked();
            fit = ui.button("Ajustar").clicked();
            ui.add(egui::Label::new(RichText::new(format!("{} · {}",video.label,if video.kind=="screen" {"Pantalla"} else {"Cámara"})).color(ink)).truncate());
        });
        if leave || ui.is_enabled() && !egui::Popup::is_any_open(ui.ctx()) && ui.input(|input|input.key_pressed(egui::Key::Escape)) {
            ui.ctx().data_mut(|data|data.remove::<CallExpansion>(key));
            self.send("video-quality",video.id.to_string(),"false","","");
            return false;
        }
        let extent = egui::vec2(ui.available_width().max(1.), (height-call_button_height(ui)-ui.spacing().item_spacing.y).max(20.));
        let (canvas,response) = ui.allocate_exact_size(extent,egui::Sense::click_and_drag());
        let painter = ui.painter().with_clip_rect(canvas.intersect(ui.clip_rect()));
        painter.rect_filled(canvas,4.,call_background(self.theme.base));
        let natural = texture.size_vec2();
        let mut viewer = Viewer { url:String::new(),local_path:None,zoom:expanded.zoom,pan:expanded.pan };
        if fit {viewer.zoom=1.;viewer.pan=egui::Vec2::ZERO;}
        let popup_was_open=egui::Popup::is_any_open(ui.ctx());
        if response.hovered() && !popup_was_open {
            let scroll = ui.input_mut(|input| {
                let scroll = input.raw_scroll_delta.y;
                // The gallery's outer scroll must not also consume a zoom gesture.
                input.raw_scroll_delta=egui::Vec2::ZERO;
                input.smooth_scroll_delta=egui::Vec2::ZERO;
                scroll
            });
            if scroll!=0. {
                let anchor=ui.input(|input|input.pointer.hover_pos()).unwrap_or(canvas.center())-canvas.center();
                let requested=viewer.zoom*(scroll*0.002).exp();
                dialogs::zoom_viewer(&mut viewer,natural,canvas.size(),requested,anchor);
            }
        }
        if response.dragged_by(egui::PointerButton::Primary) && !popup_was_open {viewer.pan+=ui.input(|input|input.pointer.delta());}
        dialogs::clamp_viewer_pan(&mut viewer,natural,canvas.size());
        let scale=dialogs::viewer_fit(natural,canvas.size())*viewer.zoom;
        painter.image(texture.id(),egui::Rect::from_center_size(canvas.center()+viewer.pan,natural*scale),
            egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),Color32::WHITE);
        painter.text(canvas.left_bottom()+egui::vec2(8.,-8.),egui::Align2::LEFT_BOTTOM,
            format!("{:.0} % · rueda para ampliar · arrastra para desplazar",scale*100.),egui::FontId::proportional(12.),ink);
        if video.kind=="screen" {
            response.context_menu(|ui| {
                if let Some(participant)=state.call_participants.iter().find(|participant|participant.user.id==video.user_id) {
                    self.draw_call_participant_menu(ui,participant,state,&state.call_channel_id,state.call_instance_id,false);
                } else {self.draw_stream_volume_control(ui,state);}
            });
        }
        let collapse = response.clicked_by(egui::PointerButton::Primary) && !popup_was_open && !egui::Popup::is_any_open(ui.ctx());
        response.on_hover_cursor(if viewer.zoom>1. {egui::CursorIcon::Grab}else{egui::CursorIcon::ZoomIn});
        expanded.zoom=viewer.zoom; expanded.pan=viewer.pan;
        #[cfg(test)] ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("call-expanded-canvas"),canvas));
        if collapse {
            ui.ctx().data_mut(|data|data.remove::<CallExpansion>(key));
            self.send("video-quality",video.id.to_string(),"false","","");
            ui.ctx().request_repaint();
        } else { ui.ctx().data_mut(|data|data.insert_temp(key,expanded)); }
        true
    }
    fn draw_call_controls(
        &mut self,
        ui: &mut egui::Ui,
        state: &AppSnapshot,
        testing: bool,
        one_row: bool,
    ) {
        let width = ui.available_width();
        let button_height = call_button_height(ui);
        let permissions = ui.ctx().data(|data| data.get_temp::<Value>(egui::Id::new("voice-permissions"))).unwrap_or(Value::Null);
        let may_speak = permissions["speak"].as_bool().unwrap_or(true);
        let may_video = permissions["video"].as_bool().unwrap_or(true);
        let controls = [
            (
                if state.self_mute {
                    "Activar micrófono"
                } else {
                    "Silenciar micrófono"
                },
                if state.self_mute {
                    "Activar mic."
                } else {
                    "Silenciar"
                },
                state.voice_connected && !testing && (state.self_mute || may_speak),
                "toggle-mute",
            ),
            (
                if state.self_deaf {
                    "Reactivar audio"
                } else {
                    "Ensordecer"
                },
                if state.self_deaf {
                    "Activar audio"
                } else {
                    "Ensordecer"
                },
                state.voice_connected && !testing,
                "toggle-deafen",
            ),
            (
                if state.screen_paused {
                    "Cancelar pantalla en pausa"
                } else if state.screen_sharing || state.screen_preparing {
                    "Dejar de compartir"
                } else {
                    "Compartir pantalla"
                },
                if state.screen_sharing || state.screen_preparing {
                    "Cerrar pantalla"
                } else {
                    "Pantalla"
                },
                !testing
                    && (state.screen_sharing || state.screen_preparing || state.voice_connected && may_video),
                "share",
            ),
            (
                if state.camera_enabled {
                    "Apagar cámara"
                } else if state.camera_preparing {
                    "Cancelar cámara en preparación"
                } else {
                    "Encender cámara"
                },
                if state.camera_enabled || state.camera_preparing {
                    "Cerrar cámara"
                } else {
                    "Cámara"
                },
                state.camera_enabled || state.camera_preparing || state.voice_connected && may_video,
                "toggle-camera",
            ),
            ("Salir de la llamada", "Salir", true, "disconnect-voice"),
        ];
        if one_row {
            theme::horizontal_scroll(ui)
                .id_salt("call-controls-horizontal")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (label, short, enabled, action) in controls {
                            let text = RichText::new(short);
                            let text = if action == "disconnect-voice" {
                                text.color(Color32::from_rgb(242, 63, 67))
                            } else {
                                text
                            };
                            if ui
                                .add_enabled(
                                    enabled,
                                    egui::Button::new(text).min_size(egui::vec2(0., button_height)),
                                )
                                .on_hover_text(label)
                                .clicked()
                            {
                                self.simple(action);
                            }
                        }
                    });
                });
        } else {
            let columns = ((width + ui.spacing().item_spacing.x) / 96.)
                .floor()
                .clamp(1., 5.) as usize;
            let cell = (((width - ui.spacing().item_spacing.x * (columns - 1) as f32)
                / columns as f32)
                - 1.)
                .floor()
                .max(1.);
            egui::Grid::new("call-controls")
                .num_columns(columns)
                .min_col_width(cell)
                .max_col_width(cell)
                .spacing(ui.spacing().item_spacing)
                .show(ui, |ui| {
                    for (index, (label, short, enabled, action)) in controls.into_iter().enumerate()
                    {
                        let text = RichText::new(if width < 720. { short } else { label });
                        let text = if action == "disconnect-voice" {
                            text.color(Color32::from_rgb(242, 63, 67))
                        } else {
                            text
                        };
                        if ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(text)
                                    .truncate()
                                    .min_size(egui::vec2(cell, button_height)),
                            )
                            .on_hover_text(label)
                            .clicked()
                        {
                            self.simple(action);
                        }
                        if (index + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        }
    }
    pub(super) fn draw_media_settings(&mut self, ctx: &egui::Context, state: &AppSnapshot) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("mic-test-dialog-visible"), true));
        let mic = ctx
            .data(|data| data.get_temp::<Value>(egui::Id::new("mic-test-state")))
            .unwrap_or(Value::Null);
        let active = mic["active"] == true;
        let pending = mic["pending"] == true;
        let loading = ctx.data(|data| data.get_temp::<bool>(egui::Id::new("devices-loading"))).unwrap_or(false);
        let now = Instant::now();
        let refresh = ctx.data(|data| data.get_temp::<Instant>(egui::Id::new("devices-refresh-at")));
        if refresh.is_none() || refresh.is_some_and(|next| now >= next) && !loading {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new("devices-refresh-at"), now + Duration::from_secs(5)));
            if refresh.is_some() { self.simple("list-devices"); }
        }
        ctx.request_repaint_after(Duration::from_secs(1));
        let mut open = true;
        dialogs::centered_dialog(ctx,"Voz, vídeo y dispositivos","media-settings",egui::vec2(680.,650.)).open(&mut open).collapsible(false).show(ctx,|ui| {
            ui.heading("Voz");
            ui.horizontal(|ui| {
                if ui.add_enabled(!loading, egui::Button::new("Actualizar dispositivos y fuentes")).clicked() {
                    self.simple("list-devices");
                    ctx.data_mut(|data| data.insert_temp(egui::Id::new("devices-refresh-at"), Instant::now() + Duration::from_secs(5)));
                }
                if loading { ui.spinner(); ui.label("Cargando dispositivos…"); }
            });
            ui.separator();
            ui.strong("Dispositivos de audio");
            let mut settings=self.media_settings.clone();
            if !settings.is_object() {settings=serde_json::json!({});}
            for (key,label,inventory) in [("InputId","Micrófono","input"),("OutputId","Altavoces","output")] {
                ui.label(label);let current=s(&settings,key).to_string();let mut chosen=current.clone();
                let list=field(&self.devices,inventory).and_then(Value::as_array).cloned().unwrap_or_default();
                let default=if key=="CameraId" {"Seleccionar al activar"} else {"Windows predeterminado"};
                let name=list.iter().find(|d|s(d,"id")==current).map(|d|s(d,"name")).unwrap_or(if current.is_empty()||current=="default" {default} else {&current});
                egui::ComboBox::from_id_salt(("media-device",key)).selected_text(name).width(ui.available_width().min(500.)).show_ui(ui,|ui| {
                    ui.selectable_value(&mut chosen,if key=="CameraId" {String::new()} else {"default".into()},default);
                    for device in &list {ui.selectable_value(&mut chosen,s(device,"id").into(),s(device,"name"));}
                });
                if chosen!=current {if key=="CameraId" {if chosen.is_empty(){self.stop_selection_preview();}else{self.select_media_preview("camera",&chosen);}}settings[key]=Value::String(chosen);}
                self.draw_inventory_status(ui, inventory, loading);
            }
            ui.separator();
            ui.strong("Volumen");
            for (key,label,max) in [("InputVolume","Volumen del micrófono",200),("OutputVolume","Volumen de salida",400)] {
                let mut volume=field(&settings,key).and_then(Value::as_i64).unwrap_or(100) as i32;
                if ui.add(egui::Slider::new(&mut volume,0..=max).text(label).suffix(" %")).changed() {settings[key]=Value::from(volume);}
            }
            ui.separator();ui.strong("Sensibilidad de entrada · manual");
            let mut threshold=field(&settings,"InputSensitivityDb").and_then(Value::as_i64).unwrap_or(-40).clamp(-100,0) as i32;
            let response=ui.add(egui::Slider::new(&mut threshold,-100..=0).text("Umbral mínimo").suffix(" dB"));
            if response.changed() {settings["InputSensitivityDb"]=Value::from(threshold);}
            let db=mic["db"].as_f64().filter(|value|value.is_finite()).unwrap_or(-100.).clamp(-100.,0.) as f32;
            let meter=ui.add(egui::ProgressBar::new((db+100.)/100.).fill(Color32::from_rgb(35,165,90)).text(format!("Entrada: {db:.0} dB · umbral: {threshold} dB")));
            ui.painter().rect_stroke(meter.rect, meter.rect.height()/2., egui::Stroke::new(1.0_f32,ui.visuals().window_stroke.color), egui::StrokeKind::Inside);
            let threshold_x=meter.rect.left()+meter.rect.width()*(threshold as f32+100.)/100.;
            ui.painter().line_segment([egui::pos2(threshold_x,meter.rect.top()),egui::pos2(threshold_x,meter.rect.bottom())],egui::Stroke::new(2.0_f32,Color32::from_rgb(250,190,70)));
            ui.small("Más cerca de 0 dB exige más volumen. El medidor usa la señal real; en reposo pulsa Probar para abrir el micrófono.");
            ui.separator();ui.strong("Prueba local de micrófono");
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!active&&!pending&&!loading,egui::Button::new("Probar micrófono (30 s máximo)")).clicked() {self.send("test-mic","",settings.to_string(),"","");}
                if ui.add_enabled(active||pending,egui::Button::new("Detener prueba")).clicked() {self.simple("stop-mic-test");}
            });
            if active||pending {ui.colored_label(self.theme.accent,if pending {"Preparando prueba local…"} else {"Escucha tu micrófono en el dispositivo de salida seleccionado"});}
            if let Some(error) = mic["error"].as_str().filter(|error| !error.is_empty()) {
                ui.colored_label(Color32::from_rgb(242,63,67), error);
            }
            ui.small("La prueba es local. En llamada se fuerza silencio y ensordecimiento temporal; se detiene la pantalla para no reenviar el loopback. Al detener o cerrar se restaura tu estado anterior.");
            ui.separator();ui.strong("Procesamiento de audio");
            for (key,label) in [("NoiseSuppression","Supresión de ruido estándar"),("EchoCancellation","Cancelación de eco")] {
                let mut value=field(&settings,key).and_then(Value::as_bool).unwrap_or(key!="PushToTalk");if ui.checkbox(&mut value,label).changed() {settings[key]=Value::Bool(value);}
            }
            ui.separator();ui.strong("Activación de voz");
            let mut push_to_talk=field(&settings,"PushToTalk").and_then(Value::as_bool).unwrap_or(false);
            if ui.checkbox(&mut push_to_talk,"Pulsar para hablar").changed() {settings["PushToTalk"]=Value::Bool(push_to_talk);}
            ui.small(if field(&settings,"PushToTalk").and_then(Value::as_bool).unwrap_or(false) {"Pulsar para hablar: la tecla controla la transmisión; el medidor sigue mostrando la señal captada."} else {"Detección manual: el umbral configurado decide cuándo se activa la voz. El ajuste automático de umbral está desactivado."});
            let mut key=field(&settings,"PttVirtualKey").and_then(Value::as_i64).unwrap_or(163);
            draw_ptt_binding(ui, &mut key);
            settings["PttVirtualKey"]=Value::from(key);
            self.media_settings=settings;
            if response.drag_stopped() || response.changed()&&!response.dragged() {self.send("save-media-settings","",self.media_settings.to_string(),"","");}
            ui.separator();ui.heading("Cámara y pantalla");
            ui.label("La vista previa local sólo abre el dispositivo que elijas y no transmite. La cámara y pantalla salientes esperan la conexión cifrada.");
            ui.strong("Cámara");
            let cameras = self.devices["cameras"].as_array().cloned().unwrap_or_default();
            let current = s(&self.media_settings,"CameraId").to_owned();
            let mut camera = current.clone();
            egui::ComboBox::from_id_salt("settings-camera").selected_text(cameras.iter().find(|device|s(device,"id")==camera).map(|device|s(device,"name")).unwrap_or("Seleccionar cámara")).width(ui.available_width().min(500.)).show_ui(ui,|ui| {
                ui.selectable_value(&mut camera, String::new(), "Ninguna cámara seleccionada");
                for device in &cameras { ui.selectable_value(&mut camera,s(device,"id").into(),s(device,"name")); }
            });
            if camera != current {
                self.media_settings["CameraId"] = json!(camera);
                if camera.is_empty() { self.stop_selection_preview(); } else { self.select_media_preview("camera", &camera); }
            }
            self.draw_inventory_status(ui, "cameras", loading);
            ui.separator();ui.strong("Pantalla o ventana");
            let sources = self.devices["sources"].as_array().cloned().unwrap_or_default();
            let mut source = ctx.data(|data| data.get_temp::<String>(egui::Id::new("settings-screen-source"))).unwrap_or_default();
            let previous = source.clone();
            egui::ComboBox::from_id_salt("settings-screen").selected_text(sources.iter().find(|device|s(device,"id")==source).map(|device|s(device,"name")).unwrap_or("Seleccionar pantalla o ventana")).width(ui.available_width().min(500.)).show_ui(ui,|ui| {
                ui.selectable_value(&mut source, String::new(), "Ninguna pantalla seleccionada");
                for device in &sources { ui.selectable_value(&mut source,s(device,"id").into(),format!("{} {}",if s(device,"type")=="screen" {"Pantalla ·"}else{"Ventana ·"},s(device,"name"))); }
            });
            if source != previous {
                ctx.data_mut(|data| data.insert_temp(egui::Id::new("settings-screen-source"), source.clone()));
                if source.is_empty() { self.stop_selection_preview(); } else { self.select_media_preview("screen", &source); }
            }
            self.draw_inventory_status(ui, "sources", loading);
            ui.separator();ui.strong("Vista previa local");
            if let Some((kind, source)) = ctx.data(|data|data.get_temp::<(String,String)>(egui::Id::new("selected-media-preview"))) {
                let list = if kind=="camera" {&cameras} else {&sources};
                if list.iter().any(|item|s(item,"id")==source) { self.draw_selection_preview(ui,state,&kind,&source,200.); }
                else if !loading { self.stop_selection_preview(); ui.label("La fuente seleccionada dejó de estar disponible."); }
            } else { ui.label("Selecciona una cámara, pantalla o ventana para ver su vista previa."); }
            if ui.add_enabled(state.voice_connected,egui::Button::new("Seleccionar cámara para la llamada")).clicked() {self.simple("toggle-camera");}
            ui.separator();
            if ui.button("Guardar dispositivos y ajustes").clicked() {self.send("save-media-settings","",self.media_settings.to_string(),"","");}
            if !self.dialog_error.is_empty() {ui.colored_label(Color32::from_rgb(242,63,67),&self.dialog_error);}
        });
        if !open {
            self.simple("stop-mic-test");
            self.stop_selection_preview();
            self.dialog = None;
            ctx.data_mut(|data| data.insert_temp(egui::Id::new("mic-test-dialog-visible"), false));
        }
    }
    fn draw_inventory_status(&self, ui: &mut egui::Ui, part: &str, loading: bool) {
        match s(&self.devices["states"][part], "state") {
            "permission_denied" => { ui.colored_label(Color32::from_rgb(242,63,67),"Windows o el dispositivo no concedieron permiso. Revisa los permisos de privacidad y vuelve a actualizar."); }
            "error" => { ui.colored_label(Color32::from_rgb(242,63,67),format!("No se pudo enumerar: {}",s(&self.devices["states"][part],"error"))); }
            "empty" => { ui.label(if part=="cameras" {"No se encontraron cámaras."}else if part=="sources" {"No se encontraron pantallas ni ventanas disponibles."}else {"No se encontraron dispositivos de audio."}); }
            "ready" => (),
            _ if !loading => { ui.label("Inventario pendiente. Pulsa Actualizar dispositivos y fuentes."); }
            _ => (),
        }
    }
    pub(super) fn check_mic_test_dialog_closed(&mut self) {
        let visible = self
            .ctx
            .data(|data| data.get_temp::<bool>(egui::Id::new("mic-test-dialog-visible")))
            .unwrap_or(false);
        if visible && !matches!(self.dialog, Some(Dialog::Media)) {
            self.simple("stop-mic-test");
            self.ctx
                .data_mut(|data| data.insert_temp(egui::Id::new("mic-test-dialog-visible"), false));
        }
    }
    pub(super) fn select_media_preview(&mut self, kind: &str, source: &str) {
        if source.is_empty() {
            return;
        }
        let choice = (kind.to_owned(), source.to_owned());
        let old = self.ctx.data(|data| {
            data.get_temp::<(String, String)>(egui::Id::new("selected-media-preview"))
        });
        if old.as_ref() == Some(&choice) {
            return;
        }
        self.stop_selection_preview();
        self.ctx
            .data_mut(|data| data.insert_temp(egui::Id::new("selected-media-preview"), choice));
        self.send(
            if kind == "screen" {
                "start-screen-preview"
            } else {
                "start-camera-preview"
            },
            source,
            "",
            "",
            "",
        );
    }
    pub(super) fn stop_selection_preview(&mut self) {
        let had = self.ctx.data_mut(|data| {
            data.remove::<(String, u64, String, String)>(egui::Id::new("suspended-media-preview"));
            let key = egui::Id::new("selected-media-preview");
            let had = data.get_temp::<(String, String)>(key).is_some();
            data.remove::<(String, String)>(key);
            had
        });
        if had {
            self.simple("stop-media-preview");
        }
    }
    pub(super) fn set_selection_preview_suspended(&mut self, suspended: bool) {
        let key = egui::Id::new("suspended-media-preview");
        let selection = self.ctx.data(|data| data.get_temp::<(String, String)>(
            egui::Id::new("selected-media-preview")));
        let account = self.state.current_user.as_ref().map(|user|user.id.as_str()).unwrap_or("");
        if suspended {
            if let Some((kind, source)) = selection {
                let newly_suspended = self.ctx.data_mut(|data| {
                    if data.get_temp::<(String, u64, String, String)>(key).is_some() { return false; }
                    data.insert_temp(key, (account.to_owned(), self.state.notification_generation, kind, source));
                    true
                });
                if newly_suspended { self.simple("stop-media-preview"); }
            }
        } else {
            let pending = self.ctx.data_mut(|data|data.remove_temp::<(String, u64, String, String)>(key));
            if let Some((owner, generation, kind, source)) = pending
                && self.state.signed_in && owner == account
                && generation == self.state.notification_generation
                && selection.as_ref().is_some_and(|choice| choice.0 == kind && choice.1 == source)
                && matches!(self.dialog, Some(Dialog::Sources | Dialog::Cameras | Dialog::CamerasPreview | Dialog::Media))
            {
                // Resume only the already selected local preview. Publication
                // remains owned by the call and is never toggled here.
                self.send(if kind == "screen" {"start-screen-preview"} else {"start-camera-preview"},
                    source, "", "", "");
            }
        }
    }
    pub(super) fn check_selection_preview_closed(&mut self) {
        if !matches!(
            self.dialog,
            Some(Dialog::Sources | Dialog::Cameras | Dialog::CamerasPreview | Dialog::Media)
        ) {
            self.stop_selection_preview();
        }
    }
    pub(super) fn draw_selection_preview(
        &mut self,
        ui: &mut egui::Ui,
        state: &AppSnapshot,
        kind: &str,
        source: &str,
        max_height: f32,
    ) -> egui::Rect {
        let video = state
            .call_videos
            .iter()
            .filter(|video| video.preview && video.kind == kind && video.source_id == source)
            .max_by_key(|video| video.id);
        let width = ui.available_width().max(1.);
        let size = egui::vec2(width, (width * 9. / 16.).min(max_height).max(32.));
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(rect, 6, self.theme.input);
        if let Some(texture) = video.and_then(|video| self.video_texture(video.id, ui.ctx())) {
            let natural = texture.size_vec2();
            let scale = (size.x / natural.x).min(size.y / natural.y);
            painter.image(texture.id(), egui::Rect::from_center_size(rect.center(), natural * scale),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)), Color32::WHITE);
        } else {
            let selected = self
                .ctx
                .data(|data| {
                    data.get_temp::<(String, String)>(egui::Id::new("selected-media-preview"))
                })
                .is_some_and(|selection| selection.0 == kind && selection.1 == source);
            let text = video
                .filter(|video| video.status == "error")
                .map(|video| {
                    if video.error.is_empty() {
                        "No se pudo preparar esta vista previa"
                    } else {
                        video.error.as_str()
                    }
                })
                .unwrap_or(if selected {
                    "Preparando vista previa…"
                } else {
                    "Selecciona una fuente para ver su vista previa"
                });
            let galley = ui.painter().layout(
                text.into(),
                egui::FontId::proportional(13.),
                self.theme.ink,
                (width - 20.).max(1.),
            );
            painter.with_clip_rect(rect.shrink(8.)).galley(
                rect.center() - galley.size() / 2.,
                galley,
                self.theme.ink,
            );
        }
        ui.small("Vista previa de la selección. Todavía no se está transmitiendo.");
        if video.is_some_and(|video| video.status == "error")
            && ui.button("Reintentar vista previa").clicked()
        {
            self.stop_selection_preview();
            self.select_media_preview(kind, source);
        }
        rect
    }
    fn draw_media_source_choices(&mut self, ui: &mut egui::Ui, state: &AppSnapshot, kind: &str, sources: &[Value]) {
        let list_height = if ui.available_width() >= 520. { 220. } else { 136. };
        let draw_list = |app: &mut Self, ui: &mut egui::Ui| {
            theme::vertical_scroll(ui).id_salt(("media-source-list", kind))
                .min_scrolled_height(0.).max_height(list_height).auto_shrink([false, true])
                .show(ui, |ui| {
                    for source in sources {
                        let id = s(source, "id");
                        let label = if kind == "screen" {
                            format!("{} {}", if s(source, "type") == "screen" { "▣" } else { "□" }, s(source, "name"))
                        } else { s(source, "name").to_owned() };
                        let response = ui.add_sized([ui.available_width(), 32.],
                            egui::Button::new(label).selected(app.prompt == id).truncate());
                        if response.on_hover_text(s(source, "name")).clicked() { app.prompt = id.to_owned(); }
                    }
                }).inner_rect
        };
        let old = self.prompt.clone();
        let (list, preview) = if ui.available_width() >= 520. {
            let mut rectangles = (egui::Rect::NOTHING, egui::Rect::NOTHING);
            ui.columns(2, |columns| {
                columns[0].strong(if kind=="camera" {"Cámaras"}else{"Pantallas y ventanas"});
                rectangles.0 = draw_list(self, &mut columns[0]);
                if self.prompt != old { let choice = self.prompt.clone(); self.select_media_preview(kind, &choice); }
                let choice = self.prompt.clone();
                columns[1].strong("Vista previa");
                rectangles.1 = self.draw_selection_preview(&mut columns[1], state, kind, &choice, 180.);
            });
            rectangles
        } else {
            ui.strong(if kind=="camera" {"Cámaras"}else{"Pantallas y ventanas"});
            let list = draw_list(self, ui);
            if self.prompt != old { let choice = self.prompt.clone(); self.select_media_preview(kind, &choice); }
            ui.add_space(8.);
            let choice = self.prompt.clone();
            ui.strong("Vista previa");
            let preview = self.draw_selection_preview(ui, state, kind, &choice, 180.);
            (list, preview)
        };
        #[cfg(test)]
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(("media-source-layout", kind)), (list, preview)));
        #[cfg(not(test))]
        let _ = (list, preview);
    }
    pub(super) fn draw_source_selector(
        &mut self,
        ctx: &egui::Context,
        state: &AppSnapshot,
    ) -> bool {
        let mut open = true;
        let mut done = false;
        media_selector_window(ctx,"Compartir pantalla","share-source-selector").order(egui::Order::Foreground).open(&mut open).show(ctx,|ui| {
            ui.label("Elige la pantalla o ventana que quieres compartir.");
            let sources=self.dialog_data.as_array().cloned().unwrap_or_default();
            self.draw_media_source_choices(ui,state,"screen",&sources);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_label("Resolución").selected_text(format!("{}p",self.share_height)).show_ui(ui,|ui|{for height in [480,720,1080,1440] {ui.selectable_value(&mut self.share_height,height,format!("{height}p"));}});
                egui::ComboBox::from_label("Fotogramas").selected_text(format!("{} FPS",self.share_fps)).show_ui(ui,|ui|{for fps in [15,30,60] {ui.selectable_value(&mut self.share_fps,fps,format!("{fps} FPS"));}});
            });
            ui.checkbox(&mut self.share_audio,"Compartir audio");
            if self.share_audio {ui.checkbox(&mut self.share_loopback,"Incluir audio del sistema completo");}
            if !self.dialog_error.is_empty(){ui.add(egui::Label::new(&self.dialog_error).wrap());}
            let (confirm,cancel)=media_selector_footer(ui,"Compartir la fuente seleccionada",sources.iter().any(|source|s(source,"id")==self.prompt));
            if confirm {
                let pid=sources.iter().find(|source|s(source,"id")==self.prompt).and_then(|source|field(source,"pid")).and_then(Value::as_u64).unwrap_or(0);
                self.stop_selection_preview();
                self.send("start-share",self.prompt.clone(),serde_json::json!({"height":self.share_height,"fps":self.share_fps,"audio":self.share_audio,"pid":pid,"loopback":self.share_audio&&self.share_loopback}).to_string(),"","");done=true;
            }
            if cancel {done=true;}
        });
        if done || !open {
            self.stop_selection_preview();
            self.dialog = None;
        }
        open
    }
    pub(super) fn draw_camera_selector(
        &mut self,
        ctx: &egui::Context,
        state: &AppSnapshot,
        settings_only: bool,
    ) -> bool {
        let mut open = true;
        let mut done = false;
        media_selector_window(ctx,"Seleccionar cámara","camera-picker-dialog")
        .order(egui::Order::Foreground)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("Selecciona una cámara para ver su vista previa.");
            let cameras = self.dialog_data.as_array().cloned().unwrap_or_default();
            self.draw_media_source_choices(ui, state, "camera", &cameras);
            if cameras.is_empty() {
                ui.label("No se encontraron cámaras disponibles.");
            }
            if !self.dialog_error.is_empty() {
                ui.add(egui::Label::new(&self.dialog_error).wrap());
            }
            let (confirm,cancel)=media_selector_footer(ui,if settings_only {
                "Usar cámara seleccionada"
            }else{"Encender la cámara seleccionada"},cameras.iter().any(|camera|s(camera,"id")==self.prompt));
            if confirm {
                self.stop_selection_preview();
                if settings_only {
                    self.media_settings["CameraId"] = Value::String(self.prompt.clone());
                    self.send(
                        "save-media-settings",
                        "",
                        self.media_settings.to_string(),
                        "",
                        "",
                    );
                    self.dialog = Some(Dialog::Media);
                } else {
                    self.send("start-camera", self.prompt.clone(), "", "", "");
                    self.dialog = None;
                }
                done = true;
            }
            if cancel {
                self.stop_selection_preview();
                self.dialog = None;
                done = true;
            }
        });
        if !open {
            self.stop_selection_preview();
            self.dialog = None;
        }
        if done {
            return true;
        }
        open
    }
    pub(super) fn draw_incoming_call_in_chat(
        &mut self,
        ui: &mut egui::Ui,
        state: &AppSnapshot,
    ) -> bool {
        if state.view != "chat" {
            return false;
        }
        let ctx = ui.ctx().clone();
        prune_participant_media_selections(&ctx, state);
        let calls: Vec<_> = state
            .incoming_calls
            .iter()
            .filter(|call| {
                call.channel_id == state.selected_channel_id
                    && (call.ringing
                        || !call.call_participants.is_empty()
                        || !call.participants.is_empty())
                    && !(state.call_channel_id == call.channel_id
                        && (state.voice_connected || state.voice_connecting))
            })
            .take(1)
            .cloned()
            .collect();
        if calls.is_empty() {
            ctx.data_mut(|data| {
                let key = egui::Id::new("incoming-call-pending");
                if data
                    .get_temp::<(String, Instant)>(key)
                    .is_some_and(|pending| {
                        !state
                            .incoming_calls
                            .iter()
                            .any(|call| call.channel_id == pending.0)
                    })
                {
                    data.remove::<(String, Instant)>(key);
                }
            });
            return false;
        }
        let style = ctx.style();
        let spacing = style.spacing.item_spacing;
        let pending_call = ctx.data(|data| {
            data.get_temp::<(String, Instant)>(egui::Id::new("incoming-call-pending"))
        });
        let is_pending = |channel: &str| {
            pending_call.as_ref().is_some_and(|pending| {
                pending.0 == channel && pending.1.elapsed() < Duration::from_secs(3)
            })
        };
        let (body_height, small_height, action_width, button_height) = ctx.fonts_mut(|fonts| {
            let button = egui::TextStyle::Button.resolve(&style);
            let buttons = ["Aceptar", "Rechazar"].map(|label| {
                fonts
                    .layout_no_wrap(label.into(), button.clone(), self.theme.ink)
                    .size()
                    .x
                    + style.spacing.button_padding.x * 2.
                    + 2.
            });
            (
                fonts.row_height(&egui::TextStyle::Body.resolve(&style)),
                fonts.row_height(&egui::TextStyle::Small.resolve(&style)),
                buttons,
                style
                    .spacing
                    .interact_size
                    .y
                    .max(fonts.row_height(&button) + style.spacing.button_padding.y * 2. + 1.)
                    .max(24.),
            )
        });
        let actions_width = action_width[0] + action_width[1] + spacing.x;
        // Reserve the actions before laying out text. A vertical child that
        // takes the entire available width used to wrap both buttons out of view.
        let inner_width = (ui.available_width() - 38.).max(1.);
        let narrow = inner_width < 42. + spacing.x * 2. + actions_width + 140.;
        let stacked_actions = inner_width < actions_width;
        let action_rows_height = if stacked_actions {
            button_height * 2. + spacing.y
        } else {
            button_height
        };
        let header_height = |pending: bool| {
            (body_height * 2. + 1. + if pending { small_height + 1. } else { 0. })
                .ceil()
                .max(42.)
        };
        let row_height = |call: &IncomingCall| {
            let mut height = header_height(is_pending(&call.channel_id));
            if narrow {
                height += action_rows_height + spacing.y;
            }
            let count = call.call_participants.len().max(call.participants.len());
            if count > 0 {
                let columns = call_gallery_columns(inner_width,spacing.x,count,true);
                let media_controls = call
                    .call_participants
                    .iter()
                    .map(|person| {
                        usize::from(person.self_video) + usize::from(person.screen_sharing)
                    })
                    .max()
                    .unwrap_or(0);
                let tile_height = 54.
                    + body_height
                    + small_height
                    + spacing.y * 4.
                    + media_controls as f32 * (button_height + body_height + 2. + spacing.y);
                height += count.div_ceil(columns) as f32 * tile_height + small_height + spacing.y;
            }
            height + 4.
        };
        let content_height: f32 = calls.iter().map(row_height).sum();
        let available_height = ui.available_height().max(1.);
        let minimum = header_height(is_pending(&calls[0].channel_id))
            + if narrow {
                action_rows_height + spacing.y
            } else {
                0.
            }
            + 26.;
        let maximum = (available_height * 0.65)
            .max(minimum)
            .min(340.)
            .min(available_height * 0.65);
        let panel_height = (content_height + 24.)
            .min(maximum)
            .max(minimum.min(maximum));
        egui::Frame::new()
            .fill(self.theme.surface)
            .stroke(egui::Stroke::new(1_f32, self.theme.line))
            .inner_margin(10)
            .show(ui, |ui| {
                theme::vertical_scroll(ui)
                    .id_salt("incoming-calls-scroll")
                    .auto_shrink([false, true])
                    .min_scrolled_height(0.)
                    .max_height((panel_height - 22.).max(1.))
                    .show(ui, |ui| {
                        for call in calls {
                            let caller = call.caller.clone().unwrap_or(Person {
                                id: call.channel_id.clone(),
                                name: call.channel_name.clone(),
                                avatar_url: call.icon_url.clone(),
                                ..Default::default()
                            });
                            let pending = is_pending(&call.channel_id);
                            let width = ui.available_width();
                            let separate_actions =
                                width < 42. + spacing.x * 2. + actions_width + 140.;
                            let details_width = (width
                                - 42.
                                - spacing.x
                                - if separate_actions {
                                    0.
                                } else {
                                    actions_width + spacing.x
                                })
                            .max(1.);
                            let draw_actions = |this: &mut Self, ui: &mut egui::Ui| {
                                let stacked = width < actions_width;
                                let layout = if stacked {
                                    egui::Layout::top_down(egui::Align::Min)
                                } else {
                                    egui::Layout::left_to_right(egui::Align::Center)
                                };
                                ui.with_layout(layout, |ui| {
                                    ui.set_max_width(width);
                                    if stacked {
                                        ui.spacing_mut().button_padding.x = 2.;
                                    }
                                    for (label, action) in
                                        [("Aceptar", "join-call"), ("Rechazar", "reject-call")]
                                    {
                                        if action == "reject-call" && !call.ringing {
                                            continue;
                                        }
                                        let response = ui.add_enabled(
                                            !pending,
                                            egui::Button::new(label).min_size(egui::vec2(
                                                action_width[usize::from(label == "Rechazar")]
                                                    .min(width),
                                                button_height,
                                            )),
                                        );
                                        #[cfg(test)]
                                        ctx.data_mut(|data| {
                                            data.insert_temp(
                                                egui::Id::new((
                                                    "incoming-action-response",
                                                    &call.channel_id,
                                                    action,
                                                )),
                                                (response.rect, ui.clip_rect()),
                                            )
                                        });
                                        if response.clicked() {
                                            ctx.data_mut(|data| {
                                                data.insert_temp(
                                                    egui::Id::new("incoming-call-pending"),
                                                    (call.channel_id.clone(), Instant::now()),
                                                )
                                            });
                                            this.send(
                                                action,
                                                &call.channel_id,
                                                "",
                                                serde_json::json!({"instanceId":call.instance_id})
                                                    .to_string(),
                                                &call.channel_id,
                                            );
                                        }
                                    }
                                });
                            };
                            ui.allocate_ui_with_layout(
                                egui::vec2(width, header_height(pending)),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_min_height(header_height(pending));
                                    self.avatar(ui, &caller, "", 42.);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(details_width, header_height(pending)),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.set_width(details_width);
                                            ui.set_min_height(header_height(pending));
                                            ui.spacing_mut().item_spacing.y = 1.;
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new("Llamada entrante").strong(),
                                                )
                                                .truncate(),
                                            );
                                            let identity = if call.channel_name == caller.name {
                                                caller.name.clone()
                                            } else {
                                                format!("{} · {}", caller.name, call.channel_name)
                                            };
                                            ui.add(egui::Label::new(&identity).truncate())
                                                .on_hover_text(&identity);
                                            if pending {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new("Procesando solicitud…")
                                                            .small(),
                                                    )
                                                    .truncate(),
                                                );
                                            }
                                        },
                                    );
                                    if !separate_actions {
                                        draw_actions(self, ui);
                                    }
                                },
                            );
                            if separate_actions {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(
                                        width,
                                        if width < actions_width {
                                            button_height * 2. + spacing.y
                                        } else {
                                            button_height
                                        },
                                    ),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_min_height(if width < actions_width {
                                            button_height * 2. + spacing.y
                                        } else {
                                            button_height
                                        });
                                        draw_actions(self, ui);
                                    },
                                );
                            }
                            if !call.call_participants.is_empty() || !call.participants.is_empty() {
                                let participants = if call.call_participants.is_empty() {
                                    call.participants
                                        .iter()
                                        .map(|person| CallParticipant {
                                            user: person.clone(),
                                            ..Default::default()
                                        })
                                        .collect::<Vec<_>>()
                                } else {
                                    call.call_participants.clone()
                                };
                                ui.small(format!("Participantes: {}", participants.len()));
                                self.draw_media_participant_tiles(
                                    ui,
                                    &format!(
                                        "account:{}:incoming:{}:{}",
                                        state
                                            .current_user
                                            .as_ref()
                                            .map(|user| user.id.as_str())
                                            .unwrap_or(""),
                                        call.channel_id,
                                        call.instance_id
                                    ),
                                    &call.channel_id,
                                    call.instance_id,
                                    &participants,
                                    &[],
                                    &[],
                                    self.theme.ink,
                                    54.,
                                    true,
                                    true,
                                    false,
                                    false,
                                    None,
                                    state,
                                );
                            }
                            if pending {
                                ctx.request_repaint_after(Duration::from_millis(100));
                            }
                            ui.add_space(4.);
                        }
                    });
            });
        true
    }
    #[cfg(test)]
    fn draw_participant_tiles(
        &mut self,
        ui: &mut egui::Ui,
        state: &AppSnapshot,
        ink: Color32,
        max_image_height: f32,
        compact: bool,
    ) {
        self.draw_participant_tiles_in_height(ui,state,ink,max_image_height,compact,None);
    }
    fn draw_participant_tiles_in_height(
        &mut self,
        ui: &mut egui::Ui,
        state: &AppSnapshot,
        ink: Color32,
        max_image_height: f32,
        compact: bool,
        row_height: Option<f32>,
    ) {
        prune_participant_media_selections(ui.ctx(), state);
        let mut participants = state.call_participants.clone();
        for video in state.call_videos.iter().filter(|video| {
            (!video.preview
                || video.local
                    && (video.kind == "camera" && state.camera_preparing
                        || video.kind == "screen" && state.screen_preparing))
                && (video.active
                    || matches!(
                        video.status.as_str(),
                        "loading" | "connecting" | "waiting" | "starting"
                    ))
        }) {
            let id = if video.user_id.is_empty() {
                format!("video-{}", video.id)
            } else {
                video.user_id.clone()
            };
            if !participants
                .iter()
                .any(|participant| participant.user.id == id)
            {
                participants.push(CallParticipant {
                    user: Person {
                        id,
                        name: video.label.clone(),
                        ..Default::default()
                    },
                    is_self: video.local,
                    self_video: video.kind == "camera",
                    screen_sharing: video.kind == "screen",
                    ..Default::default()
                });
            }
        }
        let scope = format!(
            "account:{}:call:{}:{}",
            state
                .current_user
                .as_ref()
                .map(|user| user.id.as_str())
                .unwrap_or(""),
            state.call_channel_id,
            state.call_instance_id
        );
        self.draw_media_participant_tiles(
            ui,
            &scope,
            &state.call_channel_id,
            state.call_instance_id,
            &participants,
            &state.call_pending_invitees,
            &state.call_videos,
            ink,
            max_image_height,
            compact,
            false,
            state.camera_preparing,
            state.screen_preparing,
            row_height,
            state,
        );
    }
    fn draw_media_participant_tiles(
        &mut self,
        ui: &mut egui::Ui,
        scope: &str,
        channel_id: &str,
        instance_id: u64,
        participants: &[CallParticipant],
        pending: &[Person],
        videos: &[CallVideo],
        ink: Color32,
        max_image_height: f32,
        _compact: bool,
        incoming: bool,
        camera_preparing: bool,
        screen_preparing: bool,
        row_height: Option<f32>,
        state: &AppSnapshot,
    ) {
        let available = ui.available_width();
        let spacing = ui.spacing().item_spacing.x;
        let mut tile_ids=participants.iter().map(|participant|participant.user.id.as_str()).collect::<HashSet<_>>();
        for person in pending.iter().filter(|person|state.current_user.as_ref().is_none_or(|current|current.id!=person.id)) {
            tile_ids.insert(person.id.as_str());
        }
        let columns = call_gallery_columns(available,spacing,tile_ids.len(),incoming);
        let width = ((available - columns.saturating_sub(1) as f32 * spacing) / columns as f32).max(1.);
        egui::Grid::new(("call-participant-grid",scope,columns)).num_columns(columns)
            .min_col_width(width).max_col_width(width).min_row_height(0.)
            .spacing(ui.spacing().item_spacing).show(ui,|ui| {
                let mut tile_index=0usize;
                for participant in participants {
                    let source = |kind: &str| videos
                        .iter()
                        .filter(|video| {
                            (!video.preview
                                || video.local
                                    && (video.kind == "camera" && camera_preparing
                                        || video.kind == "screen" && screen_preparing))
                                && (video.user_id == participant.user.id
                                    || video.user_id.is_empty()
                                        && participant.user.id == format!("video-{}", video.id))
                        })
                        .filter(|video| video.kind == kind)
                        .max_by_key(|video| video.id)
                        .cloned();
                    let camera = source("camera");
                    let screen = source("screen");
                    let selector_preview = |kind: &str| match kind {
                        "camera"=>camera.as_ref(),"screen"=>screen.as_ref(),_=>None,
                    }.is_some_and(|video|video.local && video.preview);
                    let running = |video: &CallVideo| video.active || matches!(
                        video.status.as_str(), "loading" | "connecting" | "waiting" | "starting"
                    );
                    let retained = |video: &CallVideo| running(video) || video.status=="error";
                    let camera_available = participant.self_video || camera.as_ref().is_some_and(|video|
                        retained(video) && (video.local || video.view_requested));
                    let screen_available = participant.screen_sharing || participant.is_self
                        && screen.as_ref().is_some_and(retained);
                    let requested = |video: &CallVideo| video.view_requested && retained(video);
                    let automatic = if screen_available && screen.as_ref().is_some_and(requested) {
                        "screen"
                    } else if camera_available && camera.as_ref().is_some_and(requested) {
                        "camera"
                    } else { "avatar" };
                    let key = format!("{scope}:{}", participant.user.id);
                    let mut remembered = ui.ctx().data_mut(|data| data.get_temp_mut_or_default::<HashMap<String,String>>(
                        egui::Id::new("participant-media-selection"),
                    ).get(&key).cloned());
                    let mut selected = match remembered.as_deref().unwrap_or(automatic) {
                        "camera" if camera_available => "camera",
                        "screen" if screen_available => "screen",
                        _ => "avatar",
                    };
                    let selected_source = match selected { "camera"=>camera.as_ref(), "screen"=>screen.as_ref(), _=>None };
                    let previous = ui.ctx().data_mut(|data| data.get_temp_mut_or_default::<HashMap<String,ParticipantSource>>(
                        egui::Id::new("participant-source-lifecycle")).get(&key).cloned());
                    let pending_retry=previous.as_ref().filter(|previous|previous.kind==selected&&selected=="screen")
                        .and_then(|previous|previous.pending_retry.clone()).filter(|(_,started)|started.elapsed()<Duration::from_secs(10));
                    let retry_waiting=pending_retry.as_ref().is_some_and(|(old,_)|
                        selected_source.is_none_or(|video|video.source_id==*old));
                    let retry_acknowledged=pending_retry.as_ref().is_some_and(|(old,_)|
                        selected_source.is_some_and(|video|!video.local&&video.view_requested&&retained(video)
                            && !video.source_id.is_empty()&&video.source_id!=*old));
                    if retry_waiting {
                        if let Some((_,started))=&pending_retry {
                            ui.ctx().request_repaint_after(Duration::from_secs(10).saturating_sub(started.elapsed()));
                        }
                    }
                    // A Go Live watch can replace its native decoder/SSRC.
                    // Keep the explicit choice only within the same watch token.
                    let changed = previous.as_ref().is_some_and(|previous| previous.kind == selected && selected != "avatar"
                        && (previous.id.zip(selected_source.map(|video|video.id)).is_some_and(|(old,new)|old!=new)
                                && !selected_source.is_some_and(|video| selected=="screen" && !video.local
                                    && video.view_requested && !video.source_id.is_empty() && previous.source_id==video.source_id)
                            || selected_source.is_some_and(|video| selected=="screen" && !video.local
                                && !previous.source_id.is_empty() && previous.source_id!=video.source_id)
                            || selected_source.is_some_and(|video|previous.selector_preview!=(video.local && video.preview))
                            || previous.running && !selected_source.is_some_and(retained)));
                    let unavailable = matches!(remembered.as_deref(),Some("camera")) && !camera_available
                        || matches!(remembered.as_deref(),Some("screen")) && !screen_available;
                    let ended = previous.as_ref().is_some_and(|previous| previous.kind != "avatar"
                        && remembered.as_deref().is_none_or(|kind|kind==previous.kind)
                        && previous.running && !match previous.kind.as_str() {
                            "camera"=>camera.as_ref(),"screen"=>screen.as_ref(),_=>None,
                        }.is_some_and(retained));
                    if unavailable || (changed || ended) && !retry_waiting && !retry_acknowledged {
                        let old = remembered.as_deref().filter(|kind|matches!(*kind,"camera"|"screen"))
                            .or_else(||previous.as_ref().map(|source|source.kind.as_str()).filter(|kind|matches!(*kind,"camera"|"screen")));
                        if let Some(old) = old {
                            // The selector owns its local preview. Resetting a
                            // tile must not turn that capture off or route it
                            // through the published-source controller.
                            if !selector_preview(old) && !previous.as_ref().is_some_and(|source|source.kind==old && source.selector_preview) {
                                let extra=if incoming {json!({"instanceId":instance_id,"visible":false})}
                                    else {json!({"voiceAttempt":instance_id,"visible":false})};
                                self.send("view-incoming-media",&participant.user.id,old,extra.to_string(),channel_id);
                            }
                        }
                        selected = "avatar"; remembered = Some("avatar".into());
                        ui.ctx().data_mut(|data| {data.get_temp_mut_or_default::<HashMap<String,String>>(
                            egui::Id::new("participant-media-selection")).insert(key.clone(),"avatar".into());});
                    }
                    let selected_source = match selected { "camera"=>camera.as_ref(), "screen"=>screen.as_ref(), _=>None };
                    let bound_id = selected_source.filter(|video|video.active).map(|video|video.id)
                        .or_else(||previous.as_ref().filter(|previous|previous.kind==selected).and_then(|previous|previous.id));
                    ui.ctx().data_mut(|data| {
                        let sources=data.get_temp_mut_or_default::<HashMap<String,ParticipantSource>>(egui::Id::new("participant-source-lifecycle"));
                        if sources.len()>=256 && !sources.contains_key(&key) {sources.clear();}
                        // A connecting watch has a provisional id. Bind source
                        // identity only after a real frame, so its first native
                        // id does not look like an unsolicited source restart.
                        sources.insert(key.clone(),ParticipantSource {kind:selected.into(),id:bound_id,
                            source_id:selected_source.map(|video|video.source_id.clone()).unwrap_or_default(),
                            pending_retry:if retry_waiting {pending_retry.clone()}else{None},
                            running:selected_source.is_some_and(retained),selector_preview:selector_preview(selected)});
                    });
                    let video = match selected { "camera"=>camera.as_ref(), "screen"=>screen.as_ref(), _=>None }
                        .filter(|video| running(video)||video.status=="error");
                    let gap=ui.spacing().item_spacing.y;
                    let display_name=if !incoming&&!state.call_guild_id.is_empty()&&state.selected_guild_id==state.call_guild_id {
                        state.guild_display_name(&participant.user)
                    }else{participant.user.name.as_str()};
                    let mut flags=Vec::new();
                    if participant.self_mute||participant.server_mute {flags.push("Silenciado");}
                    if participant.self_deaf||participant.server_deaf {flags.push("Ensordecido");}
                    if camera_available {flags.push("cámara");}
                    if participant.screen_sharing {flags.push("pantalla");}
                    let flags=flags.join(" · ");
                    let mut footer=call_tile_text_height(ui,width,display_name,&egui::TextStyle::Body)+gap;
                    if !flags.is_empty() {footer+=call_tile_text_height(ui,width,&flags,&egui::TextStyle::Small)+gap;}
                    if screen_available {footer+=call_tile_text_height(ui,width,"Espectadores: 999",&egui::TextStyle::Small)+gap;}
                    for (label,available) in [("Cámara",camera_available),("Pantalla",screen_available)] {
                        if available {footer+=call_tile_button_height(ui,width,&format!("{label} · Dejar de ver"))+gap;}
                    }
                    if video.is_some_and(|video|video.status=="error") {footer+=ui.text_style_height(&egui::TextStyle::Small)+gap;}
                    let image_height=call_tile_image_height(width,max_image_height,row_height,footer);
                    // Grid centers requested cell sizes vertically. A zero-
                    // height allocation would start halfway down its cached
                    // row before growing; anchor the child at the row top.
                    let tile_rect=egui::Rect::from_min_size(ui.cursor().min,egui::vec2(width,0.));
                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(tile_rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                        |ui| {
                            ui.set_width(width);
                            let clip=ui.clip_rect();
                            ui.set_clip_rect(clip.intersect(egui::Rect::from_min_max(
                                egui::pos2(ui.max_rect().left(),clip.top()),egui::pos2(ui.max_rect().left()+width,clip.bottom()))));
                            let height = image_height;
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(width, height),
                                if selected=="screen" {egui::Sense::click_and_drag()}else{egui::Sense::click()},
                            );
                            ui.painter()
                                .rect_filled(rect, 6, call_background(self.theme.base));
                            let texture = video.filter(|video|video.active)
                                .and_then(|video| self.video_texture(video.id, ui.ctx()));
                            if let Some(texture) = texture {
                                let natural = texture.size_vec2();
                                let ratio =
                                    (rect.width() / natural.x).min(rect.height() / natural.y);
                                egui::Image::new((texture.id(), natural * ratio))
                                    .tint(Color32::WHITE)
                                    .paint_at(
                                        ui,
                                        egui::Rect::from_center_size(
                                            rect.center(),
                                            natural * ratio,
                                        ),
                                    );
                                if response.clicked_by(egui::PointerButton::Primary) && !egui::Popup::is_any_open(ui.ctx()) {
                                    let id = video.unwrap().id;
                                    if !incoming {
                                        ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("call-expanded-video"),CallExpansion {
                                            scope:scope.into(),id,zoom:1.,pan:egui::Vec2::ZERO,
                                        }));
                                    }
                                    self.send("video-quality", &id.to_string(), "true", "", "");
                                }
                            } else if video.is_some_and(|video|video.status!="error") {
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Cargando vídeo…",
                                    egui::FontId::proportional(12.),
                                    ink,
                                );
                                ui.ctx().request_repaint_after(Duration::from_millis(100));
                            } else {
                                let avatar = egui::Rect::from_center_size(
                                    rect.center(),
                                    egui::Vec2::splat(height.min(54.)),
                                );
                                self.pictures.paint_avatar(ui, avatar, &participant.user);
                                if participant.speaking
                                    && !participant.self_mute
                                    && !participant.self_deaf
                                    && !participant.server_mute
                                    && !participant.server_deaf
                                {
                                    ui.painter().circle_stroke(
                                        avatar.center(),
                                        avatar.width() / 2. + 2.,
                                        egui::Stroke::new(3_f32, presence_color("online")),
                                    );
                                }
                            }
                            if selected=="avatar" || selected=="screen" {
                                #[cfg(test)] ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new(("participant-avatar",scope,&participant.user.id)),rect));
                                response.context_menu(|ui|self.draw_call_participant_menu(ui,participant,state,channel_id,instance_id,incoming));
                            }
                            ui.add(
                                egui::Label::new(RichText::new(display_name).color(ink)).wrap(),
                            ).on_hover_text(display_name);
                            if !flags.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&flags).small().color(ink),
                                    )
                                    .wrap(),
                                );
                            }
                            if screen_available && !incoming {
                                let viewers=ui.ctx().data(|data|data.get_temp::<Value>(egui::Id::new("stream-viewers"))).unwrap_or(Value::Null);
                                if viewers["channelId"]==channel_id && viewers["voiceAttempt"].as_u64()==Some(instance_id) {
                                    let key=screen.as_ref().map(|video|video.stream_key.as_str()).filter(|key|!key.is_empty());
                                    if let Some(count)=key.and_then(|key|viewers["counts"][key].as_u64()) {
                                        ui.add(egui::Label::new(RichText::new(format!("Espectadores: {count}")).small().color(ink)).wrap());
                                    }
                                }
                            }
                            for (kind,label,available) in [("camera","Cámara",camera_available),("screen","Pantalla",screen_available)] {
                                if !available {continue;}
                                let failed=selected==kind && video.is_some_and(|video|video.status=="error");
                                let retrying=retry_waiting&&selected==kind;
                                let showing=retrying||!failed && selected==kind && (incoming || video.is_some() || remembered.as_deref()==Some(kind));
                                let response=ui.add_sized([width,call_button_height(ui)],egui::Button::new(
                                    format!("{label} · {}",if retrying {"Cancelar"} else if failed {"Reintentar"} else if showing {"Dejar de ver"} else {"Mostrar"})
                                ).wrap());
                                #[cfg(test)] ui.ctx().data_mut(|data|data.insert_temp(
                                    egui::Id::new(("participant-media-button",scope,&participant.user.id,kind)),
                                    (response.rect,ui.clip_rect()),
                                ));
                                if response.clicked() {
                                    let next=if showing {"avatar"} else {kind};
                                    ui.ctx().data_mut(|data|{
                                        let id=egui::Id::new("participant-media-selection");
                                        let choices=data.get_temp_mut_or_default::<HashMap<String,String>>(id);
                                        if choices.len()>=256 && !choices.contains_key(&key) {choices.clear();}
                                        choices.insert(key.clone(),next.into());
                                        let sources=data.get_temp_mut_or_default::<HashMap<String,ParticipantSource>>(
                                            egui::Id::new("participant-source-lifecycle"));
                                        sources.remove(&key);
                                        if failed&&!retrying&&kind=="screen" {
                                            if let Some(video)=screen.as_ref().filter(|video|!video.local&&!video.source_id.is_empty()) {
                                                // Rendering may run again before the actor replaces
                                                // the failed watch. Keep this explicit retry across
                                                // that old snapshot and its new watch token.
                                                sources.insert(key.clone(),ParticipantSource {kind:kind.into(),id:None,
                                                    source_id:video.source_id.clone(),pending_retry:Some((video.source_id.clone(),Instant::now())),
                                                    running:false,selector_preview:false});
                                            }
                                        }
                                    });
                                    for (source,visible) in [(selected,false),(next,true)] {
                                        if source=="avatar" || selector_preview(source) {continue;}
                                        let extra=if incoming {serde_json::json!({"instanceId":instance_id,"visible":visible})}
                                            else {serde_json::json!({"voiceAttempt":instance_id,"visible":visible})};
                                        self.send("view-incoming-media",&participant.user.id,source,extra.to_string(),channel_id);
                                    }
                                }
                            }
                            if let Some(video)=video.filter(|video|video.status=="error") {
                                let error=if video.error.is_empty() {
                                    "No se pudo abrir este vídeo"
                                } else {&video.error};
                                ui.add(egui::Label::new(RichText::new(error).small().color(ink)).truncate()).on_hover_text(error);
                            }
                        },
                    );
                    tile_index+=1;
                    if tile_index%columns==0 {ui.end_row();}
                }
                let mut shown = HashSet::new();
                for person in pending.iter().filter(|person|
                    !participants.iter().any(|participant| participant.user.id == person.id)
                        && state.current_user.as_ref().is_none_or(|current| current.id != person.id)) {
                    if !shown.insert(person.id.as_str()) { continue; }
                    let tile_rect=egui::Rect::from_min_size(ui.cursor().min,egui::vec2(width,0.));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(tile_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                        ui.set_width(width);
                        let clip=ui.clip_rect();
                        ui.set_clip_rect(clip.intersect(egui::Rect::from_min_max(
                            egui::pos2(ui.max_rect().left(),clip.top()),egui::pos2(ui.max_rect().left()+width,clip.bottom()))));
                        let footer=call_tile_text_height(ui,width,&person.name,&egui::TextStyle::Body)
                            +call_tile_text_height(ui,width,"Llamando...",&egui::TextStyle::Small)+ui.spacing().item_spacing.y*2.;
                        let height = call_tile_image_height(width,max_image_height,row_height,footer);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 6, call_background(self.theme.base));
                        let avatar = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(height.min(54.)));
                        self.pictures.paint_avatar(ui, avatar, person);
                        ui.painter().rect_filled(rect, 6, Color32::from_rgba_unmultiplied(100, 100, 100, 160));
                        ui.add(egui::Label::new(RichText::new(&person.name).color(ink.gamma_multiply(0.8))).wrap()).on_hover_text(&person.name);
                        ui.add(egui::Label::new(RichText::new("Llamando...").small().color(ink)).wrap());
                        #[cfg(test)] ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new(("pending-call-tile",scope,&person.id)),rect));
                    });
                    tile_index+=1;
                    if tile_index%columns==0 {ui.end_row();}
                }
            });
    }
    pub(super) fn draw_add_to_call(&mut self, ui: &mut egui::Ui, person: &Person, state: &AppSnapshot) {
        if !state.call_guild_id.is_empty() || state.call_channel_id.is_empty()
            || !state.call_invite_candidates.iter().any(|candidate| candidate.id == person.id) { return; }
        let enabled = state.voice_connected && state.signed_in && !person.is_bot
            && state.current_user.as_ref().is_none_or(|current| current.id != person.id)
            && !state.call_participants.iter().any(|participant| participant.user.id == person.id)
            && !state.call_pending_invitees.iter().any(|pending| pending.id == person.id)
            && !state.call_invite_busy_ids.contains(&person.id);
        if ui.add_enabled(enabled, egui::Button::new("Añadir a la llamada"))
            .on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.send("call-invite", &person.id, "", json!({"voiceAttempt":state.call_instance_id}).to_string(), &state.call_channel_id);
            ui.close();
        }
    }
    fn draw_call_invite_menu(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        if !state.call_guild_id.is_empty() || state.call_invite_candidates.is_empty() { return; }
        ui.menu_button("Miembros de la conversación", |ui| {
            for person in &state.call_invite_candidates {
                ui.menu_button(&person.name, |ui| self.draw_add_to_call(ui, person, state));
            }
        });
    }
    fn draw_call_participant_menu(&mut self,ui:&mut egui::Ui,participant:&CallParticipant,state:&AppSnapshot,channel:&str,attempt:u64,incoming:bool) {
        let valid=notdiscord_core::parsing::valid_id(&participant.user.id);
        let other=valid && !participant.is_self && state.current_user.as_ref().is_none_or(|user|user.id!=participant.user.id);
        for label in ["Enviar mensaje","Abrir conversación"] {
            if ui.add_enabled(other,egui::Button::new(label)).clicked() {self.send("open-dm",&participant.user.id,"","","");ui.close();}
        }
        if ui.add_enabled(valid,egui::Button::new("Ver perfil")).clicked() {self.send("profile",&participant.user.id,"","","");ui.close();}
        let dm=state.direct_messages.iter().find(|item|item.kind=="dm"&&item.recipient_id==participant.user.id);
        let callable=other&&!participant.user.is_bot&&state.voice_available&&dm.is_some_and(|item|item.id!=state.call_channel_id);
        if ui.add_enabled(callable,egui::Button::new("Llamar")).on_hover_text("Conectar a la conversación de esta persona; se cerrará la llamada anterior.").clicked() {
            self.send("call-dm",&dm.unwrap().id,"","","");ui.close();
        }
        ui.separator();
        self.draw_stream_volume_control(ui,state);
        if !other {return;}
        ui.separator();
        let enabled=!incoming&&state.voice_connected&&state.call_channel_id==channel&&state.call_instance_id==attempt;
        let key=egui::Id::new(("participant-volume",channel,attempt,&participant.user.id));
        let maximum=500.;
        let confirmed=if participant.volume_percent.is_finite(){participant.volume_percent.clamp(0.,maximum)}else{100.};
        let pending=ui.ctx().data(|data|data.get_temp::<(f32,Instant)>(key)).filter(|(value,time)|*value!=confirmed&&time.elapsed()<Duration::from_secs(3));
        let mut value=pending.map(|(value,_)|value).unwrap_or(confirmed);
        ui.label(format!("Volumen de voz del participante · {confirmed:.0} %"));
        let response=ui.add_enabled(enabled,egui::Slider::new(&mut value,0.0..=maximum).integer().suffix(" %"));
        #[cfg(test)] ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new(("participant-volume-slider",&participant.user.id)),response.rect));
        if response.changed() {ui.ctx().data_mut(|data|data.insert_temp(key,(value,Instant::now())));}
        if enabled&&(response.drag_stopped()||response.changed()&&!response.dragged()) {
            self.send("participant-volume",&participant.user.id,value.to_string(),json!({"voiceAttempt":attempt}).to_string(),channel);
        }
        if pending.is_some() {ui.small("Aplicando volumen…");ui.ctx().request_repaint_after(Duration::from_millis(100));}
    }
    fn draw_stream_volume_control(&mut self,ui:&mut egui::Ui,state:&AppSnapshot) {
        let mut percent=if state.stream_volume_percent.is_finite(){state.stream_volume_percent.clamp(0.,100.)}else{100.};
        ui.label("Volumen de las transmisiones");
        let response=ui.add(egui::Slider::new(&mut percent,0.0..=100.).integer().suffix(" %"));
        if response.changed() {self.send("stream-volume","",percent.to_string(),"","");}
    }
}

#[derive(Clone)]
pub(super) struct CallExpansion {
    scope: String,
    id: u64,
    zoom: f32,
    pan: egui::Vec2,
}
fn call_gallery_columns(_width:f32,_gap:f32,count:usize,_incoming:bool)->usize {
    // The cell width adapts to zoom and available space; the requested count
    // decides the columns. Grid/end_row enforce this instead of automatic wrap.
    count.clamp(1,4)
}
fn call_gallery_tile_count(state:&AppSnapshot)->usize {
    let mut ids=state.call_participants.iter().map(|participant|participant.user.id.clone()).collect::<HashSet<_>>();
    for video in state.call_videos.iter().filter(|video|
        (!video.preview || video.local && (video.kind=="camera" && state.camera_preparing || video.kind=="screen" && state.screen_preparing))
        && (video.active || matches!(video.status.as_str(),"loading"|"connecting"|"waiting"|"starting"))) {
        ids.insert(if video.user_id.is_empty() {format!("video-{}",video.id)}else{video.user_id.clone()});
    }
    for person in state.call_pending_invitees.iter().filter(|person|state.current_user.as_ref().is_none_or(|current|current.id!=person.id)) {
        ids.insert(person.id.clone());
    }
    ids.len()
}
fn call_tile_button_height(ui:&egui::Ui,width:f32,label:&str)->f32 {
    let padding=ui.spacing().button_padding;
    let font=egui::TextStyle::Button.resolve(ui.style());
    let text=ui.fonts_mut(|fonts|fonts.layout(label.into(),font,Color32::WHITE,(width-padding.x*2.).max(1.)).size().y);
    call_button_height(ui).max(text+padding.y*2.+1.)
}
fn call_tile_text_height(ui:&egui::Ui,width:f32,text:&str,style:&egui::TextStyle)->f32 {
    let font=style.resolve(ui.style());
    ui.fonts_mut(|fonts|fonts.layout(text.into(),font,Color32::WHITE,width.max(1.)).size().y)
}
fn call_controls_height(ui:&egui::Ui,width:f32)->f32 {
    let columns=((width+ui.spacing().item_spacing.x)/96.).floor().clamp(1.,5.) as usize;
    let rows=5_usize.div_ceil(columns);
    rows as f32*call_button_height(ui)+rows.saturating_sub(1) as f32*ui.spacing().item_spacing.y
}
fn call_tile_image_height(width:f32,maximum:f32,row_height:Option<f32>,footer:f32)->f32 {
    let natural=(width*9./16.).min(maximum);
    row_height.map_or(natural,|height|natural.min((height-footer-2.).max(12.))).max(12.)
}
#[derive(Clone)]
struct ParticipantSource { kind:String, id:Option<u64>, source_id:String, pending_retry:Option<(String,Instant)>, running:bool, selector_preview:bool }
impl CallExpansion {
    fn matches(&self, state: &AppSnapshot, video: &CallVideo) -> bool {
        self.scope==format!("account:{}:call:{}:{}",state.current_user.as_ref().map(|user|user.id.as_str()).unwrap_or(""),state.call_channel_id,state.call_instance_id)
            && self.id==video.id && video.active && !video.preview
            && (video.user_id.is_empty() || state.call_participants.iter().any(|person|person.user.id==video.user_id))
    }
}
pub(super) fn prune_expanded_video(ctx: &egui::Context, state: &AppSnapshot) {
    let key=egui::Id::new("call-expanded-video");
    ctx.data_mut(|data| {
        if data.get_temp::<CallExpansion>(key).is_some_and(|expanded| !state.call_videos.iter().any(|video|expanded.matches(state,video))) {
            data.remove::<CallExpansion>(key);
        }
    });
}

fn prune_participant_media_selections(ctx: &egui::Context, state: &AppSnapshot) {
    let account = state
        .current_user
        .as_ref()
        .map(|user| user.id.as_str())
        .unwrap_or("");
    let mut scopes = vec![format!(
        "account:{account}:call:{}:{}:",
        state.call_channel_id, state.call_instance_id
    )];
    scopes.extend(state.incoming_calls.iter().map(|call| {
        format!(
            "account:{account}:incoming:{}:{}:",
            call.channel_id, call.instance_id
        )
    }));
    ctx.data_mut(|data| {
        let choices = data.get_temp_mut_or_default::<HashMap<String, String>>(egui::Id::new(
            "participant-media-selection",
        ));
        choices.retain(|key, _| scopes.iter().any(|scope| key.starts_with(scope)));
        let sources = data.get_temp_mut_or_default::<HashMap<String,ParticipantSource>>(egui::Id::new("participant-source-lifecycle"));
        sources.retain(|key,_| scopes.iter().any(|scope|key.starts_with(scope)));
    });
}

#[derive(Clone, Default)]
struct PttCapture { released: bool }
impl PttCapture {
    fn next(&mut self, keys: &[i32]) -> Option<i32> {
        if !self.released { self.released = keys.is_empty(); return None; }
        keys.iter().copied().find(|key| !(matches!(key,0x10|0x11|0x12) && keys.iter().any(|other| match key {
            0x10 => matches!(other,0xa0|0xa1), 0x11 => matches!(other,0xa2|0xa3), _ => matches!(other,0xa4|0xa5),
        })))
    }
}
fn ptt_key_name(key: i32) -> String {
    let mouse = match key {1=>Some("Botón izquierdo del ratón"),2=>Some("Botón derecho del ratón"),4=>Some("Botón central del ratón"),5=>Some("Botón lateral 1"),6=>Some("Botón lateral 2"),_=>None};
    if let Some(name) = mouse { return name.into(); }
    if !(1..=254).contains(&key) { return "Tecla no válida".into(); }
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyNameTextW,MapVirtualKeyW,MAPVK_VK_TO_VSC};
    let mut text = [0u16;128];
    let length = unsafe {
        let scan = MapVirtualKeyW(key as u32,MAPVK_VK_TO_VSC);
        let extended = matches!(key,0x21..=0x28|0x2c..=0x2e|0x5b..=0x5d|0x6f|0x90|0xa3|0xa5);
        GetKeyNameTextW(((scan << 16) | if extended {1 << 24} else {0}) as i32,&mut text)
    };
    if length > 0 { String::from_utf16_lossy(&text[..length as usize]) } else { format!("Tecla VK 0x{key:02X}") }
}
fn draw_ptt_binding(ui: &mut egui::Ui, key: &mut i64) {
    let id = egui::Id::new("ptt-key-capture");
    let mut capture = ui.ctx().data(|data| data.get_temp::<PttCapture>(id));
    let mut start = false;
    let mut cancel = false;
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Pulsar para hablar: {}",ptt_key_name(*key as i32)));
        start = ui.button(if capture.is_some() {"Esperando tecla…"}else{"Cambiar tecla"}).clicked() && capture.is_none();
        if capture.is_some() { cancel = ui.button("Cancelar selección de tecla").clicked(); }
    });
    if cancel { capture = None; }
    else if start { capture = Some(PttCapture::default()); }
    else if let Some(selection) = capture.as_mut() {
        if crate::platform::foreground_is_current_process() {
            let keys: Vec<_> = (1..=254).filter(|key|crate::platform::key_down(*key)).collect();
            if let Some(chosen) = selection.next(&keys) { *key = chosen as i64; capture = None; }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(20));
    }
    if capture.is_some() { ui.label("Suelta las teclas y pulsa la que quieras registrar (también botones del ratón)."); }
    ui.small("Una tecla física detectada por Windows; los atajos reservados, el escritorio seguro y algunas teclas de firmware pueden no llegar a la aplicación. El micrófono debe estar activo y el canal debe permitir hablar.");
    ui.ctx().data_mut(|data| { if let Some(capture)=capture { data.insert_temp(id,capture); } else { data.remove::<PttCapture>(id); } });
}
fn call_button_height(ui: &egui::Ui) -> f32 {
    ui.spacing()
        .interact_size
        .y
        .max(
            ui.text_style_height(&egui::TextStyle::Button)
                + 2. * ui.spacing().button_padding.y
                + 1.,
        )
        .max(24.)
}
fn call_background(base: Color32) -> Color32 {
    let [r, g, b, a] = base.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r / 2, g / 2, b / 2, a)
}
fn call_luminance(color: Color32) -> f64 {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let linear = |value: u8| {
        let value = value as f64 / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}
fn call_contrast(a: Color32, b: Color32) -> f64 {
    let a = call_luminance(a);
    let b = call_luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
fn call_ink(background: Color32) -> Color32 {
    if call_contrast(background, Color32::WHITE) >= call_contrast(background, Color32::BLACK) {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}
fn voice_ring_outline(background: Color32, green: Color32) -> Option<Color32> {
    (call_contrast(background, green) < 3.).then(|| call_ink(background))
}
#[cfg(test)]
mod tests {
    use super::*;
    include!("voice/invitation_tests.rs");
    #[test]
    fn call_panel_follows_private_conversation_or_call_guild_without_mutating_the_call() {
        let mut state=AppSnapshot{voice_connected:true,call_channel_id:"200".into(),call_instance_id:7,selected_channel_id:"200".into(),..Default::default()};
        assert!(call_panel_visible(&state));state.selected_channel_id="201".into();assert!(!call_panel_visible(&state));
        state.selected_guild_id="500".into();state.selected_channel_id="200".into();assert!(!call_panel_visible(&state));
        state.call_guild_id="500".into();assert!(call_panel_visible(&state));state.selected_channel_id="501".into();assert!(call_panel_visible(&state));
        state.selected_guild_id="600".into();assert!(!call_panel_visible(&state));state.selected_guild_id.clear();assert!(!call_panel_visible(&state));
        assert!(state.voice_connected);assert_eq!(state.call_channel_id,"200");assert_eq!(state.call_instance_id,7);
        state.voice_connected=false;state.voice_connecting=true;state.selected_guild_id="500".into();assert!(call_panel_visible(&state));
        state.call_channel_id.clear();assert!(!call_panel_visible(&state));
        assert_eq!(CallParticipant::default().volume_percent,100.);
        assert_eq!(serde_json::from_value::<CallParticipant>(json!({"User":{"Id":"300"}})).unwrap().volume_percent,100.);
    }
    #[test]
    fn participant_context_menu_opens_only_on_avatar_and_routes_profile_to_that_peer() {
        for camera in [false,true] {
            let (mut app,_,mut commands)=super::super::tests::harness();let ctx=app.ctx.clone();ctx.style_mut(|style|style.animation_time=0.);
            let texture=ctx.load_texture("offline-context-camera",egui::ColorImage::filled([2,1],Color32::GREEN),egui::TextureOptions::NEAREST);app.video_textures.insert(10,texture);
            let state=AppSnapshot{current_user:Some(Person{id:"100".into(),..Default::default()}),voice_connected:true,call_channel_id:"200".into(),call_instance_id:7,
                call_participants:vec![CallParticipant{user:Person{id:"300".into(),name:"Selected peer".into(),..Default::default()},self_video:camera,..Default::default()}],
                call_videos:if camera {vec![CallVideo{id:10,user_id:"300".into(),kind:"camera".into(),active:true,view_requested:true,..Default::default()}]}else{vec![]},..Default::default()};
            let scope="account:100:call:200:7";
            let frame=|app:&mut NotDiscordUi,events|ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(800.,700.))),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.draw_participant_tiles(ui,&state,Color32::WHITE,180.,false));});
            let mut output=frame(&mut app,vec![]);for _ in 0..3 {output=frame(&mut app,vec![]);}
            let avatar=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new(("participant-avatar",scope,"300"))));
            if camera {assert!(avatar.is_none());}
            let point=avatar.map(|rect|rect.center()).unwrap_or(egui::pos2(90.,70.));
            for pressed in [true,false] {frame(&mut app,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Secondary,pressed,modifiers:egui::Modifiers::NONE}]);}
            for _ in 0..3 {output=frame(&mut app,vec![]);}
            let text_point=|label:&str|output.shapes.iter().find_map(|shape|if let egui::Shape::Text(text)=&shape.shape {(text.galley.text()==label).then_some(text.pos+text.galley.size()/2.)}else{None});
            if camera {assert!(text_point("Ver perfil").is_none());assert!(commands.try_recv().is_err());continue;}
            for label in ["Enviar mensaje","Abrir conversación","Ver perfil","Llamar"] {assert!(text_point(label).is_some(),"Missing {label}");}
            assert!(ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new(("participant-volume-slider","300")))).is_some());
            let point=text_point("Ver perfil").unwrap();
            for pressed in [true,false] {frame(&mut app,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
            let action=commands.try_recv().unwrap();assert_eq!((action.kind.as_str(),action.id.as_str()),("profile","300"));assert!(commands.try_recv().is_err());
        }
    }
    #[test]
    fn own_and_peer_volume_menus_apply_zero_to_the_correct_audio_target_without_loopback() {
        for local in [false,true] {
            let (mut app,_,mut commands)=super::super::tests::harness();let ctx=app.ctx.clone();
            let id=if local {"100"} else {"300"};
            let participant=CallParticipant {user:Person{id:id.into(),name:"Offline audio target".into(),..Default::default()},is_self:local,volume_percent:125.,..Default::default()};
            let state=AppSnapshot{current_user:Some(Person{id:"100".into(),..Default::default()}),voice_connected:true,call_channel_id:"200".into(),call_instance_id:7,call_participants:vec![participant.clone()],..Default::default()};
            let frame=|app:&mut NotDiscordUi,events|ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(700.,500.))),events,..Default::default()},|ctx| {
                egui::CentralPanel::default().show(ctx,|ui|app.draw_call_participant_menu(ui,&participant,&state,"200",7,false));
            });
            frame(&mut app,vec![]);frame(&mut app,vec![]);
            let slider=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new(("participant-volume-slider",id)))).unwrap();
            let point=egui::pos2(slider.left()+0.1,slider.center().y);
            for pressed in [true,false] {frame(&mut app,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton{pos:point,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
            let actions=std::iter::from_fn(||commands.try_recv().ok()).collect::<Vec<_>>();
            assert!(!actions.is_empty());
            for action in actions {
                assert_eq!(action.kind,if local {"participant-input-volume"} else {"participant-volume"});
                assert_eq!(action.id,id);assert_eq!(action.channel_id,"200");assert_eq!(action.text,"0");
                assert_eq!(serde_json::from_str::<Value>(&action.extra).unwrap()["voiceAttempt"],7);
            }
        }
    }
    #[test]
    fn call_video_expands_zooms_pans_and_resets_when_the_source_ends_without_extra_textures() {
        let (mut app,_,mut commands)=super::super::tests::harness();let ctx=app.ctx.clone();
        let texture=ctx.load_texture("offline-call-expand",egui::ColorImage::filled([320,180],Color32::GREEN),egui::TextureOptions::NEAREST);
        let texture_id=texture.id();app.video_textures.insert(99,texture);
        let mut state=AppSnapshot {call_channel_id:"200".into(),call_instance_id:7,voice_connected:true,
            call_videos:vec![CallVideo{id:99,user_id:"300".into(),kind:"screen".into(),active:true,view_requested:true,status:"playing".into(),..Default::default()}],
            call_participants:vec![CallParticipant{user:Person{id:"300".into(),name:"Synthetic peer".into(),..Default::default()},screen_sharing:true,..Default::default()}],..Default::default()};
        let frame=|app:&mut NotDiscordUi,state:&AppSnapshot,events|ctx.run(egui::RawInput {
            screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(900.,650.))),events,..Default::default()
        },|ctx| {egui::CentralPanel::default().show(ctx,|ui|app.draw_call_in_height(ui,state,400.));});
        let mut output=frame(&mut app,&state,vec![]);for _ in 0..3 {output=frame(&mut app,&state,vec![]);}
        let point=output.shapes.iter().find_map(|shape|match &shape.shape {
            egui::Shape::Rect(rect) if rect.brush.as_ref().is_some_and(|brush|brush.fill_texture_id==texture_id)=>Some(rect.rect.center()),_=>None
        }).expect("Video is clickable");
        for pressed in [true,false] {frame(&mut app,&state,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton {pos:point,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
        assert_eq!(commands.try_recv().unwrap().kind,"video-quality");
        frame(&mut app,&state,vec![]);
        let canvas=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new("call-expanded-canvas"))).unwrap();
        frame(&mut app,&state,vec![egui::Event::PointerMoved(canvas.center()),egui::Event::MouseWheel {unit:egui::MouseWheelUnit::Point,delta:egui::vec2(0.,700.),modifiers:egui::Modifiers::NONE}]);
        let expanded=ctx.data(|data|data.get_temp::<CallExpansion>(egui::Id::new("call-expanded-video"))).unwrap();assert!(expanded.zoom>1.);
        let origin=canvas.center();
        frame(&mut app,&state,vec![egui::Event::PointerMoved(origin),egui::Event::PointerButton {pos:origin,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}]);
        frame(&mut app,&state,vec![egui::Event::PointerMoved(origin+egui::vec2(70.,40.))]);
        frame(&mut app,&state,vec![egui::Event::PointerButton {pos:origin+egui::vec2(70.,40.),button:egui::PointerButton::Primary,pressed:false,modifiers:egui::Modifiers::NONE}]);
        let expanded=ctx.data(|data|data.get_temp::<CallExpansion>(egui::Id::new("call-expanded-video"))).unwrap();assert!(expanded.pan.length()>1.);
        assert_eq!(app.video_textures.len(),1);assert!(commands.try_recv().is_err(),"Zoom is local, with no per-frame IPC or capture");
        for pressed in [true,false] {frame(&mut app,&state,vec![egui::Event::PointerMoved(canvas.center()),
            egui::Event::PointerButton {pos:canvas.center(),button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);}
        assert!(ctx.data(|data|data.get_temp::<CallExpansion>(egui::Id::new("call-expanded-video"))).is_none(),"a click collapses, while the preceding drag preserved the expansion");
        assert_eq!(commands.try_recv().unwrap().text,"false");
        ctx.data_mut(|data|data.insert_temp(egui::Id::new("call-expanded-video"),expanded));
        state.call_videos.clear();prune_expanded_video(&ctx,&state);
        assert!(ctx.data(|data|data.get_temp::<CallExpansion>(egui::Id::new("call-expanded-video"))).is_none());
        frame(&mut app,&state,vec![]);
        let released=commands.try_recv().unwrap();assert_eq!(released.kind,"view-incoming-media");
        assert_eq!(serde_json::from_str::<Value>(&released.extra).unwrap()["visible"],false);
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn ptt_capture_waits_for_opening_click_release_and_accepts_all_key_families() {
        let mut capture=PttCapture::default();
        assert_eq!(capture.next(&[1]),None);
        assert_eq!(capture.next(&[]),None);
        assert_eq!(capture.next(&[0x11,0xa3]),Some(0xa3));
        for key in [1,2,4,5,6,0x1b,0x20,0x41,0x7b,0xad,0xfe] {
            let mut capture=PttCapture{released:true};
            assert_eq!(capture.next(&[key]),Some(key));
            assert!(!ptt_key_name(key).is_empty());
        }
    }
    #[test]
    fn media_settings_show_all_sources_and_never_open_hardware_without_selection() {
        let (mut app,_,mut actions)=super::super::tests::harness();
        app.devices=json!({"input":[],"output":[],"cameras":[{"id":"cam-one","name":"Synthetic camera"}],"sources":[{"id":"window:1","name":"Synthetic window","type":"window"}],"states":{"input":{"state":"empty"},"output":{"state":"empty"},"cameras":{"state":"ready"},"sources":{"state":"ready"}}});
        let state=AppSnapshot::default(); let ctx=app.ctx.clone();
        for _ in 0..2 { let _=ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1000.,950.))),..Default::default()},|ctx|app.draw_media_settings(ctx,&state)); }
        let sent:Vec<_>=std::iter::from_fn(||actions.try_recv().ok()).collect();
        assert!(!sent.iter().any(|action|matches!(action.kind.as_str(),"start-camera-preview"|"start-screen-preview"|"test-mic"|"start-camera"|"start-share")));
        assert!(app.ctx.data(|data|data.get_temp::<(String,String)>(egui::Id::new("selected-media-preview"))).is_none());
    }
    #[test]
    fn source_and_camera_previews_are_clipped_and_do_not_cover_lists_or_footer() {
        for kind in ["screen", "camera"] {
            for (pixels, scale) in [(egui::vec2(1360., 860.), 1.), (egui::vec2(2000., 1400.), 2.), (egui::vec2(700., 1100.), 1.5)] {
                let (mut app, _, mut actions) = super::super::tests::harness();
                let ctx = app.ctx.clone();ctx.set_pixels_per_point(scale);
                app.dialog_data = serde_json::Value::Array((0..50).map(|index|serde_json::json!({
                    "id":format!("source-{index}"),"name":format!("Fuente {index} · {}", "Un título de ventana muy largo que no debe invadir la vista previa ".repeat(4)),"type":"window"
                })).collect());
                app.prompt = "source-0".into();
                let texture = ctx.load_texture("synthetic-portrait-selection",egui::ColorImage::filled([20,40],Color32::GREEN),egui::TextureOptions::NEAREST);
                let texture_id = texture.id();app.video_textures.insert(99,texture);
                let state = AppSnapshot { call_videos:vec![CallVideo {id:99,kind:kind.into(),source_id:"source-0".into(),preview:true,status:"ready".into(),..Default::default()}],..Default::default() };
                let frame = |app:&mut NotDiscordUi|ctx.run(egui::RawInput {
                    screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,pixels/scale)),..Default::default()
                },|ctx| {if kind=="screen" {app.draw_source_selector(ctx,&state);} else {app.draw_camera_selector(ctx,&state,false);} });
                for _ in 0..3 {frame(&mut app);}
                let output=frame(&mut app);
                let (list,preview)=ctx.data(|data|data.get_temp::<(egui::Rect,egui::Rect)>(egui::Id::new(("media-source-layout",kind)))).unwrap();
                assert!(!list.intersects(preview),"{kind} scale={scale}: list={list:?}, preview={preview:?}");
                let clips:Vec<_>=output.shapes.iter().filter_map(|shape|match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id==texture_id =>Some(shape.clip_rect),_=>None
                }).collect();
                assert!(!clips.is_empty(),"selected preview must actually paint its synthetic texture");
                for clip in clips {assert!(preview.expand(0.01).contains_rect(clip),"texture clip escaped preview: {clip:?}, {preview:?}");}
                let footer=if kind=="screen" {"Compartir la fuente seleccionada"} else {"Encender la cámara seleccionada"};
                let footer_y=output.shapes.iter().find_map(|shape|match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text==footer=>Some(text.pos.y),_=>None
                }).expect("selection footer remains painted");
                assert!(footer_y>=list.bottom().max(preview.bottom()),"footer overlaps selection content");
                assert!(actions.try_recv().is_err(),"drawing a synthetic selection must not start capture or a call");
            }
        }
    }

    #[test]
    fn a_visible_preview_does_not_intercept_selecting_another_source() {
        for kind in ["screen","camera"] {
            let (mut app, _, mut actions)=super::super::tests::harness();let ctx=app.ctx.clone();
            app.dialog_data=serde_json::json!([
                {"id":"source-0","name":"Fuente 0","type":"window"},
                {"id":"source-1","name":"Fuente 1","type":"window"},
                {"id":"source-2","name":"Fuente 2","type":"window"}
            ]);app.prompt="source-0".into();
            let texture=ctx.load_texture("selected-preview-click-test",egui::ColorImage::filled([32,18],Color32::GREEN),egui::TextureOptions::NEAREST);
            app.video_textures.insert(99,texture);
            let state=AppSnapshot {call_videos:vec![CallVideo{id:99,kind:kind.into(),source_id:"source-0".into(),preview:true,status:"ready".into(),..Default::default()}],..Default::default()};
            let frame=|app:&mut NotDiscordUi,events|ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1360.,860.))),events,..Default::default()},|ctx| {
                if kind=="screen" {app.draw_source_selector(ctx,&state);} else {app.draw_camera_selector(ctx,&state,false);}
            });
            for _ in 0..3 {frame(&mut app,vec![]);}
            let output=frame(&mut app,vec![]);
            let point=output.shapes.iter().find_map(|shape|match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text.ends_with("Fuente 1")=>Some(text.pos+text.galley.size()/2.),_=>None
            }).unwrap();
            for pressed in [true,false] {
                frame(&mut app,vec![egui::Event::PointerMoved(point),egui::Event::PointerButton {pos:point,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);
            }
            assert_eq!(app.prompt,"source-1");
            let action=actions.try_recv().expect("selection requests exactly its preview");
            assert_eq!(action.kind,if kind=="screen" {"start-screen-preview"} else {"start-camera-preview"});
            assert_eq!(action.id,"source-1");assert!(actions.try_recv().is_err());
        }
    }
    #[test]
    fn changing_selection_replaces_only_the_preview_and_closing_releases_it() {
        let (mut app, _, mut actions) = super::super::tests::harness();
        app.select_media_preview("camera", "camera-A");
        app.select_media_preview("camera", "camera-A");
        let action = actions.try_recv().unwrap();
        assert_eq!(
            (action.kind.as_str(), action.id.as_str()),
            ("start-camera-preview", "camera-A")
        );
        assert!(actions.try_recv().is_err());
        app.select_media_preview("screen", "window-B");
        assert_eq!(actions.try_recv().unwrap().kind, "stop-media-preview");
        let action = actions.try_recv().unwrap();
        assert_eq!(
            (action.kind.as_str(), action.id.as_str()),
            ("start-screen-preview", "window-B")
        );
        app.dialog = Some(Dialog::Sources);
        app.check_selection_preview_closed();
        assert!(actions.try_recv().is_err());
        app.dialog = None;
        app.check_selection_preview_closed();
        assert_eq!(actions.try_recv().unwrap().kind, "stop-media-preview");
        app.check_selection_preview_closed();
        assert!(actions.try_recv().is_err());
    }
    #[test]
    fn participant_tile_replaces_avatar_and_restores_it_when_remote_screen_stops() {
        let (mut app, _, mut actions) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        let texture = ctx.load_texture(
            "ui4-screen-fixture",
            egui::ColorImage::filled([2, 1], Color32::from_rgb(220, 80, 40)),
            egui::TextureOptions::NEAREST,
        );
        let texture_id = texture.id();
        app.video_textures.insert(7, texture);
        let mut state = AppSnapshot {
            voice_encryption_ready: true,
            call_participants: vec![CallParticipant {
                user: Person {
                    id: "42".into(),
                    name: "Screen owner".into(),
                    ..Default::default()
                },
                screen_sharing: true,
                ..Default::default()
            }],
            call_videos: vec![CallVideo {
                id: 7,
                user_id: "42".into(),
                kind: "screen".into(),
                active: true,
                view_requested: true,
                stream_key: "private:50:42".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let frame = |app: &mut NotDiscordUi, state: &AppSnapshot| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700., 500.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.draw_participant_tiles(ui, state, Color32::WHITE, 180., false)
                    });
                },
            )
        };
        let has_image = |output: &egui::FullOutput| {
            output.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) => mesh.texture_id == texture_id,
                egui::Shape::Rect(rect) => rect
                    .brush
                    .as_ref()
                    .is_some_and(|brush| brush.fill_texture_id == texture_id),
                _ => false,
            })
        };
        frame(&mut app, &state);
        let output = frame(&mut app, &state);
        assert!(has_image(&output));
        assert_eq!(output.shapes.iter().filter(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Screen owner")).count(),1);
        assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Pantalla · Dejar de ver")));
        state.call_videos[0].active = false;
        state.call_videos[0].status = "paused".into();
        let output = frame(&mut app, &state);
        assert!(!has_image(&output));
        assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Pantalla · Mostrar")));
        state.call_videos[0].active = true;
        state.call_participants[0].screen_sharing = false;
        let output = frame(&mut app, &state);
        assert!(!has_image(&output));
        state.call_videos[0].kind = "camera".into();
        state.call_videos[0].preview = true;
        let output = frame(&mut app, &state);
        assert!(!has_image(&output));
        let release=actions.try_recv().unwrap();
        assert_eq!((release.kind.as_str(),release.text.as_str()),("view-incoming-media","screen"));
        assert_eq!(serde_json::from_str::<Value>(&release.extra).unwrap()["visible"],false);
        assert!(actions.try_recv().is_err(),"source retirement releases the selected sink without opening devices or beginning another view");
    }
    #[test]
    fn participant_camera_and_screen_have_separate_stacked_toggles_and_restore_avatar() {
        let (mut app, _, mut actions) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        ctx.style_mut(|style| style.animation_time = 0.);
        let mut texture_ids = Vec::new();
        for id in [7, 8] {
            let texture = ctx.load_texture(
                format!("media-toggle-{id}"),
                egui::ColorImage::filled([2, 1], Color32::from_rgb(id * 20, 70, 120)),
                egui::TextureOptions::NEAREST,
            );
            texture_ids.push(texture.id());
            app.video_textures.insert(id as u64, texture);
        }
        let state = AppSnapshot {
            current_user: Some(Person {
                id: "100".into(),
                ..Default::default()
            }),
            call_channel_id: "50".into(),
            call_instance_id: 17,
            voice_connected: true,
            voice_encryption_ready: true,
            call_participants: vec![CallParticipant {
                user: Person {
                    id: "42".into(),
                    name: "Camera and screen owner".into(),
                    ..Default::default()
                },
                self_video: true,
                screen_sharing: true,
                ..Default::default()
            }],
            call_videos: vec![
                CallVideo {
                    id: 7,
                    user_id: "42".into(),
                    kind: "camera".into(),
                    active: true,
                    view_requested: true,
                    ..Default::default()
                },
                CallVideo {
                    id: 8,
                    user_id: "42".into(),
                    kind: "screen".into(),
                    stream_key: "private:50:42".into(),
                    active: true,
                    view_requested: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let scope = "account:100:call:50:17";
        let frame = |app: &mut NotDiscordUi, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700., 600.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.draw_participant_tiles(ui, &state, Color32::WHITE, 180., false)
                    });
                },
            )
        };
        let has_texture = |output: &egui::FullOutput, id| {
            output.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) => mesh.texture_id == id,
                egui::Shape::Rect(rect) => rect
                    .brush
                    .as_ref()
                    .is_some_and(|brush| brush.fill_texture_id == id),
                _ => false,
            })
        };
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let initial = frame(&mut app, vec![]);
        assert!(!has_texture(&initial, texture_ids[0]));
        assert!(has_texture(&initial, texture_ids[1]));
        assert!(
            actions.try_recv().is_err(),
            "Rendering must not join a call or start viewing"
        );
        let button = |kind: &str| {
            ctx.data(|data| {
                data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new((
                    "participant-media-button",
                    scope,
                    "42",
                    kind,
                )))
            })
            .unwrap()
        };
        let (camera, camera_clip) = button("camera");
        let (screen, screen_clip) = button("screen");
        assert!(camera_clip.contains_rect(camera) && screen_clip.contains_rect(screen));
        assert!(screen.top() >= camera.bottom());
        assert!((camera.left() - screen.left()).abs() < 0.5);
        let click = |app: &mut NotDiscordUi, kind: &str| {
            let position = button(kind).0.center();
            for pressed in [true, false] {
                frame(
                    app,
                    vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            frame(app, vec![])
        };
        let camera_frame = click(&mut app, "camera");
        assert!(has_texture(&camera_frame, texture_ids[0]));
        assert!(!has_texture(&camera_frame, texture_ids[1]));
        let commands = std::iter::from_fn(|| actions.try_recv().ok()).collect::<Vec<_>>();
        assert_eq!(commands.len(), 2);
        for (action, kind, visible) in [
            (&commands[0], "screen", false),
            (&commands[1], "camera", true),
        ] {
            assert_eq!(
                (
                    action.kind.as_str(),
                    action.id.as_str(),
                    action.channel_id.as_str(),
                    action.text.as_str()
                ),
                ("view-incoming-media", "42", "50", kind)
            );
            let extra: Value = serde_json::from_str(&action.extra).unwrap();
            assert_eq!(extra["voiceAttempt"], 17);
            assert_eq!(extra["visible"], visible);
        }
        let avatar = click(&mut app, "camera");
        assert!(texture_ids.iter().all(|id| !has_texture(&avatar, *id)));
        let hidden = actions.try_recv().unwrap();
        assert_eq!(hidden.text, "camera");
        assert_eq!(
            serde_json::from_str::<Value>(&hidden.extra).unwrap()["visible"],
            false
        );
        let screen_frame = click(&mut app, "screen");
        assert!(!has_texture(&screen_frame, texture_ids[0]));
        assert!(has_texture(&screen_frame, texture_ids[1]));
        actions.try_recv().unwrap();
        let avatar = click(&mut app, "screen");
        assert!(texture_ids.iter().all(|id| !has_texture(&avatar, *id)));
        assert_eq!(
            serde_json::from_str::<Value>(&actions.try_recv().unwrap().extra).unwrap()["visible"],
            false
        );
    }
    #[test]
    fn incoming_participants_show_both_media_controls_without_joining_until_clicked() {
        for zoom in [1., 1.5] {
            let (mut app, _, mut actions) = super::super::tests::harness();
            let ctx = app.ctx.clone();
            app.prefs.zoom = zoom;
            app.theme.apply(&ctx, &app.prefs);
            ctx.style_mut(|style| style.animation_time = 0.);
            let person = Person {
                id: "42".into(),
                name: "Incoming participant with both camera and screen".into(),
                ..Default::default()
            };
            let state = AppSnapshot {
                signed_in: true,
                view: "chat".into(),
                selected_channel_id: "50".into(),
                selected_channel_kind: "group".into(),
                channel_access: ChannelAccess::private(),
                current_user: Some(Person {
                    id: "100".into(),
                    ..Default::default()
                }),
                incoming_calls: vec![IncomingCall {
                    channel_id: "50".into(),
                    channel_name: "Incoming group".into(),
                    instance_id: 88,
                    ringing: true,
                    caller: Some(person.clone()),
                    participants: vec![person.clone()],
                    call_participants: vec![CallParticipant {
                        user: person,
                        self_video: true,
                        screen_sharing: true,
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            };
            let frame = |app: &mut NotDiscordUi, events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1360. / zoom, 860. / zoom),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.draw_shell(ctx, &state),
                )
            };
            for _ in 0..4 {
                frame(&mut app, vec![]);
            }
            let scope = "account:100:incoming:50:88";
            let button = |kind: &str| {
                ctx.data(|data| {
                    data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new((
                        "participant-media-button",
                        scope,
                        "42",
                        kind,
                    )))
                })
                .unwrap()
            };
            let camera = button("camera");
            let screen = button("screen");
            assert!(
                camera.1.contains_rect(camera.0),
                "Incoming camera clipped at zoom{zoom}: {camera:?}"
            );
            assert!(
                screen.1.contains_rect(screen.0),
                "Incoming screen clipped at zoom{zoom}: {screen:?}"
            );
            assert!(screen.0.top() >= camera.0.bottom());
            let before = std::iter::from_fn(|| actions.try_recv().ok()).collect::<Vec<_>>();
            assert!(
                !before.iter().any(|action| matches!(
                    action.kind.as_str(),
                    "join-call" | "view-incoming-media" | "watch-stream"
                )),
                "Incoming render must not join or capture"
            );
            for pressed in [true, false] {
                let pos = camera.0.center();
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            let emitted = std::iter::from_fn(|| actions.try_recv().ok())
                .find(|action| action.kind == "view-incoming-media")
                .expect("Explicit incoming camera click emits join-and-view intent");
            assert_eq!(
                (
                    emitted.id.as_str(),
                    emitted.text.as_str(),
                    emitted.channel_id.as_str()
                ),
                ("42", "camera", "50")
            );
            let extra: Value = serde_json::from_str(&emitted.extra).unwrap();
            assert_eq!(extra["instanceId"], 88);
            assert_eq!(extra["visible"], true);
        }
    }
    #[test]
    fn explicit_media_survives_join_without_opening_paused_rtp_and_old_choices_are_pruned() {
        let (mut app, _, mut actions) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        let mut ids = Vec::new();
        for id in [7, 8] {
            let texture = ctx.load_texture(
                format!("join-media-{id}"),
                egui::ColorImage::filled([2, 1], Color32::from_rgb(90, 70, 150)),
                egui::TextureOptions::NEAREST,
            );
            ids.push(texture.id());
            app.video_textures.insert(id, texture);
        }
        let mut state = AppSnapshot {
            current_user: Some(Person {
                id: "100".into(),
                ..Default::default()
            }),
            call_channel_id: "50".into(),
            call_instance_id: 17,
            voice_connected: true,
            voice_encryption_ready: true,
            call_participants: vec![CallParticipant {
                user: Person {
                    id: "42".into(),
                    name: "Media owner".into(),
                    ..Default::default()
                },
                self_video: true,
                screen_sharing: true,
                ..Default::default()
            }],
            call_videos: vec![
                CallVideo {
                    id: 7,
                    user_id: "42".into(),
                    kind: "camera".into(),
                    status: "paused".into(),
                    ..Default::default()
                },
                CallVideo {
                    id: 8,
                    user_id: "42".into(),
                    kind: "screen".into(),
                    status: "active".into(),
                    active: true,
                    view_requested: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        ctx.data_mut(|data| {
            let choices = data.get_temp_mut_or_default::<HashMap<String, String>>(egui::Id::new(
                "participant-media-selection",
            ));
            choices.insert("account:100:incoming:50:88:42".into(), "screen".into());
            choices.insert("account:old:call:50:17:42".into(), "camera".into());
            choices.insert("account:100:call:50:16:42".into(), "camera".into());
        });
        let frame = |app: &mut NotDiscordUi, state: &AppSnapshot| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700., 600.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.draw_participant_tiles(ui, state, Color32::WHITE, 180., false)
                    });
                },
            )
        };
        let shown = |output: &egui::FullOutput, id| {
            output.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Rect(rect) => rect
                    .brush
                    .as_ref()
                    .is_some_and(|brush| brush.fill_texture_id == id),
                egui::Shape::Mesh(mesh) => mesh.texture_id == id,
                _ => false,
            })
        };
        frame(&mut app, &state);
        let joined = frame(&mut app, &state);
        assert!(
            !shown(&joined, ids[0]),
            "An unsolicited paused camera RTP source cannot replace the avatar or selected screen"
        );
        assert!(
            shown(&joined, ids[1]),
            "The explicitly watched source seeds the tile after joining"
        );
        assert!(
            ctx.data_mut(|data| data
                .get_temp_mut_or_default::<HashMap<String, String>>(egui::Id::new(
                    "participant-media-selection"
                ))
                .is_empty()),
            "Choices from the previous account/incoming instance/voice attempt must be removed"
        );
        state.call_videos[1].active = false;
        state.call_videos[1].status = "paused".into();
        let hidden = frame(&mut app, &state);
        assert!(
            ids.iter().all(|id| !shown(&hidden, *id)),
            "A cancelled incoming intent must remain an avatar after join"
        );
        ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<HashMap<String, String>>(egui::Id::new(
                "participant-media-selection",
            ))
            .insert("account:100:call:50:17:42".into(), "avatar".into());
        });
        state.call_videos[1].active = true;
        let stale = frame(&mut app, &state);
        assert!(
            ids.iter().all(|id| !shown(&stale, *id)),
            "An explicit hide wins over a late active snapshot"
        );
        state.call_instance_id = 18;
        let new_call = frame(&mut app, &state);
        assert!(
            shown(&new_call, ids[1]),
            "An old call's hidden selection cannot leak into the new call instance"
        );
        let release=actions.try_recv().unwrap();
        assert_eq!((release.kind.as_str(),release.text.as_str()),("view-incoming-media","screen"));
        assert_eq!(serde_json::from_str::<Value>(&release.extra).unwrap()["visible"],false);
        assert!(actions.try_recv().is_err(),"source retirement may release its sink but cannot emit join/capture commands");
    }
    #[test]
    fn incoming_call_actions_use_the_real_channel_and_cancelled_calls_disappear() {
        for (zoom, pixels) in [
            (1., [1360., 860.]),
            (1.5, [1360., 860.]),
            (1., [1000., 650.]),
            (1.5, [1000., 650.]),
            (1., [480., 650.]),
            // The rail and navigation need 378 logical points. A 320-point
            // viewport is below the native 1000-pixel minimum and has no chat.
            (1.5, [720., 650.]),
        ] {
            for (label, expected) in [("Aceptar", "join-call"), ("Rechazar", "reject-call")] {
                let (mut app, _, mut actions) = super::super::tests::harness();
                let ctx = app.ctx.clone();
                app.prefs.zoom = zoom;
                app.theme.apply(&ctx, &app.prefs);
                ctx.style_mut(|style| style.animation_time = 0.);
                let mut state = AppSnapshot {
                signed_in:true,
                view:"chat".into(),
                selected_channel_id:"50".into(),
                selected_channel_kind:"group".into(),
                channel_name:"Group fixture".into(),
                channel_access:ChannelAccess::private(),
                incoming_calls: vec![IncomingCall {
                    channel_id: "50".into(),
                    instance_id:77,
                    channel_name: "Group fixture with a very long conversation name that cannot push actions outside the panel".into(),
                    caller: Some(Person {
                        id: "42".into(),
                        name: "Caller fixture with a very long display name that must elide without clipping Aceptar or Rechazar".into(),
                        ..Default::default()
                    }),
                    participants:(0..8).map(|index|Person{id:format!("10{index}"),name:format!("Participant {index}"),..Default::default()}).collect(),
                    ringing: true,
                    ..Default::default()
                }],
                ..Default::default()
            };
                let frame = |app: &mut NotDiscordUi, state: &AppSnapshot, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(pixels[0] / zoom, pixels[1] / zoom),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            app.draw_shell(ctx, state);
                        },
                    )
                };
                for _ in 0..3 {
                    frame(&mut app, &state, vec![]);
                }
                let output = frame(&mut app, &state, vec![]);
                for (text, action) in [("Aceptar", "join-call"), ("Rechazar", "reject-call")] {
                    let (button, clip) = ctx
                        .data(|data| {
                            data.get_temp::<(egui::Rect, egui::Rect)>(egui::Id::new((
                                "incoming-action-response",
                                "50",
                                action,
                            )))
                        })
                        .unwrap();
                    assert!(
                        clip.contains_rect(button),
                        "zoom={zoom} pixels={pixels:?} {text}: button={button:?} clip={clip:?}"
                    );
                    assert!(ctx.content_rect().contains_rect(button));
                    let painted = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(painted) if painted.galley.job.text == text => {
                                Some((painted, shape.clip_rect))
                            }
                            _ => None,
                        })
                        .expect("complete incoming action text was painted");
                    assert!(!painted.0.galley.elided, "{text} was abbreviated");
                    assert!(
                        painted.1.contains_rect(
                            painted
                                .0
                                .galley
                                .mesh_bounds
                                .translate(painted.0.pos.to_vec2())
                        ),
                        "{text} glyphs were clipped"
                    );
                }
                let position = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text == label => {
                            Some(text.galley.rect.translate(text.pos.to_vec2()).center())
                        }
                        _ => None,
                    })
                    .expect("incoming action is visible");
                for pressed in [true, false] {
                    frame(
                        &mut app,
                        &state,
                        vec![
                            egui::Event::PointerMoved(position),
                            egui::Event::PointerButton {
                                pos: position,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }
                let emitted = std::iter::from_fn(|| actions.try_recv().ok()).collect::<Vec<_>>();
                let action = emitted
                    .iter()
                    .find(|action| action.kind == expected)
                    .expect("click on the visible complete button emits its action");
                assert_eq!((action.kind.as_str(), action.id.as_str()), (expected, "50"));
                assert_eq!(action.channel_id, "50");
                assert_eq!(
                    serde_json::from_str::<Value>(&action.extra).unwrap()["instanceId"],
                    77
                );
                state.incoming_calls.clear();
                let output = frame(&mut app, &state, vec![]);
                assert!(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.contains("Llamada entrante"))));
            }
        }
    }
    #[test]
    fn call_panel_darkens_the_current_base_by_half() {
        assert_eq!(
            call_background(Color32::from_rgb(72, 35, 101)),
            Color32::from_rgb(36, 17, 50)
        );
        assert_eq!(
            call_background(Color32::WHITE),
            Color32::from_rgb(127, 127, 127)
        );
        assert_eq!(call_background(Color32::BLACK), Color32::BLACK);
    }
    #[test]
    fn call_labels_and_voice_ring_remain_visible_on_extreme_custom_bases() {
        for base in [
            Color32::BLACK,
            Color32::WHITE,
            Color32::from_rgb(0, 255, 255),
            Color32::from_rgb(0, 255, 0),
            Color32::from_rgb(255, 0, 255),
            Color32::from_rgb(255, 255, 0),
            Color32::from_rgb(49, 51, 56),
        ] {
            let background = call_background(base);
            assert!(call_contrast(background, call_ink(background)) >= 4.5);
            let green = presence_color("online");
            match voice_ring_outline(background, green) {
                Some(outline) => assert!(call_contrast(background, outline) >= 3.),
                None => assert!(call_contrast(background, green) >= 3.),
            }
        }
        assert!(
            voice_ring_outline(
                call_background(Color32::from_rgb(0, 255, 255)),
                presence_color("online")
            )
            .is_some()
        );
    }
    #[test]
    fn actual_call_text_overrides_global_ink_and_panel_fills_available_width() {
        let ctx = egui::Context::default();
        let (commands, mut receive) = tokio::sync::mpsc::unbounded_channel();
        let (_, updates) = std::sync::mpsc::channel();
        let mut app = NotDiscordUi::from_context(
            &ctx,
            std::env::temp_dir().join(format!("notdiscord-voice-{}", uuid::Uuid::new_v4())),
            commands,
            updates,
        );
        app.prefs.background = "#00FFFF".into();
        app.theme = Theme::from_preferences(&app.prefs);
        app.theme.apply(&ctx, &app.prefs);
        let global_ink = ctx.style().visuals.override_text_color;
        let state = AppSnapshot {
            call_channel_name: "Visible fixture".into(),
            call_status_label: "Visible status".into(),
            call_started_at: Some(chrono::Utc::now().fixed_offset()),
            camera_preparing: true,
            call_participants: vec![CallParticipant {
                user: Person {
                    id: "100".into(),
                    name: "Visible user".into(),
                    ..Default::default()
                },
                self_mute: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("mic-test-state"),
                serde_json::json!({"active":true}),
            )
        });
        let mut edge = 0.;
        let mut output = None;
        for _ in 0..2 {
            output = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 650.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        edge = ui.max_rect().right();
                        app.draw_call(ui, &state);
                    });
                },
            ));
        }
        let output = output.unwrap();
        let background = call_background(app.theme.base);
        let ink = call_ink(background);
        assert_ne!(
            Some(ink),
            global_ink,
            "Fixture must exercise the inherited-color regression"
        );
        for needle in [
            "Llamada · Visible fixture",
            "Visible status",
            "Visible user",
            "Silenciado",
            "Selección en espera",
            "Prueba local de micrófono",
            "00:",
        ] {
            let text = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text.contains(needle) => Some(text),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing call label {needle}"));
            assert!(text.override_text_color.is_none_or(|color| color == ink));
            assert!(
                text.galley
                    .job
                    .sections
                    .iter()
                    .all(|section| section.format.color == ink),
                "Wrong rendered label color: {needle}"
            );
        }
        let panel = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill == background => Some(rect.rect),
                _ => None,
            })
            .expect("Missing call frame");
        assert!(
            (panel.right() - edge).abs() < 1.,
            "Call frame shrank instead of filling its assigned width: {panel:?}, right={edge}"
        );
        assert_eq!(ctx.style().visuals.override_text_color, global_ink);
        assert!(receive.try_recv().is_err());
    }
    #[test]
    fn bounded_video_gallery_keeps_controls_chat_and_composer_visible_at_150_percent() {
        for count in [2_u64, 4] {
            for selected in [0_u64, 1] {
                let (mut app, _, mut commands) = super::super::tests::harness();
                let ctx = app.ctx.clone();
                app.prefs.zoom = 1.5;
                app.theme.apply(&ctx, &app.prefs);
                ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new("selected-call-video"), selected)
                });
                let mut state = AppSnapshot {
                    signed_in: true,
                    view: "chat".into(),
                    selected_channel_id: "200".into(),
                    call_channel_id: "200".into(),
                    call_instance_id: 17,
                    call_channel_name: "Galería acotada".into(),
                    call_status_label: "Conexión cifrada".into(),
                    voice_connected: true,
                    voice_encryption_ready: true,
                    messages: vec![ChatMessage {
                        id: "300".into(),
                        channel_id: "200".into(),
                        content: "El chat permanece visible".into(),
                        author: Person {
                            id: "100".into(),
                            name: "Usuario sintético".into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }]
                    .into(),
                    ..Default::default()
                };
                for id in 1..=count {
                    let texture = ctx.load_texture(
                        format!("bounded-video-{id}"),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [2, 1],
                            &[255, 0, 0, 255, 0, 255, 0, 255],
                        ),
                        egui::TextureOptions::LINEAR,
                    );
                    app.video_textures.insert(id, texture);
                    state.call_videos.push(CallVideo {
                        id,
                        user_id: format!("10{id}"),
                        label: format!("Vídeo sintético {id}"),
                        kind: if id == count { "screen" } else { "camera" }.into(),
                        active: true,
                        view_requested: true,
                        ..Default::default()
                    });
                    state.call_participants.push(CallParticipant {
                        user: Person {
                            id: format!("10{id}"),
                            name: format!("Participante {id}"),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                }
                let mut output = None;
                let mut central = egui::Rect::NOTHING;
                for _ in 0..4 {
                    output = Some(ctx.run(
                        egui::RawInput {
                            // Window dimensions in pixels, divided by the zoom to
                            // reproduce the smaller logical client area.
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1000. / 1.5, 650. / 1.5),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::SidePanel::left("fixture-nav")
                                .exact_width(170.)
                                .resizable(false)
                                .show(ctx, |_| {});
                            egui::SidePanel::right("fixture-members")
                                .exact_width(120.)
                                .resizable(false)
                                .show(ctx, |_| {});
                            egui::CentralPanel::default().show(ctx, |ui| {
                                central = ui.max_rect();
                                app.draw_chat(ui, &state);
                            });
                        },
                    ));
                }
                let output = output.unwrap();
                assert_eq!(ctx.zoom_factor(), 1.5);
                let panel = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) if rect.fill == call_background(app.theme.base) => {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .expect("painted call panel");
                assert!(
                    panel.height() <= central.height() * 0.57,
                    "Call consumed chat area: {panel:?}, viewport {central:?}"
                );
                assert!(
                    panel.right() <= central.right() + 1.,
                    "Call invaded member panel: {panel:?}, {central:?}"
                );
                let composer = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill == app.theme.surface && rect.rect.width() > 200. =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .max_by(|a, b| a.bottom().total_cmp(&b.bottom()))
                    .expect("painted composer");
                assert!(
                    composer.bottom() <= central.bottom() + 1.,
                    "Composer outside viewport: {composer:?}, {central:?}"
                );
                assert!(
                    (composer.bottom() - central.bottom()).abs() < 4.,
                    "Composer left an unused strip"
                );
                assert!(
                    composer.top() - panel.bottom() > 55.,
                    "No usable message area remains"
                );
                for needle in ["Silenciar", "El chat permanece visible"] {
                    let shape=output.shapes.iter().find_map(|shape|match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text==needle=>Some((shape.clip_rect,text)),_=>None
                    }).unwrap_or_else(||panic!("Missing visible control/chat text {needle}, count={count}, selected={selected}"));
                    let rect = shape.1.galley.rect.translate(shape.1.pos.to_vec2());
                    assert!(
                        shape.0.intersect(central).contains_rect(rect),
                        "Clipped control/chat {needle}: {rect:?}"
                    );
                }
                let image_ids: Vec<_> = app
                    .video_textures
                    .values()
                    .map(egui::TextureHandle::id)
                    .collect();
                let images: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) if image_ids.contains(&mesh.texture_id) => {
                            Some(mesh.calc_bounds())
                        }
                        egui::Shape::Rect(rect)
                            if rect.brush.as_ref().is_some_and(|brush| {
                                image_ids.contains(&brush.fill_texture_id)
                            }) =>
                        {
                            Some(rect.rect)
                        }
                        _ => None,
                    })
                    .collect();
                assert!(
                    !images.is_empty(),
                    "Synthetic video was not painted: count={count} selected={selected}"
                );
                for image in images {
                    assert!(
                        image.height() < panel.height(),
                        "Video exceeded gallery budget: {image:?}"
                    );
                    assert!(
                        (image.width() / image.height() - 2.).abs() < 0.02,
                        "Video was cropped/stretched"
                    );
                }
                assert!(
                    commands.try_recv().is_err(),
                    "Rendering must not capture or transmit"
                );
            }
        }
    }
    #[test]
    fn call_panel_honors_tiny_budget_and_shrinks_without_media() {
        let (mut app, _, _) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        let state = AppSnapshot {
            call_channel_name: "Solo".into(),
            call_status_label: "Conectando".into(),
            ..Default::default()
        };
        for budget in [80., 300.] {
            let mut output = None;
            for _ in 0..3 {
                output = Some(ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(370., 500.),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| app.draw_call_in_height(ui, &state, budget));
                    },
                ));
            }
            let panel = output
                .unwrap()
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) if rect.fill == call_background(app.theme.base) => {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .unwrap();
            assert!(
                panel.height() <= budget + 1.,
                "Call exceeded explicit budget {budget}: {panel:?}"
            );
            if budget == 300. {
                assert!(
                    panel.height() < 200.,
                    "Empty gallery reserved unused video space"
                );
            }
        }
    }
    #[test]
    fn compact_call_shows_full_frames_and_can_scroll_to_last_control() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        app.prefs.zoom = 1.5;
        app.theme.apply(&ctx, &app.prefs);
        let mut state = AppSnapshot {
            call_channel_name: "130pt con medios".into(),
            voice_connected: true,
            voice_encryption_ready: true,
            ..Default::default()
        };
        for id in 1..=2 {
            app.video_textures.insert(
                id,
                ctx.load_texture(
                    format!("compact-{id}"),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [2, 1],
                        &[255, 0, 0, 255, 0, 255, 0, 255],
                    ),
                    egui::TextureOptions::LINEAR,
                ),
            );
            state.call_videos.push(CallVideo {
                id,
                label: format!("Vídeo {id}"),
                kind: "camera".into(),
                active: true,
                view_requested: true,
                ..Default::default()
            });
        }
        let ids: Vec<_> = app
            .video_textures
            .values()
            .map(egui::TextureHandle::id)
            .collect();
        let mut initial = None;
        let mut last = None;
        for frame in 0..9 {
            let events = if frame == 4 {
                vec![
                    egui::Event::PointerMoved(egui::pos2(220., 60.)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(-1500., 0.),
                        modifiers: egui::Modifiers::default(),
                    },
                ]
            } else {
                vec![]
            };
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(520., 330.),
                    )),
                    time: Some(frame as f64 * 0.25),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| app.draw_call_in_height(ui, &state, 130.));
                },
            );
            if frame == 3 {
                initial = Some(output.clone());
            }
            last = Some(output);
        }
        let initial = initial.unwrap();
        let frames: Vec<_> = initial
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect
                        .brush
                        .as_ref()
                        .is_some_and(|brush| ids.contains(&brush.fill_texture_id)) =>
                {
                    Some((rect.rect, shape.clip_rect))
                }
                _ => None,
            })
            .collect();
        assert_eq!(frames.len(), 2, "Both videos must appear in compact panel");
        for (rect, clip) in &frames {
            assert!(rect.height() >= 12., "Gallery collapsed: {rect:?}");
            assert!(
                clip.expand(0.7).contains_rect(*rect),
                "Video cropped by compact gallery: {rect:?} vs {clip:?}"
            );
        }
        assert!(
            (frames[0].0.top() - frames[1].0.top()).abs() < 0.7,
            "Tiles are not aligned at top"
        );
        let last = last.unwrap();
        let exit = last
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Salir" => Some((
                    text.galley.rect.translate(text.pos.to_vec2()),
                    shape.clip_rect,
                )),
                _ => None,
            })
            .expect("Last control reachable by horizontal wheel");
        assert!(
            exit.1.contains_rect(exit.0),
            "Last control remains clipped after scrolling"
        );
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn gallery_top_aligns_tiles_with_different_footer_heights() {
        let (mut app, _, _) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        let mut state = AppSnapshot::default();
        for id in 1..=2 {
            app.video_textures.insert(
                id,
                ctx.load_texture(
                    format!("aligned-{id}"),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [2, 1],
                        &[255, 0, 0, 255, 0, 255, 0, 255],
                    ),
                    egui::TextureOptions::LINEAR,
                ),
            );
            state.call_videos.push(CallVideo {
                id,
                label: format!("Vídeo {id}"),
                kind: "camera".into(),
                local: id == 1,
                active: true,
                view_requested: true,
                ..Default::default()
            });
        }
        let ids: Vec<_> = app
            .video_textures
            .values()
            .map(egui::TextureHandle::id)
            .collect();
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(750., 500.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        app.draw_participant_tiles(ui, &state, Color32::WHITE, 180., false)
                    });
                },
            ));
        }
        let frames: Vec<_> = output
            .unwrap()
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect
                        .brush
                        .as_ref()
                        .is_some_and(|brush| ids.contains(&brush.fill_texture_id)) =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(frames.len(), 2);
        assert!(
            (frames[0].top() - frames[1].top()).abs() < 0.7,
            "Different footer heights shifted video tops: {frames:?}"
        );
    }
}
