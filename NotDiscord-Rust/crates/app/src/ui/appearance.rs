use super::*;

fn settings_transform(open_zoom: f32, current_zoom: f32) -> egui::emath::TSTransform {
    let scale = if open_zoom.is_finite() && current_zoom.is_finite() && current_zoom > 0. {
        open_zoom / current_zoom
    } else {
        1.
    };
    egui::emath::TSTransform::from_scaling(scale)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ColorField { Background, Separator, Accent }
impl ColorField {
    fn label(self) -> &'static str {
        match self {
            Self::Background => "Color base de toda la interfaz · #RRGGBB",
            Self::Separator => "Color de líneas separadoras · #RRGGBB",
            Self::Accent => "Color de acento · #RRGGBB",
        }
    }
    fn value(self, prefs: &mut Preferences) -> &mut String {
        match self {
            Self::Background => &mut prefs.background,
            Self::Separator => &mut prefs.separator,
            Self::Accent => &mut prefs.accent_color,
        }
    }
}
fn hex_color([red, green, blue]: [u8; 3]) -> String { format!("#{red:02X}{green:02X}{blue:02X}") }
/// Child of the settings transaction: it edits the parent's draft live and
/// keeps only what it needs to give that field back when cancelled.
pub(super) struct ColorEditor {
    field: ColorField,
    original: String,
    applied: String,
    hsva: egui::ecolor::Hsva,
    rgb: [u8; 3],
    hex: String,
}
impl ColorEditor {
    fn new(field: ColorField, value: &str) -> Self {
        let color = theme::parse_color(value).unwrap_or(Color32::GRAY);
        let rgb = [color.r(), color.g(), color.b()];
        Self { field, original: value.to_owned(), applied: value.to_owned(),
            hsva: egui::ecolor::Hsva::from_srgb(rgb), rgb, hex: hex_color(rgb) }
    }
    /// Conversions run only from the control the user touched. Hue and
    /// saturation survive grays and black, where sRGB cannot carry them.
    fn set_rgb(&mut self, rgb: [u8; 3]) {
        let mut next = egui::ecolor::Hsva::from_srgb(rgb);
        if next.s == 0. || next.v == 0. { next.h = self.hsva.h; }
        if next.v == 0. { next.s = self.hsva.s; }
        self.hsva = next;
        self.rgb = rgb;
    }
}
impl NotDiscordUi {
    fn color_row(&mut self, ui: &mut egui::Ui, field: ColorField) {
        ui.label(field.label());
        ui.horizontal(|ui| {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(28., 22.), egui::Sense::click());
            if let Some(color) = theme::parse_color(field.value(&mut self.prefs)) { ui.painter().rect_filled(rect, 4., color); }
            ui.painter().rect_stroke(rect, 4., egui::Stroke::new(1_f32, self.theme.line), egui::StrokeKind::Inside);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Cambiar color"));
            if response.on_hover_text("Cambiar color").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.color_editor = Some(ColorEditor::new(field, field.value(&mut self.prefs)));
            }
            ui.add(bordered(egui::TextEdit::singleline(field.value(&mut self.prefs))).submit_action("appearance-save"));
        });
    }
    /// Restores only the field being edited; the parent keeps its other changes.
    pub(super) fn cancel_color_editor(&mut self) {
        if let Some(editor) = self.color_editor.take() { *editor.field.value(&mut self.prefs) = editor.original; }
    }
    fn draw_color_editor(&mut self, ctx: &egui::Context, transform: egui::emath::TSTransform) {
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("color-editor"));
        let Some(mut editor) = self.color_editor.take() else {
            ctx.set_transform_layer(layer, egui::emath::TSTransform::IDENTITY);
            return;
        };
        // Reset and presets rewrite the draft: this opening no longer owns it.
        if *editor.field.value(&mut self.prefs) != editor.applied {
            ctx.set_transform_layer(layer, egui::emath::TSTransform::IDENTITY);
            return;
        }
        ctx.set_transform_layer(layer, transform);
        ctx.memory_mut(|memory| memory.set_modal_layer(layer));
        let (mut open, mut accept, mut cancel, mut changed) = (true, false, false, false);
        egui::Window::new("Cambiar color").id(egui::Id::new("color-editor")).order(egui::Order::Foreground)
            .open(&mut open).collapsible(false).resizable(false)
            .pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.label(RichText::new(editor.field.label()).small().color(self.theme.muted));
                if egui::color_picker::color_picker_hsva_2d(ui, &mut editor.hsva, egui::color_picker::Alpha::Opaque) {
                    editor.rgb = editor.hsva.to_srgb();
                    editor.hex = hex_color(editor.rgb);
                    changed = true;
                }
                ui.add_space(4.);
                ui.horizontal(|ui| {
                    let mut rgb = editor.rgb;
                    let mut touched = false;
                    for (channel, prefix) in rgb.iter_mut().zip(["R ", "G ", "B "]) {
                        touched |= ui.add(egui::DragValue::new(channel).range(0..=255).prefix(prefix)).changed();
                    }
                    if touched {
                        editor.set_rgb(rgb);
                        editor.hex = hex_color(rgb);
                        changed = true;
                    }
                });
                let hex = ui.add(bordered(egui::TextEdit::singleline(&mut editor.hex).desired_width(96.)).submit_action("color-apply"));
                let valid = theme::parse_color(&editor.hex);
                if hex.changed() && let Some(color) = valid {
                    // Keep the typed buffer as is; only the draft takes the new color.
                    editor.set_rgb([color.r(), color.g(), color.b()]);
                    changed = true;
                }
                if valid.is_none() { ui.colored_label(ui.visuals().error_fg_color, "Usa el formato #RRGGBB."); }
                ui.separator();
                ui.horizontal(|ui| {
                    let submitted = super::input_ownership::take_action(ctx, "color-apply");
                    accept = (ui.add_enabled(valid.is_some(), egui::Button::new("Aplicar")).clicked() || submitted) && valid.is_some();
                    cancel = ui.button("Cancelar").clicked();
                });
            });
        if changed {
            let value = hex_color(editor.rgb);
            editor.applied = value.clone();
            *editor.field.value(&mut self.prefs) = value;
        }
        if accept { ctx.set_transform_layer(layer, egui::emath::TSTransform::IDENTITY); return; }
        self.color_editor = Some(editor);
        if cancel || !open {
            self.cancel_color_editor();
            ctx.set_transform_layer(layer, egui::emath::TSTransform::IDENTITY);
        }
    }
    pub(super) fn media_error_link(&mut self, ui: &mut egui::Ui, error: &str) -> bool {
        if super::media_limit::is_limit_error(error) {
            let response = ui.link(error).on_hover_text("Cambiar el límite de descarga de imágenes y GIFs");
            #[cfg(test)]
            ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("image-limit-warning-link"), response.rect));
            if response.clicked() {
                if !matches!(self.dialog, Some(Dialog::Settings)) {
                    self.settings_backup = Some(self.prefs.clone());
                    self.dialog = Some(Dialog::Settings);
                }
                ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("settings-focus-image-limit"), true));
                ui.ctx().send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
                return true;
            }
        } else {
            ui.label(RichText::new(error).small().color(self.theme.muted));
        }
        false
    }
    pub(super) fn draw_appearance_settings(
        &mut self,
        ctx: &egui::Context,
        state: &AppSnapshot,
        open: &mut bool,
    ) {
        let key = egui::Id::new("settings-opening-zoom");
        let current_zoom = ctx.zoom_factor();
        let opening_zoom =
            ctx.data_mut(|data| *data.get_temp_mut_or_insert_with(key, || current_zoom));
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("settings-dialog"));
        // Transform both paint and hit testing. The window keeps the physical
        // geometry it had when opened; the shell uses the current global zoom.
        ctx.set_transform_layer(layer, settings_transform(opening_zoom, current_zoom));
        let available = ctx.content_rect().size() * (current_zoom / opening_zoom.max(0.1));
        let maximum = (available - egui::vec2(32., 64.)).max(egui::vec2(280., 180.));
        let mut close = false;
        egui::Window::new("Ajustes de NotDiscord")
            .order(egui::Order::Foreground)
            .id(egui::Id::new("settings-dialog"))
            .open(open).default_size(egui::vec2(620., 720.).min(maximum)).max_size(maximum)
            .constrain(false).resizable(true).collapsible(false)
            .show(ctx, |ui| {
                theme::vertical_scroll(ui).id_salt("appearance-settings-scroll").max_height((maximum.y - 60.).clamp(100., 630.)).show(ui, |ui| {
                    ui.heading("Apariencia y accesibilidad");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Voz, vídeo y dispositivos").clicked() { self.open_media(); }
                        if ui.button("Sonidos de eventos").clicked() { self.dialog = Some(Dialog::Sounds); }
                    });
                    ui.separator();
                    ui.label(RichText::new("Escala y texto").strong());
                    ui.label("Los colores se previsualizan en toda la aplicación. Cancelar recupera el tema guardado.");
                    let zoom_control = ui.add(egui::Slider::new(&mut self.prefs.zoom, 0.8..=1.5).step_by(0.05).text("Escala de interfaz"));
                    #[cfg(test)]
                    ctx.data_mut(|data| data.insert_temp(egui::Id::new("settings-test-slider"), zoom_control.rect));
                    let _ = zoom_control;
                    ui.label(RichText::new("Este menú conserva su escala mientras esté abierto. Al volver a abrirlo usará la escala nueva.").small().color(self.theme.muted));
                    ui.add(egui::Slider::new(&mut self.prefs.chat_size, 14.0..=22.0).step_by(1.).text("Texto de mensajes"));
                    ui.checkbox(&mut self.prefs.compact, "Diseño compacto");
                    ui.separator();
                    ui.label(RichText::new("Notificaciones y escritura").strong());
                    ui.checkbox(&mut self.prefs.windows_notifications, "Notificaciones de mensajes y llamadas en Windows");
                    ui.checkbox(&mut self.prefs.send_typing, "Enviar indicador de escritura");
                    ui.label(RichText::new("Se respetan No molestar y las conversaciones silenciadas. Pulsar un aviso abre el chat sin responder llamadas.").small().color(self.theme.muted));
                    ui.separator();
                    ui.label(RichText::new("Colores y fondos").strong());
                    for field in [ColorField::Background, ColorField::Separator, ColorField::Accent] { self.color_row(ui, field); }
                    ui.horizontal_wrapped(|ui| {
                        for (name, bg, line, accent) in [("Discord", "#313338", "#45474D", "#5865F2"), ("Noche azul", "#13263A", "#36516A", "#54BDFF"), ("Bosque", "#142B25", "#386652", "#60D49A"), ("Claro", "#F1F3F6", "#ABB4C1", "#4055CB")] {
                            if ui.button(name).clicked() { self.prefs.background = bg.into(); self.prefs.separator = line.into(); self.prefs.accent_color = accent.into(); }
                        }
                    });
                    ui.add(egui::Slider::new(&mut self.prefs.opacity, 0.2..=1.).step_by(0.01).text("Opacidad de fondos"));
                    ui.label(RichText::new("La transparencia de los fondos permite ver las ventanas de detrás. El texto, los iconos y los medios conservan su opacidad.").small().color(self.theme.muted));
                    let mut blur = self.prefs.blur_fraction();
                    if ui.add(egui::Slider::new(&mut blur, 0.0..=1.0).step_by(0.01).fixed_decimals(2).text("Intensidad de desenfoque")).changed() { self.prefs.set_blur_fraction(blur); }
                    ui.label(RichText::new("Mezcla gradualmente el fondo original con el desenfoque de Windows. El texto, las líneas y los medios permanecen nítidos. Con fondos opacos el efecto no se activa.").small().color(self.theme.muted));
                    if let Some(error) = &self.backdrop.error { ui.colored_label(ui.visuals().error_fg_color, error); }
                    ui.separator();
                    ui.label(RichText::new("Imágenes y animaciones").strong());
                    ui.checkbox(&mut self.prefs.allow_gif_playback, "Permitir animaciones GIF y emojis personalizados");
                    ui.label(RichText::new("Los emojis personalizados se animan sólo al pasar el puntero por su sección visible.").small().color(self.theme.muted));
                    let limit_setting = ui.group(|ui| {
                        ui.label("Descarga de imágenes y GIFs");
                        ui.add(egui::Slider::new(&mut self.prefs.image_download_limit_mib, 1..=super::media_limit::MAX_MIB).suffix(" MiB").text("Límite por archivo"));
                        ui.label(RichText::new("Se aplica a vistas previas, reproducción, visor, copia y guardado de originales. Los límites de resolución y fotogramas siguen vigentes. No cambia el tamaño permitido al enviar adjuntos.").small().color(self.theme.muted));
                    }).response;
                    #[cfg(test)]
                    ctx.data_mut(|data| data.insert_temp(egui::Id::new("settings-image-limit-rect"), limit_setting.rect.intersect(ui.clip_rect())));
                    if ctx.data_mut(|data| data.remove_temp::<bool>(egui::Id::new("settings-focus-image-limit"))).unwrap_or(false) {
                        ui.scroll_to_rect_animation(limit_setting.rect, Some(egui::Align::Center), egui::style::ScrollAnimation::none());
                    }
                    ui.separator();
                    ui.label(RichText::new("Actividad de juegos y Discord RPC").strong());
                    if ui.button(if state.game_activity_enabled { "Ocultar actividad de juego en Discord" } else { "Mostrar actividad de juego en Discord" }).clicked() { self.simple("toggle-game-activity"); }
                    ui.checkbox(&mut self.prefs.detect_games, "Detectar juegos y recibir Discord RPC en este equipo");
                    if !state.game_detection_note.is_empty() { ui.label(RichText::new(&state.game_detection_note).small().color(self.theme.muted)); }
                    ui.label(RichText::new("Se reconocen ejecutables y ventanas de juegos compatibles y se recibe su actividad por Discord RPC local. No se lee la memoria de los juegos ni se guarda la lista de procesos. Sólo se comparte actividad cuando es visible en Discord y tu estado permite mostrarla.").small().color(self.theme.muted));
                    if !state.custom_activity_label.is_empty() { ui.label(format!("Actividad actual: {}", state.custom_activity_label)); }
                    if ui.button("Publicar actividad personalizada…").clicked() { self.prompt_for("Actividad personalizada", "Nombre que se publicará como «Jugando a…». Tiene prioridad sobre la detección automática.", "custom-activity", "", "", "", false); }
                    if ui.button("Quitar actividad personalizada").clicked() { self.simple("custom-activity"); }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Restablecer").clicked() { self.prefs = Preferences::default(); }
                    if ui.button("Cancelar").clicked() { if let Some(p) = self.settings_backup.take() { self.prefs = p; } close = true; }
                    if ui.button("Guardar").clicked() || super::input_ownership::take_action(ctx,"appearance-save") {
                        if theme::parse_color(&self.prefs.background).is_none() || theme::parse_color(&self.prefs.separator).is_none() || theme::parse_color(&self.prefs.accent_color).is_none() { self.error = "Usa colores como #10151C.".into(); }
                        else { match self.prefs.save(&self.data_dir) { Ok(()) => { self.settings_backup = None; self.send("ui-settings", "", serde_json::to_string(&self.prefs).unwrap_or_default(), "", ""); close = true; }, Err(e) => self.error = e } }
                    }
                });
            });
        self.draw_color_editor(ctx, settings_transform(opening_zoom, current_zoom));
        if theme::parse_color(&self.prefs.background).is_some() {
            self.theme = Theme::from_preferences(&self.prefs);
            self.theme.apply(ctx, &self.prefs);
        }
        if close {
            self.dialog = None;
        }
        if close || !*open || !matches!(self.dialog, Some(Dialog::Settings)) {
            self.cancel_color_editor();
            ctx.set_transform_layer(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("color-editor")), egui::emath::TSTransform::IDENTITY);
            ctx.data_mut(|data| data.remove::<f32>(key));
            ctx.set_transform_layer(layer, egui::emath::TSTransform::IDENTITY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_editor_keeps_hue_through_gray_and_restores_only_its_own_field() {
        let mut editor = ColorEditor::new(ColorField::Accent, "#FF0000");
        let hue = editor.hsva.h;
        editor.set_rgb([128, 128, 128]);
        assert_eq!(editor.hsva.h, hue);
        editor.set_rgb([0, 0, 0]);
        assert_eq!(editor.hsva.h, hue);
        assert_eq!(hex_color([0x12, 0xAB, 0x05]), "#12AB05");
        let (mut app, _, _) = super::super::tests::harness();
        app.prefs.separator = "#111111".into();
        app.prefs.accent_color = "#222222".into();
        app.color_editor = Some(ColorEditor::new(ColorField::Accent, "#222222"));
        app.prefs.accent_color = "#333333".into();
        app.cancel_color_editor();
        assert_eq!((app.prefs.accent_color.as_str(), app.prefs.separator.as_str()), ("#222222", "#111111"));
    }
    #[test]
    fn orderly_close_never_saves_an_unaccepted_preview_or_an_invalid_color() {
        let (mut app, _, _) = super::super::tests::harness();
        app.settings_backup = Some(app.prefs.clone());
        app.prefs.background = "#12".into();
        assert!(app.prepare_orderly_close().is_err());
        assert!(app.settings_backup.is_some());
    }
    #[test]
    fn size_warning_uses_link_cursor_and_opens_settings_scrolled_to_its_exact_control() {
        let (mut app,_,_)=super::super::tests::harness();
        let ctx=app.ctx.clone();
        let screen=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1100.,620.));
        let warning="La imagen supera el límite local de 25 MiB.";
        let frame=|app:&mut NotDiscordUi,events|ctx.run(egui::RawInput {screen_rect:Some(screen),events,..Default::default()},|ctx| {
            egui::CentralPanel::default().show(ctx,|ui| {app.media_error_link(ui,warning);});
        });
        frame(&mut app,vec![]);
        let link=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new("image-limit-warning-link"))).unwrap();
        let output=frame(&mut app,vec![egui::Event::PointerMoved(link.center())]);
        assert_eq!(output.platform_output.cursor_icon,egui::CursorIcon::PointingHand);
        for pressed in [true,false] {
            frame(&mut app,vec![egui::Event::PointerButton {pos:link.center(),button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]);
        }
        assert!(matches!(app.dialog,Some(Dialog::Settings)));
        assert!(app.settings_backup.is_some());
        for _ in 0..5 {
            let _ = ctx.run(egui::RawInput {screen_rect:Some(screen),..Default::default()},|ctx| {
                egui::CentralPanel::default().show(ctx,|_|{});
                app.draw_dialog(ctx,&AppSnapshot::welcome());
            });
        }
        let visible=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new("settings-image-limit-rect"))).unwrap();
        assert!(visible.is_positive() && visible.height()>60.,"The setting must be in the settings viewport after clicking its warning: {visible:?}");
        assert!(ctx.data(|data|data.get_temp::<bool>(egui::Id::new("settings-focus-image-limit"))).is_none(),"The one-shot deep link must not override subsequent manual settings scrolling");
    }
    #[test]
    fn open_settings_cancel_the_global_zoom_in_physical_coordinates() {
        for opening in [0.8, 1., 1.5] {
            for current in [0.8, 1., 1.5] {
                let point = egui::pos2(240., 360.);
                let transformed = settings_transform(opening, current) * point;
                assert!((transformed.x * current - point.x * opening).abs() < 0.001);
                assert!((transformed.y * current - point.y * opening).abs() < 0.001);
            }
        }
    }
    #[test]
    fn settings_window_and_slider_hold_physical_geometry_during_live_zoom_and_reopen_at_new_scale()
    {
        let (mut app, _, _) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        let state = AppSnapshot::welcome();
        app.dialog = Some(Dialog::Settings);
        let id = egui::Id::new("settings-dialog");
        let layer = egui::LayerId::new(egui::Order::Foreground, id);
        let frame = |app: &mut NotDiscordUi, events| {
            let zoom = ctx.zoom_factor();
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1600., 1200.) / zoom,
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |_| {});
                    app.draw_dialog(ctx, &state);
                },
            )
        };
        let physical = || {
            let rect = ctx.memory(|memory| memory.area_rect(id)).unwrap();
            let transform = ctx
                .layer_transform_to_global(layer)
                .unwrap_or(egui::emath::TSTransform::IDENTITY);
            let rect = transform * rect;
            egui::Rect::from_min_max(rect.min * ctx.zoom_factor(), rect.max * ctx.zoom_factor())
        };
        for _ in 0..5 {
            frame(&mut app, vec![]);
        }
        let initial = physical();
        let slider = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("settings-test-slider")))
            .unwrap();
        app.prefs.zoom = 1.5;
        for _ in 0..5 {
            frame(&mut app, vec![]);
        }
        let scaled = physical();
        assert!(
            (scaled.min - initial.min).length() < 2.,
            "position changed: {initial:?} -> {scaled:?}"
        );
        assert!(
            (scaled.size() - initial.size()).length() < 2.,
            "size changed: {initial:?} -> {scaled:?}"
        );
        let next_slider = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("settings-test-slider")))
            .unwrap();
        assert!((next_slider.center() - slider.center()).length() < 1.);
        // Send pointer events in transformed global coordinates, exactly as the
        // native integration does, and verify that the fixed slider still works.
        let local = egui::pos2(next_slider.left() + 8., next_slider.center().y);
        let target = ctx.layer_transform_to_global(layer).unwrap() * local;
        for pressed in [true, false] {
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(target),
                    egui::Event::PointerButton {
                        pos: target,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(
            app.prefs.zoom < 1.5,
            "transformed slider did not receive pointer input"
        );
        // Close by switching dialogs through the same cleanup branch.
        app.dialog = None;
        let mut open = false;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1600., 1200.) / ctx.zoom_factor(),
                )),
                ..Default::default()
            },
            |ctx| app.draw_appearance_settings(ctx, &state, &mut open),
        );
        assert!(ctx.layer_transform_to_global(layer).is_none());
        app.dialog = Some(Dialog::Settings);
        for _ in 0..5 {
            frame(&mut app, vec![]);
        }
        let opening = ctx
            .data(|data| data.get_temp::<f32>(egui::Id::new("settings-opening-zoom")))
            .unwrap();
        assert!((opening - ctx.zoom_factor()).abs() < 0.001);
    }
}
