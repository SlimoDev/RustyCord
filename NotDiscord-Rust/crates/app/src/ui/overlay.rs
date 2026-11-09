//! Common dismissal policy for app-owned foreground surfaces. egui retains
//! ownership of its native menu/ComboBox popups, including nested selectors.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Surface { Search, Dialog, ColorEditor, Picker, Viewer, Timestamp, TextViewer, Diagnostics }

/// The small foreground contract shared by dismissal, keyboard ownership and
/// the backdrop. Form rollback remains with its existing owner.
#[derive(Clone, Copy)]
pub(super) struct Descriptor {
    pub kind: Surface,
    pub id: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(app:&mut NotDiscordUi,state:&AppSnapshot,events:Vec<egui::Event>)->egui::FullOutput {
        let ctx=app.ctx.clone();
        ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1100.,800.))),events,..Default::default()},|ctx| {
            app.dismiss_outside_overlays(ctx);
            super::super::input_ownership::begin(app,ctx,state);
            app.draw_modal_barrier(ctx);
            app.draw_overlay_surfaces(ctx,state);
        })
    }
    fn timestamp(app:&mut NotDiscordUi)->AppSnapshot {
        let mut state=super::super::tests::signed_in("synthetic-surface-account");
        state.channel_access.can_send=true;
        app.drafts.insert("channel".into(),Draft {text:"@time".into(),revision:7,..Default::default()});
        app.open_timestamp_picker(&state,0,5,7);
        assert!(app.time_picker.is_some());state
    }
    #[test]
    fn timestamp_outside_press_retires_only_top_without_editing_draft_or_clicking_through() {
        let (mut app,_,mut commands)=super::super::tests::harness();
        let state=timestamp(&mut app);
        app.dialog=Some(Dialog::Confirm {title:"Parent fixture".into(),description:"Synthetic parent".into(),action:UiAction::new("never-confirm")});
        for _ in 0..3 {frame(&mut app,&state,vec![]);}
        let top=app.active_surfaces(&app.ctx).last().copied().unwrap();
        assert_eq!(top.kind,Surface::Timestamp);
        assert_eq!(top.keyboard_layer().unwrap().id,egui::Id::new("timestamp-picker"));
        let ctx=app.ctx.clone();
        let _=ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1100.,800.))),events:vec![egui::Event::PointerMoved(egui::pos2(2.,2.)),egui::Event::PointerButton {pos:egui::pos2(2.,2.),button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}],..Default::default()},|ctx| {
            app.dismiss_outside_overlays(ctx);
            assert!(app.time_picker.is_none());assert!(app.dialog.is_some());
            assert!(!ctx.input(|input|input.events.iter().any(|e|matches!(e,egui::Event::PointerButton {pressed:true,..}))));
            assert!(app.background_blocked());
        });
        assert_eq!(app.drafts["channel"].text,"@time");assert_eq!(app.drafts["channel"].revision,7);
        while let Ok(action)=commands.try_recv() {assert_ne!(action.kind,"never-confirm");}
    }
    #[test]
    fn native_child_popup_keeps_the_first_escape_before_timestamp_dismissal() {
        let (mut app,_,_)=super::super::tests::harness();let state=timestamp(&mut app);
        for _ in 0..3 {frame(&mut app,&state,vec![]);}
        let ctx=app.ctx.clone();
        let _=ctx.run(egui::RawInput {events:vec![egui::Event::Key {key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:egui::Modifiers::NONE}],..Default::default()},|ctx| {
            egui::Popup::open_id(ctx,egui::Id::new("synthetic-child-popup"));
            app.dismiss_outside_overlays(ctx);assert!(app.time_picker.is_some());
            assert!(ctx.input(|input|input.key_pressed(egui::Key::Escape)));
        });
    }
    #[test]
    fn foreground_keyboard_policy_covers_readers_forms_and_nested_editors() {
        // Readers block typing; forms/editors own a foreground field. The
        // enclosing dialog delegates its field choice to its own form.
        let cases=[(Surface::Search,"search-completion",false,true),
            (Surface::Dialog,"confirmation-dialog",false,false),
            (Surface::ColorEditor,"color-editor",false,true),
            (Surface::Picker,"content-picker",false,true),
            (Surface::Viewer,"media-viewer",true,true),
            (Surface::Timestamp,"timestamp-picker",false,true),
            (Surface::TextViewer,"text-viewer",true,true),
            (Surface::Diagnostics,"diagnostic-panel",true,true)];
        for (kind,id,blocked,scoped) in cases {
            let descriptor=Descriptor {kind,id};
            assert_eq!(descriptor.keyboard_blocked(),blocked,"{kind:?}");
            assert_eq!(descriptor.keyboard_layer().is_some(),scoped,"{kind:?}");
        }
    }
    #[test]
    fn static_backdrop_does_not_schedule_sixteen_millisecond_repaints() {
        let (mut app,_,_)=super::super::tests::harness();
        app.dialog=Some(Dialog::Confirm {title:"Static".into(),description:String::new(),action:UiAction::new("unused")});
        let ctx=app.ctx.clone();ctx.style_mut(|style|style.animation_time=0.);
        let mut output=None;
        for index in 0..5 {
            output=Some(ctx.run(egui::RawInput {time:Some(index as f64),screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1100.,800.))),..Default::default()},|ctx|app.draw_modal_barrier(ctx)));
        }
        let delay=output.unwrap().viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        assert!(delay>std::time::Duration::from_millis(16),"A static input barrier has no periodic work: {delay:?}");
    }
}
impl Descriptor {
    pub(super) fn keyboard_blocked(self) -> bool {
        matches!(self.kind, Surface::Viewer | Surface::TextViewer | Surface::Diagnostics)
    }
    pub(super) fn keyboard_layer(self) -> Option<egui::LayerId> {
        (!matches!(self.kind, Surface::Dialog)).then(||egui::LayerId::new(egui::Order::Foreground,egui::Id::new(self.id)))
    }
}

