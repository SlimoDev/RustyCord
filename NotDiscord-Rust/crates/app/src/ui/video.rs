use super::*;

impl NotDiscordUi {
    /// Upload only the newest frame, retaining one GPU texture per active sink.
    pub(super) fn poll_video_frames(&mut self, ctx: &egui::Context) {
        super::voice::prune_expanded_video(ctx,&self.state);
        let Some(shared) = self.video_frames.as_ref() else {
            return;
        };
        let (active, frames) = {
            let mut store = shared.lock().unwrap_or_else(|error| error.into_inner());
            (store.active_ids(), store.drain())
        };
        self.video_textures.retain(|id, _| active.contains(id));
        for frame in frames {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );
            if let Some(texture) = self.video_textures.get_mut(&frame.subscription_id) {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                let texture = ctx.load_texture(
                    format!("call-video:{}", frame.subscription_id),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                self.video_textures.insert(frame.subscription_id, texture);
            }
        }
    }
    pub(super) fn video_texture(
        &self,
        id: u64,
        _ctx: &egui::Context,
    ) -> Option<egui::TextureHandle> {
        self.video_textures.get(&id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use notdiscord_core::video::*;
    #[test]
    fn rgb_frames_reuse_texture_and_detach_frees_it() {
        let (mut app, _, _) = super::super::tests::harness();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(LatestFrames::default()));
        app.video_frames = Some(shared.clone());
        shared.lock().unwrap().activate(9);
        let frame = |sequence| VideoFrame {
            subscription_id: 9,
            sequence,
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
        };
        shared.lock().unwrap().publish(frame(1));
        let ctx = app.ctx.clone();
        app.poll_video_frames(&ctx);
        let first = app.video_textures[&9].id();
        shared.lock().unwrap().publish(frame(2));
        app.poll_video_frames(&ctx);
        assert_eq!(first, app.video_textures[&9].id());
        assert_eq!(app.video_textures.len(), 1);
        assert_eq!(shared.lock().unwrap().buffered_bytes(), 0);
        shared.lock().unwrap().remove(9);
        app.poll_video_frames(&ctx);
        assert!(app.video_textures.is_empty());
    }
}
