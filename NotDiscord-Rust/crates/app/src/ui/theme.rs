use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::path::Path;

fn center_scrollbar_in_frame(ui: &mut egui::Ui, horizontal: bool) {
    // egui reserves the scrollbar inside the content rectangle. Its symmetric
    // 3pt margins otherwise ignore the enclosing frame's padding, leaving the
    // handle visibly against the content. Move it by half that extra space,
    // preserving the 8pt handle and 14pt layout gutter exactly.
    let end = if horizontal { ui.max_rect().bottom() } else { ui.max_rect().right() };
    let clip_end = if horizontal { ui.clip_rect().bottom() } else { ui.clip_rect().right() };
    let mut padding = 0.;
    for entry in ui.stack().iter() {
        if entry.kind() == Some(egui::UiKind::ScrollArea) {
            // Nested scrolls must stay within their outer viewport; never
            // compensate the same frame twice or paint across its clipping.
            break;
        }
        if entry.kind() == Some(egui::UiKind::Frame) || entry.is_panel_ui() || entry.kind() == Some(egui::UiKind::Window) {
            let frame_end = if horizontal { entry.max_rect.bottom() } else { entry.max_rect.right() };
            if (frame_end - end).abs() < 0.5 {
                let margin = if horizontal { entry.info.frame.inner_margin.bottom } else { entry.info.frame.inner_margin.right };
                padding = (margin as f32 + entry.info.frame.stroke.width)
                    .min((clip_end - end).max(0.)).clamp(0., 32.);
            }
            break;
        }
    }
    let style = &mut ui.spacing_mut().scroll;
    style.bar_inner_margin = 3. + padding * 0.5;
    style.bar_outer_margin = 3. - padding * 0.5;
}
pub(super) fn vertical_scroll(ui: &mut egui::Ui) -> super::autoscroll::ScrollArea {
    center_scrollbar_in_frame(ui, false);
    super::autoscroll::ScrollArea::vertical()
}
pub(super) fn horizontal_scroll(ui: &mut egui::Ui) -> super::autoscroll::ScrollArea {
    center_scrollbar_in_frame(ui, true);
    super::autoscroll::ScrollArea::horizontal()
}