fn bounds(ctx: &egui::Context, id: &str) -> Option<egui::Rect> {
    let id = egui::Id::new(id);
    let rect = egui::containers::AreaState::load(ctx, id)?.rect();
    let rect = ctx.layer_transform_to_global(egui::LayerId::new(egui::Order::Foreground, id))
        .map_or(rect, |transform| transform * rect);
    (rect.is_finite() && rect.is_positive()).then_some(rect)
}

impl NotDiscordUi {
    /// Bottom to top, matching draw_dialog's search/form/color child order,
    /// followed by draw_overlay_surfaces below. Keep native popups owned by egui.
    pub(super) fn active_surfaces(&self, ctx: &egui::Context) -> Vec<Descriptor> {
        let mut result=Vec::with_capacity(8);
        let mut push=|kind,id|result.push(Descriptor {kind,id});
        if self.search_open {push(Surface::Search,"search-completion");}
        if let Some(dialog)=&self.dialog {
            let id=match dialog {
                Dialog::Settings=>"settings-dialog", Dialog::Sounds=>"sound-settings", Dialog::Media=>"media-settings",
                Dialog::Members=>"channel-members-dialog", Dialog::DmProfile=>"dm-profile-dialog",
                Dialog::GuildInvite(_)=>"guild-invite-dialog", Dialog::Sources=>"share-source-selector",
                Dialog::Cameras|Dialog::CamerasPreview=>"camera-picker-dialog",
                Dialog::Prompt {action,..} if action.kind=="edit"=>"edit-message-dialog",
                Dialog::Prompt {..}=>"prompt-dialog", Dialog::Confirm {..}=>"confirmation-dialog",
                Dialog::Forward(_)=>"forward-dialog", Dialog::Invite(_)=>"accept-invite-dialog",
            };
            push(Surface::Dialog,id);
        }
        if self.color_editor.is_some() {push(Surface::ColorEditor,"color-editor");}
        if self.picker.is_some() {push(Surface::Picker,"content-picker");}
        if self.viewer.is_some() {push(Surface::Viewer,"media-viewer");}
        if self.time_picker.is_some() {push(Surface::Timestamp,"timestamp-picker");}
        if self.text_viewer.is_some() {push(Surface::TextViewer,"text-viewer");}
        if crate::diagnostics::is_open(ctx) {push(Surface::Diagnostics,"diagnostic-panel");}
        result
    }
    pub(super) fn draw_overlay_surfaces(&mut self,ctx:&egui::Context,state:&AppSnapshot) {
        self.draw_dialog(ctx,state); // search, dialog, and its color child
        self.draw_picker(ctx,state);
        self.draw_viewer(ctx);
        self.draw_timestamp_picker(ctx,state);
        self.draw_text_viewer(ctx,state);
        crate::diagnostics::draw(ctx);
    }
    /// Replacement/update shutdown saves the preference editors before the
    /// controller releases its connections and the desktop ownership lease.
    /// Ordinary outside-click cancellation continues to restore its backup.
    pub fn prepare_orderly_close(&mut self) -> Result<(), String> {
        // An unaccepted preview never reaches disk through this path.
        self.cancel_color_editor();
        let result = (|| {
            if self.settings_backup.is_some() || matches!(self.dialog, Some(Dialog::Settings)) {
                if [&self.prefs.background, &self.prefs.separator, &self.prefs.accent_color]
                    .into_iter().any(|color| theme::parse_color(color).is_none()) {
                    return Err("Los colores de ajustes no son válidos.".to_owned());
                }
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
        let Some(top) = self.active_surfaces(ctx).last().copied() else { return; };
        // A newly opened child without painted bounds still owns this frame;
        // never dismiss its parent merely because its rectangle is unavailable.
        let Some(top_rect) = bounds(ctx, top.id) else { return; };
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let outside = ctx.input(|input| input.pointer.any_pressed() && input.pointer.interact_pos()
            .is_some_and(|position| !top_rect.contains(position)
                && !(matches!(top.kind, Surface::Search) && self.search_rect.contains(position))));
        if !escape && !outside { return; }
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("overlay-dismissal-block"), frame));
        ctx.input_mut(|input| {
            if escape { input.consume_key(egui::Modifiers::NONE, egui::Key::Escape); }
            if outside {
                input.events.retain(|event| !matches!(event, egui::Event::PointerButton { pressed: true, .. }));
            }
        });
        match top.kind {
            Surface::ColorEditor => self.cancel_color_editor(),
            Surface::TextViewer => self.close_text_viewer(),
            Surface::Timestamp => self.time_picker = None,
            Surface::Diagnostics => crate::diagnostics::close(ctx),
            Surface::Dialog => {
                self.color_editor = None;
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
