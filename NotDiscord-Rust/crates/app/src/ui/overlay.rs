//! Common dismissal policy for app-owned foreground surfaces. egui retains
//! ownership of its native menu/ComboBox popups, including nested selectors.
use super::*;

#[derive(Clone, Copy)]
enum Surface { Dialog, Picker, Viewer, Search }

fn bounds(ctx: &egui::Context, id: &str) -> Option<egui::Rect> {
    let id = egui::Id::new(id);
    let rect = egui::containers::AreaState::load(ctx, id)?.rect();
    let rect = ctx.layer_transform_to_global(egui::LayerId::new(egui::Order::Foreground, id))
        .map_or(rect, |transform| transform * rect);
    (rect.is_finite() && rect.is_positive()).then_some(rect)
}

impl NotDiscordUi {
    /// Replacement/update shutdown saves the preference editors before the
    /// controller releases its connections and the desktop ownership lease.
    /// Ordinary outside-click cancellation continues to restore its backup.
    pub fn prepare_orderly_close(&mut self) -> Result<(), String> {
        let result = (|| {
            if self.settings_backup.is_some() || matches!(self.dialog, Some(Dialog::Settings)) {
                self.prefs.save(&self.data_dir)?;
                self.settings_backup = None;
            }
            if matches!(self.dialog, Some(Dialog::Sounds)) {
                self.sound_preferences.save(&self.data_dir)?;
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.error = format!("No se pudo guardar antes de cambiar de versión: {error}");
        } else {
            self.sound_importer.cancel();
            self.sounds.stop_preview();
            self.sounds.stop_ring();
        }
        result
    }

    /// Call before drawing the shell, once current snapshots have been applied.
    /// Consume the dismissal press, so it cannot also activate a control behind
    /// a surface. A child menu owns the first outside click/Escape itself.
    pub(super) fn dismiss_outside_overlays(&mut self, ctx: &egui::Context) {
        if egui::Popup::is_any_open(ctx) { return; }
        let frame = ctx.cumulative_frame_nr();
        let key = egui::Id::new("overlay-dismiss-frame");
        if ctx.data(|data| data.get_temp::<u64>(key)) == Some(frame) { return; }
        ctx.data_mut(|data| data.insert_temp(key, frame));
        let mut surfaces = Vec::with_capacity(4);
        if let Some(dialog) = &self.dialog {
            let id = match dialog {
                Dialog::Settings => "settings-dialog",
                Dialog::Sounds => "sound-settings",
                Dialog::Media => "media-settings",
                Dialog::Members => "channel-members-dialog",
                Dialog::DmProfile => "dm-profile-dialog",
                Dialog::GuildInvite(_) => "guild-invite-dialog",
                Dialog::Sources => "share-source-selector",
                Dialog::Cameras | Dialog::CamerasPreview => "camera-picker-dialog",
                Dialog::Prompt { action, .. } if action.kind == "edit" => "edit-message-dialog",
                Dialog::Prompt { .. } => "prompt-dialog",
                Dialog::Confirm { .. } => "confirmation-dialog",
                Dialog::Forward(_) => "forward-dialog",
                Dialog::Invite(_) => "accept-invite-dialog",
            };
            if let Some(rect) = bounds(ctx, id) { surfaces.push((Surface::Dialog, rect)); }
        }
        if self.search_open {
            if let Some(rect) = bounds(ctx, "search-completion") { surfaces.push((Surface::Search, rect)); }
        }
        if self.picker.is_some() {
            if let Some(rect) = bounds(ctx, "content-picker") { surfaces.push((Surface::Picker, rect)); }
        }
        if self.viewer.is_some() {
            if let Some(rect) = bounds(ctx, "media-viewer") { surfaces.push((Surface::Viewer, rect)); }
        }
        let Some((top, top_rect)) = surfaces.last().copied() else { return; };
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let outside = ctx.input(|input| input.pointer.any_pressed() && input.pointer.interact_pos()
            .is_some_and(|position| !top_rect.contains(position)
                && !(matches!(top, Surface::Search) && self.search_rect.contains(position))));
        if !escape && !outside { return; }
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("overlay-dismissal-block"), frame));
        ctx.input_mut(|input| {
            if escape { input.consume_key(egui::Modifiers::NONE, egui::Key::Escape); }
            if outside {
                input.events.retain(|event| !matches!(event, egui::Event::PointerButton { pressed: true, .. }));
            }
        });
        match top {
            Surface::Dialog => {
                if let Some(backup) = self.settings_backup.take() {
                    self.prefs = backup;
                    self.theme = Theme::from_preferences(&self.prefs);
                    self.theme.apply(ctx, &self.prefs);
                }
                // The existing lifecycle checks stop local mic/selection
                // previews after their owning dialog disappears.
                self.dialog = None;
                self.sounds.stop_preview();
                self.sound_importer.cancel();
                ctx.data_mut(|data| data.remove::<f32>(egui::Id::new("settings-opening-zoom")));
                ctx.set_transform_layer(egui::LayerId::new(egui::Order::Foreground,
                    egui::Id::new("settings-dialog")), egui::emath::TSTransform::IDENTITY);
            }
            Surface::Picker => { self.picker = None; self.emojis = Vec::new(); }
            Surface::Viewer => {
                if let Some(viewer) = self.viewer.take() {
                    self.pictures.stop();
                    self.pictures.release_viewer(&viewer.url, viewer.local_path.as_deref());
                    if let Some(path) = viewer.local_path { self.cleanup_unreferenced_temporary(&[path]); }
                }
            }
            Surface::Search => {
                self.search_open = false;
                self.search_focused = false;
                super::dialogs::suspend_search_keyboard(ctx);
            }
        }
        self.mention_closed = true;
        ctx.request_repaint();
    }
}