pub(super) fn apply_interactive_cursor(ctx: &egui::Context) {
    // Include custom painted click targets, not just egui's built-in links.
    // Text edits, selectable labels, sliders and drag handles keep their own
    // cursor. Rich links that also support selection opt in at their response.
    let cursor = ctx.output(|output| output.cursor_icon);
    if !matches!(cursor, egui::CursorIcon::Default | egui::CursorIcon::Text) { return; }
    let hovered = ctx.interaction_snapshot(|snapshot| snapshot.hovered.iter().copied().collect::<Vec<_>>());
    if hovered.into_iter().any(|id| ctx.read_response(id).is_some_and(|response|
        response.enabled() && response.hovered() && response.sense.senses_click() && !response.sense.senses_drag())) {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Preferences {
    pub background: String,
    pub separator: String,
    pub accent_color: String,
    pub opacity: f32,
    pub blur: f32,
    pub zoom: f32,
    pub chat_size: f32,
    pub compact: bool,
    pub allow_gif_playback: bool,
    pub image_download_limit_mib: u32,
    pub windows_notifications: bool,
    pub send_typing: bool,
    pub show_dm_profile: bool,
    pub show_group_members: bool,
    pub show_server_members: bool,
    pub detect_games: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            background: "#313338".into(),
            separator: "#25262B".into(),
            accent_color: "#5865F2".into(),
            opacity: 1.,
            blur: 0.,
            zoom: 1.,
            chat_size: 16.,
            compact: false,
            allow_gif_playback: true,
            image_download_limit_mib: super::media_limit::DEFAULT_MIB,
            windows_notifications: true,
            send_typing: true,
            show_dm_profile: false,
            show_group_members: true,
            show_server_members: true,
            detect_games: true,
        }
    }
}
impl Preferences {
    pub(super) fn image_download_limit_bytes(&self) -> u64 {
        super::media_limit::bytes(self.image_download_limit_mib)
    }
    pub(super) fn blur_fraction(&self) -> f32 {
        if self.blur.is_finite() { self.blur.clamp(0., 100.) / 100. } else { 0. }
    }
    pub(super) fn set_blur_fraction(&mut self, amount: f32) {
        // Keep the existing on-disk 0..100 contract and native blur strength.
        self.blur = if amount.is_finite() { (amount.clamp(0., 1.) * 100.).round() } else { 0. };
    }
    pub fn load(directory: &Path) -> Self {
        let mut preferences: Self = crate::storage::read_json(&directory.join("ui-settings.json"), 16384);
        preferences.normalize();
        preferences
    }
    fn normalize(&mut self) {
        self.image_download_limit_mib = self.image_download_limit_mib.clamp(1, super::media_limit::MAX_MIB);
        self.blur = if self.blur.is_finite() {
            self.blur.clamp(0., 100.)
        } else {
            0.
        };
        self.opacity = if self.opacity.is_finite() {
            self.opacity.clamp(0.2, 1.)
        } else {
            1.
        };
        self.zoom = if self.zoom.is_finite() {
            self.zoom.clamp(0.8, 1.5)
        } else {
            1.
        };
        self.chat_size = if self.chat_size.is_finite() {
            self.chat_size.clamp(14., 22.)
        } else {
            16.
        };
    }
    pub fn save(&mut self, directory: &Path) -> Result<(), String> {
        self.normalize();
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        crate::storage::save_json(&directory.join("ui-settings.json"), self)
    }
}
#[derive(Clone, Copy)]
pub struct Theme {
    pub base: Color32,
    pub panel: Color32,
    pub rail: Color32,
    pub surface: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub line: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_surface: Color32,
    pub input: Color32,
}
pub fn parse_color(s: &str) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    Some(Color32::from_rgb(
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ))
}
fn mix(a: Color32, b: Color32, t: f64) -> Color32 {
    Color32::from_rgb(
        (a.r() as f64 + (b.r() as f64 - a.r() as f64) * t).round_ties_even() as u8,
        (a.g() as f64 + (b.g() as f64 - a.g() as f64) * t).round_ties_even() as u8,
        (a.b() as f64 + (b.b() as f64 - a.b() as f64) * t).round_ties_even() as u8,
    )
}
fn luminance(c: Color32) -> f64 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    let f = |x: u8| {
        let v = x as f64 / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}
fn surface_alpha(color: Color32, opacity: f32) -> Color32 {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let opacity = if opacity.is_finite() {
        opacity.clamp(0.2, 1.)
    } else {
        1.
    };
    Color32::from_rgba_unmultiplied(r, g, b, (opacity * 255.).round() as u8)
}
fn readable(c: Color32, surfaces: &[Color32], min: f64, toward: Color32) -> Color32 {
    for i in 0..=100 {
        let x = mix(c, toward, i as f64 / 100.);
        if surfaces.iter().all(|b| {
            let a = luminance(x);
            let b = luminance(*b);
            (a.max(b) + 0.05) / (a.min(b) + 0.05) >= min
        }) {
            return x;
        }
    }
    toward
}
impl Theme {
    pub fn from_preferences(p: &Preferences) -> Self {
        let base = parse_color(&p.background).unwrap_or(Color32::from_rgb(49, 51, 56));
        let light = luminance(base) > 0.35;
        let toward = if light {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        let near = if light {
            Color32::WHITE
        } else {
            Color32::BLACK
        };
        let accent = parse_color(&p.accent_color).unwrap_or(Color32::from_rgb(88, 101, 242));
        let mut all = [
            base,
            mix(base, Color32::BLACK, if light { 0.04 } else { 0.10 }),
            mix(base, Color32::BLACK, if light { 0.09 } else { 0.34 }),
            mix(base, toward, 0.055),
            mix(base, toward, 0.11),
            mix(base, toward, 0.15),
            mix(base, near, 0.20),
            mix(base, near, 0.10),
        ];
        for surface in &mut all {
            let a = luminance(*surface);
            let b = luminance(toward);
            if (a.max(b) + 0.05) / (a.min(b) + 0.05) < 5.2 {
                *surface = readable(*surface, &[toward], 5.2, near);
            }
        }
        Self {
            base: surface_alpha(all[0], p.opacity),
            panel: surface_alpha(all[1], p.opacity),
            rail: surface_alpha(all[2], p.opacity),
            surface: surface_alpha(all[3], p.opacity),
            hover: surface_alpha(all[4], p.opacity),
            selected: surface_alpha(all[5], p.opacity),
            input: surface_alpha(all[7], p.opacity),
            line: readable(
                parse_color(&p.separator).unwrap_or(all[1]),
                &all[..2],
                1.35,
                toward,
            ),
            ink: readable(
                if light {
                    Color32::from_rgb(27, 29, 34)
                } else {
                    Color32::from_rgb(242, 243, 245)
                },
                &all,
                4.5,
                toward,
            ),
            muted: readable(
                if light {
                    Color32::from_rgb(80, 83, 91)
                } else {
                    Color32::from_rgb(172, 177, 185)
                },
                &all,
                4.5,
                toward,
            ),
            accent: readable(accent, &all, 4.5, toward),
            accent_surface: surface_alpha(
                readable(accent, &[Color32::WHITE], 4.5, Color32::BLACK),
                p.opacity,
            ),
        }
    }
    pub fn apply(&self, ctx: &egui::Context, p: &Preferences) {
        let mut style = (*ctx.style()).clone();
        style.visuals = if luminance(self.base) > 0.35 {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        style.visuals.panel_fill = self.base;
        style.visuals.window_fill = self.base;
        style.visuals.extreme_bg_color = self.input;
        style.visuals.faint_bg_color = self.surface;
        style.visuals.override_text_color = Some(self.ink);
        style.visuals.weak_text_color = Some(self.muted);
        style.visuals.widgets.inactive.bg_fill = self.surface;
        style.visuals.widgets.inactive.weak_bg_fill = self.surface;
        style.visuals.widgets.inactive.fg_stroke.color = self.ink;
        style.visuals.widgets.hovered.bg_fill = self.hover;
        style.visuals.widgets.hovered.weak_bg_fill = self.hover;
        style.visuals.widgets.active.bg_fill = self.selected;
        style.visuals.widgets.active.weak_bg_fill = self.selected;
        style.visuals.widgets.open.bg_fill = self.selected;
        style.visuals.widgets.open.weak_bg_fill = self.selected;
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, self.line);
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
        style.visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
        style.visuals.widgets.open.bg_stroke = egui::Stroke::NONE;
        style.visuals.widgets.hovered.fg_stroke.color = self.ink;
        style.visuals.widgets.active.fg_stroke.color = self.ink;
        style.visuals.widgets.open.fg_stroke.color = self.ink;
        // Text selection is an invariant, independent of theme and opacity.
        style.visuals.selection.bg_fill = Color32::from_rgb(0, 120, 215);
        style.visuals.selection.stroke.color = Color32::WHITE;
        style.visuals.window_stroke.color = self.line;
        style.visuals.hyperlink_color = self.accent;
        style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        style.spacing.item_spacing = egui::vec2(7., 5.);
        style.spacing.button_padding = egui::vec2(9., 7.);
        // Solid bars reserve their gutter, so hovering never covers content or
        // changes either the track width or the handle width. Equal floating
        // widths also preserve that contract in a locally floated ScrollArea.
        style.spacing.scroll = egui::style::ScrollStyle {
            bar_width: 8.,
            floating_width: 8.,
            floating_allocated_width: 8.,
            bar_inner_margin: 3.,
            bar_outer_margin: 3.,
            foreground_color: true,
            dormant_background_opacity: 1.,
            active_background_opacity: 1.,
            interact_background_opacity: 1.,
            dormant_handle_opacity: 1.,
            active_handle_opacity: 1.,
            interact_handle_opacity: 1.,
            ..egui::style::ScrollStyle::solid()
        };
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
        ctx.set_style(style);
        ctx.set_zoom_factor(p.zoom.clamp(0.8, 1.5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_limit_migrates_old_settings_normalizes_and_round_trips() {
        let old: Preferences = serde_json::from_value(serde_json::json!({"Compact":true})).unwrap();
        assert_eq!(old.image_download_limit_mib, 25);
        assert_eq!(old.image_download_limit_bytes(), 25 * 1024 * 1024);
        for (input, expected) in [(0,1),(1,1),(50,50),(256,256),(u32::MAX,256)] {
            let mut prefs: Preferences = serde_json::from_value(serde_json::json!({"ImageDownloadLimitMib":input})).unwrap();
            prefs.normalize();
            assert_eq!(prefs.image_download_limit_mib, expected);
            let value = serde_json::to_value(&prefs).unwrap();
            assert_eq!(value["ImageDownloadLimitMib"], expected);
        }
    }
    #[test]
    fn buttons_use_hand_cursor_while_plain_selectable_text_keeps_text_cursor() {
        let ctx = egui::Context::default();
        let prefs = Preferences::default();
        Theme::from_preferences(&prefs).apply(&ctx,&prefs);
        let mut button = egui::Rect::NOTHING;
        let mut label = egui::Rect::NOTHING;
        let mut draw = |pointer| ctx.run(egui::RawInput {events:vec![egui::Event::PointerMoved(pointer)],..Default::default()},|ctx| {
            egui::CentralPanel::default().show(ctx,|ui| {
                button = ui.button("Clickable").rect;
                label = ui.add(egui::Label::new("Selectable text").selectable(true)).rect;
            });
        });
        draw(egui::pos2(500.,500.));
        // End the mutable borrow before the actual hit-testing passes.
        let (button,label)=(button,label);
        let draw = |pointer| ctx.run(egui::RawInput {events:vec![egui::Event::PointerMoved(pointer)],..Default::default()},|ctx| {
            egui::CentralPanel::default().show(ctx,|ui| {
                let _ = ui.button("Clickable");
                ui.add(egui::Label::new("Selectable text").selectable(true));
            });
            apply_interactive_cursor(ctx);
        });
        assert_eq!(draw(button.center()).platform_output.cursor_icon,egui::CursorIcon::PointingHand);
        assert_eq!(draw(label.center()).platform_output.cursor_icon,egui::CursorIcon::Text);
    }
    #[test]
    fn fractional_blur_preserves_every_existing_integer_setting_and_json_scale() {
        for legacy in 0..=100 {
            let mut p: Preferences = serde_json::from_value(serde_json::json!({"Blur":legacy})).unwrap();
            let fraction = p.blur_fraction();
            assert!((fraction - legacy as f32 / 100.).abs() < 0.00001);
            p.set_blur_fraction(fraction);
            assert_eq!(p.blur, legacy as f32);
            assert_eq!(serde_json::to_value(&p).unwrap()["Blur"].as_f64(), Some(legacy as f64));
        }
        let mut p = Preferences::default();
        p.set_blur_fraction(0.555);
        assert_eq!(p.blur, 56.);
        p.set_blur_fraction(f32::NAN);
        assert_eq!(p.blur, 0.);
    }
    #[test]
    fn fields_have_idle_borders_and_muted_hints_without_outlining_buttons() {
        let ctx = egui::Context::default();
        let p = Preferences::default();
        let theme = Theme::from_preferences(&p);
        theme.apply(&ctx, &p);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut text = String::new();
                ui.add(crate::ui::bordered(
                    egui::TextEdit::singleline(&mut text).hint_text("Hint fixture"),
                ));
                let _ = ui.button("Button fixture");
            });
        });
        assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(rect) if rect.stroke.width==1. && rect.stroke.color==theme.line && rect.rect.width()>100.)));
        assert_eq!(ctx.style().visuals.weak_text_color(), theme.muted);
        assert_ne!(theme.muted, theme.ink);
        let hint = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Hint fixture" => Some(text),
                _ => None,
            })
            .expect("idle field hint must be visible");
        assert_eq!(hint.fallback_color, theme.muted);
        assert!(
            hint.galley
                .job
                .sections
                .iter()
                .all(|section| section.format.color != theme.ink)
        );
        assert_eq!(
            ctx.style().visuals.widgets.inactive.bg_stroke,
            egui::Stroke::NONE
        );
    }
    #[test]
    fn baseline_surface_formula_is_preserved() {
        let t = Theme::from_preferences(&Preferences::default());
        assert_eq!(t.base, Color32::from_rgb(49, 51, 56));
        assert_eq!(t.panel, Color32::from_rgb(44, 46, 50));
        assert_eq!(t.rail, Color32::from_rgb(32, 34, 37));
        assert_eq!(t.surface, Color32::from_rgb(60, 62, 67));
        assert_eq!(t.selected, Color32::from_rgb(80, 82, 86));
    }
    #[test]
    fn text_keeps_baseline_contrast_for_dark_light_and_extreme_colors() {
        for color in ["#313338", "#F1F3F6", "#300053", "#FF00FF", "#777777"] {
            let p = Preferences {
                background: color.into(),
                ..Default::default()
            };
            let t = Theme::from_preferences(&p);
            for ink in [t.ink, t.muted, t.accent] {
                for surface in [
                    t.base, t.panel, t.rail, t.surface, t.hover, t.selected, t.input,
                ] {
                    let a = luminance(ink);
                    let b = luminance(surface);
                    assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5, "{color}");
                }
            }
        }
    }
    #[test]
    fn malformed_unicode_color_is_rejected_without_slicing_panic() {
        assert!(parse_color("#😀aa").is_none());
        assert!(parse_color("#00FFa1").is_some());
        assert!(parse_color("#zzzzzz").is_none());
    }
    #[test]
    fn opacity_changes_only_surface_alpha_and_preserves_opaque_ink_and_lines() {
        for background in ["#313338", "#F1F3F6", "#300053"] {
            let mut preferences = Preferences {
                background: background.into(),
                ..Default::default()
            };
            let opaque = Theme::from_preferences(&preferences);
            preferences.opacity = 0.35;
            let transparent = Theme::from_preferences(&preferences);
            for (surface, original) in [
                (transparent.base, opaque.base),
                (transparent.panel, opaque.panel),
                (transparent.rail, opaque.rail),
                (transparent.surface, opaque.surface),
                (transparent.hover, opaque.hover),
                (transparent.selected, opaque.selected),
                (transparent.input, opaque.input),
                (transparent.accent_surface, opaque.accent_surface),
            ] {
                assert_eq!(surface, surface_alpha(original, 0.35));
                assert_eq!(surface.a(), 89);
            }
            for (ink, original) in [
                (transparent.ink, opaque.ink),
                (transparent.muted, opaque.muted),
                (transparent.line, opaque.line),
                (transparent.accent, opaque.accent),
            ] {
                assert_eq!(ink, original);
                assert_eq!(ink.a(), 255);
            }
            let ctx = egui::Context::default();
            transparent.apply(&ctx, &preferences);
            let style = ctx.style();
            assert_eq!(style.visuals.dark_mode, luminance(opaque.base) <= 0.35);
            assert_eq!(style.visuals.panel_fill.a(), 89);
            assert_eq!(style.visuals.selection.bg_fill, Color32::from_rgb(0, 120, 215));
            assert_eq!(style.visuals.selection.stroke.color.a(), 255);
            assert_eq!(style.visuals.hyperlink_color.a(), 255);
            assert_eq!(
                style.visuals.widgets.noninteractive.bg_stroke.color.a(),
                255
            );
            for widget in [
                style.visuals.widgets.inactive,
                style.visuals.widgets.hovered,
                style.visuals.widgets.active,
                style.visuals.widgets.open,
            ] {
                assert_eq!(widget.fg_stroke.color.a(), 255);
            }
        }
    }
    #[test]
    fn saving_preferences_preserves_opacity_and_sidebar_flags_with_legacy_defaults() {
        let directory =
            std::env::temp_dir().join(format!("notdiscord-theme-{}", uuid::Uuid::new_v4()));
        let mut preferences = Preferences {
            opacity: 0.35,
            show_dm_profile: true,
            show_group_members: false,
            show_server_members: false,
            send_typing: false,
            ..Default::default()
        };
        preferences.save(&directory).unwrap();
        let saved = Preferences::load(&directory);
        assert_eq!(saved.opacity, 0.35);
        assert!(saved.show_dm_profile);
        assert!(!saved.show_group_members);
        assert!(!saved.show_server_members);
        assert!(!saved.send_typing);
        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("ui-settings.json")).unwrap())
                .unwrap();
        assert!((document["Opacity"].as_f64().unwrap() - 0.35).abs() < 0.000001);
        assert_eq!(document["ShowDmProfile"], true);
        assert_eq!(document["SendTyping"], false);
        std::fs::write(
            directory.join("ui-settings.json"),
            br##"{"Background":"#313338","Opacity":0.4}"##,
        )
        .unwrap();
        let legacy = Preferences::load(&directory);
        assert_eq!(legacy.opacity, 0.4);
        assert!(!legacy.show_dm_profile);
        assert!(legacy.show_group_members && legacy.show_server_members);
        assert!(
            legacy.send_typing,
            "Old settings retain their typing preference behavior"
        );
        std::fs::remove_file(directory.join("ui-settings.json")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn preferences_reject_non_finite_ranges_without_forcing_normal_opacity_to_one() {
        let mut preferences = Preferences {
            opacity: f32::NAN,
            zoom: f32::INFINITY,
            chat_size: f32::NEG_INFINITY,
            ..Default::default()
        };
        preferences.normalize();
        assert_eq!(
            (preferences.opacity, preferences.zoom, preferences.chat_size),
            (1., 1., 16.)
        );
        preferences.opacity = -3.;
        preferences.zoom = 20.;
        preferences.chat_size = 8.;
        preferences.normalize();
        assert_eq!(
            (preferences.opacity, preferences.zoom, preferences.chat_size),
            (0.2, 1.5, 14.)
        );
        preferences.opacity = 0.72;
        preferences.normalize();
        assert_eq!(preferences.opacity, 0.72);
    }
    #[test]
    fn scrollbar_track_and_handle_keep_their_width_on_hover_and_drag() {
        let preferences = Preferences {
            opacity: 0.4,
            ..Default::default()
        };
        let theme = Theme::from_preferences(&preferences);
        let ctx = egui::Context::default();
        theme.apply(&ctx, &preferences);
        let style = ctx.style().spacing.scroll;
        assert!(!style.floating);
        assert_eq!(style.bar_width, style.floating_width);
        assert_eq!(style.allocated_width(), 14.);
        assert_eq!(style.bar_inner_margin, style.bar_outer_margin);
        let draw = |time: f64, events: Vec<egui::Event>| {
            let mut inner = egui::Rect::NOTHING;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(300., 240.),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            inner = egui::ScrollArea::vertical()
                                .id_salt("fixed-scrollbar-fixture")
                                .max_width(220.)
                                .max_height(160.)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.allocate_exact_size(
                                        egui::vec2(180., 1000.),
                                        egui::Sense::hover(),
                                    );
                                })
                                .inner_rect;
                        });
                },
            );
            let painted: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if (rect.fill == theme.input || rect.fill == theme.ink)
                            && rect.rect.width() < 20. =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .collect();
            (inner, painted)
        };
        draw(0., vec![]);
        draw(0.5, vec![]);
        let idle = draw(1., vec![]);
        assert_eq!(idle.0.width(), 206.);
        assert_eq!(idle.1.len(), 2);
        let pointer = egui::pos2(idle.0.right() + 7., 10.);
        let hover = draw(2., vec![egui::Event::PointerMoved(pointer)]);
        let pressed = draw(
            3.,
            vec![egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let drag = draw(
            4.,
            vec![egui::Event::PointerMoved(pointer + egui::vec2(0., 50.))],
        );
        for frame in [idle, hover, pressed, drag] {
            assert_eq!(frame.0.width(), 206.);
            assert_eq!(frame.1.len(), 2);
            for rect in frame.1 {
                assert_eq!(rect.width(), 8.);
                assert!(rect.left() >= frame.0.right() + 3.);
                assert!(
                    (rect.center().x - frame.0.right() - style.allocated_width() / 2.).abs() < 0.01
                );
            }
        }
    }
    #[test]
    fn horizontal_and_floating_scrollbars_are_painted_in_the_center_of_their_gutter() {
        for horizontal in [false, true] {
            for floating in [false, true] {
                let ctx = egui::Context::default();
                let p = Preferences::default();
                let theme = Theme::from_preferences(&p);
                theme.apply(&ctx, &p);
                let draw = |time| {
                    let mut inner = egui::Rect::NOTHING;
                    let output = ctx.run(egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320., 240.))),
                        time: Some(time), ..Default::default()
                    }, |ctx| {
                        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                            ui.spacing_mut().scroll.floating = floating;
                            ui.spacing_mut().scroll.floating_allocated_width = if floating { 0. } else { 8. };
                            let scroll = if horizontal { egui::ScrollArea::horizontal() } else { egui::ScrollArea::vertical() };
                            let result = scroll.id_salt((horizontal, floating)).max_width(220.).max_height(160.)
                                .auto_shrink([false, false]).show(ui, |ui| {
                                    let size = if horizontal { egui::vec2(1000., 100.) } else { egui::vec2(180., 1000.) };
                                    ui.allocate_exact_size(size, egui::Sense::hover());
                                });
                            inner = result.inner_rect;
                        });
                    });
                    (inner, output)
                };
                draw(0.); draw(0.5);
                let (inner, output) = draw(1.);
                let painted: Vec<_> = output.shapes.iter().filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) if (rect.fill == theme.input || rect.fill == theme.ink)
                        && if horizontal { rect.rect.height() < 20. && rect.rect.width() > 20. }
                            else { rect.rect.width() < 20. && rect.rect.height() > 20. } => Some(rect.rect),
                    _ => None,
                }).collect();
                assert_eq!(painted.len(), 2, "track and thumb, horizontal={horizontal}, floating={floating}");
                let inner_end = if horizontal { inner.bottom() } else { inner.right() };
                let gutter_center = inner_end + if floating { -7. } else { 7. };
                for rect in painted {
                    let width = if horizontal { rect.height() } else { rect.width() };
                    let center = if horizontal { rect.center().y } else { rect.center().x };
                    assert!((width - 8.).abs() < 0.01);
                    assert!((center - gutter_center).abs() < 0.01, "painted bar must be centered in its 14pt gutter");
                }
            }
        }
    }
    #[test]
    fn scrollbar_is_visually_centered_in_padded_frame_and_still_receives_drag_input() {
        for padding in [0, 6, 10, 16] {
            let ctx = egui::Context::default();
            let preferences = Preferences::default();
            let theme = Theme::from_preferences(&preferences);
            theme.apply(&ctx, &preferences);
            let draw = |time, events| {
                let mut inner = egui::Rect::NOTHING;
                let mut frame_rect = egui::Rect::NOTHING;
                let mut offset = 0.;
                let output = ctx.run(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320., 260.))),
                    time: Some(time), events, ..Default::default()
                }, |ctx| {
                    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                        let frame = egui::Frame::new().fill(egui::Color32::from_rgb(17, 23, 31))
                            .inner_margin(egui::Margin::same(padding)).show(ui, |ui| {
                                let scroll = vertical_scroll(ui).id_salt(("padded-visual-scroll", padding))
                                    .max_height(180.).auto_shrink([false, false]).show(ui, |ui| {
                                        ui.allocate_exact_size(egui::vec2(180., 1000.), egui::Sense::hover());
                                    });
                                inner = scroll.inner_rect;
                                offset = scroll.state.offset.y;
                            });
                        frame_rect = frame.response.rect;
                    });
                });
                let bars: Vec<_> = output.shapes.iter().filter_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect) if (rect.fill == theme.input || rect.fill == theme.ink)
                        && (rect.rect.width() - 8.).abs() < 0.01 && rect.rect.height() > 20. => {
                        assert!(shape.clip_rect.contains_rect(rect.rect), "the moved bar must remain entirely inside its paint clip");
                        Some(rect.rect)
                    }
                    _ => None,
                }).collect();
                (inner, frame_rect, bars, offset)
            };
            draw(0., vec![]); draw(0.5, vec![]);
            let idle = draw(1., vec![]);
            assert_eq!(idle.2.len(), 2);
            let thumb = *idle.2.iter().min_by(|a, b| a.height().total_cmp(&b.height())).unwrap();
            let pointer = thumb.center();
            let hover = draw(2., vec![egui::Event::PointerMoved(pointer)]);
            let press = draw(3., vec![egui::Event::PointerButton { pos: pointer, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE }]);
            let dragged = draw(4., vec![egui::Event::PointerMoved(pointer + egui::vec2(0., 55.))]);
            assert!(dragged.3 > idle.3, "the portion outside the padded content must accept a drag");
            for (inner, frame, bars, _) in [idle, hover, press, dragged] {
                assert_eq!(bars.len(), 2);
                let visual_center = (inner.right() + frame.right()) * 0.5;
                for bar in bars {
                    assert!((bar.center().x - visual_center).abs() < 0.1,
                        "padding={padding}: bar must be centered between content and the actual frame edge");
                    assert!((bar.width() - 8.).abs() < 0.01);
                    assert!((bar.left() - inner.right() - (frame.right() - bar.right())).abs() < 0.1);
                }
            }
        }
    }
}
