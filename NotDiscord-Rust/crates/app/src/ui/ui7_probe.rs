//! Explicit diagnostic-only headless exercise of production UI components.
//! No controller, session reader, renderer, network request or capture engine.
use super::*;
use notdiscord_core::video::{LatestFrames, SharedVideoFrames, VideoFrame};
use serde_json::json;
use std::path::Path;

pub struct Ui7ResourceProbe {
    app: NotDiscordUi,
    ctx: egui::Context,
    state: AppSnapshot,
    base_messages: SharedList<ChatMessage>,
    mention_source: String,
    shared: SharedVideoFrames,
    commands: tokio::sync::mpsc::UnboundedReceiver<UiAction>,
    previous: Option<egui::FullOutput>,
    point: egui::Pos2,
    cycles: HashMap<String, u64>,
    command_counts: HashMap<String, u64>,
    frames: u64,
    video_sequence: u64,
    cleanup_checks: u64,
    emoji_static_seeded: bool,
}

impl Ui7ResourceProbe {
    pub fn new(fresh_data: &Path) -> Self {
        assert!(
            fresh_data.is_absolute() && fresh_data.starts_with(r"E:\DESCARGAS\REA\NotDiscordRust")
        );
        assert!(
            !fresh_data.exists(),
            "Diagnostic data must be a fresh isolated E: directory"
        );
        std::fs::create_dir_all(fresh_data).unwrap();
        resources::forbid_network_for_probe();
        let ctx = egui::Context::default();
        let (send, commands) = tokio::sync::mpsc::unbounded_channel();
        let (_, updates) = std::sync::mpsc::channel();
        let mut app = NotDiscordUi::from_context(&ctx, fresh_data.to_path_buf(), send, updates);
        app.prefs.show_group_members = false;
        app.prefs.show_server_members = false;
        app.prefs.send_typing = false;
        let peer = Person {
            id: "101".into(),
            name: "Offline peer".into(),
            username: "offline-peer".into(),
            ..Default::default()
        };
        let current = Person {
            id: "100".into(),
            name: "Offline self".into(),
            username: "offline-self".into(),
            ..Default::default()
        };
        let people = (0..128)
            .map(|index| Person {
                id: (1000 + index).to_string(),
                name: format!("Visible person {index}"),
                username: format!("person-{index}"),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let base_messages: SharedList<ChatMessage> = (0..20)
            .map(|index| ChatMessage {
                id: (3000 + index).to_string(),
                channel_id: "20".into(),
                author: peer.clone(),
                content: format!(
                    "Offline reference message {index}: the same chat and input controls."
                ),
                ..Default::default()
            })
            .collect::<Vec<_>>()
            .into();
        let mut access = ChannelAccess::private();
        access.can_mention_everyone = true;
        let state = AppSnapshot {
            demo: true,
            signed_in: true,
            view: "chat".into(),
            selected_guild_id: "10".into(),
            selected_channel_id: "20".into(),
            selected_channel_kind: "text".into(),
            current_user: Some(current),
            channel_name: "Offline resources".into(),
            channel_access: access,
            messages: base_messages.clone(),
            search_people: people.clone(),
            guild_members: people
                .iter()
                .cloned()
                .map(|user| ChannelMember {
                    user,
                    ..Default::default()
                })
                .collect::<Vec<_>>()
                .into(),
            direct_messages: (0..100)
                .map(|index| NavItem {
                    id: (5000 + index).to_string(),
                    name: format!("Destination {index}"),
                    kind: "dm".into(),
                    ..Default::default()
                })
                .collect(),
            call_channel_name: "Offline two participants".into(),
            call_participants: vec![
                CallParticipant {
                    user: peer,
                    self_video: true,
                    screen_sharing: true,
                    ..Default::default()
                },
                CallParticipant {
                    user: Person {
                        id: "102".into(),
                        name: "Second offline peer".into(),
                        ..Default::default()
                    },
                    self_video: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mention_source = (0..64)
            .map(|index| format!("Hello <@{}>, edited text {index}.\n", 1000 + index))
            .collect();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(LatestFrames::default()));
        app.video_frames = Some(shared.clone());
        app.state = state.clone();
        let mut probe = Self {
            app,
            ctx,
            state,
            base_messages,
            mention_source,
            shared,
            commands,
            previous: None,
            point: egui::Pos2::ZERO,
            cycles: HashMap::new(),
            command_counts: HashMap::new(),
            frames: 0,
            video_sequence: 0,
            cleanup_checks: 0,
            emoji_static_seeded: false,
        };
        // Identical ASCII font warmup in every process; this does not load the
        // Unicode emoji picker or contact Windows media/notification services.
        let ascii: String = (' '..='~').collect();
        let _ = probe.ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(&ascii);
            });
        });
        for _ in 0..4 {
            probe.frame(Vec::new());
        }
        probe
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        assert!(self.frames < 20_000, "Diagnostic frame limit exceeded");
        let ctx = self.ctx.clone();
        self.app.state = self.state.clone();
        self.app.pictures.begin_frame();
        self.app.poll_video_frames(&ctx);
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100., 800.),
                )),
                time: Some(self.frames as f64 / 60.),
                events,
                ..Default::default()
            },
            |ctx| {
                self.app.draw_modal_barrier(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_enabled_ui(!self.app.background_blocked(), |ui| {
                        self.app.draw_chat(ui, &self.state)
                    });
                });
                self.app.draw_dialog(ctx, &self.state);
            },
        );
        self.app.pictures.finish_frame(false);
        assert_eq!(
            self.app.pictures.pending_fixture_requests(),
            0,
            "No diagnostic step may schedule an image request"
        );
        assert_eq!(
            self.app.pictures.custom_animation_metrics()["worker"],
            false,
            "No network animation worker may be started"
        );
        assert_eq!(
            resources::probe_network_attempts(),
            0,
            "A fixture attempted to request network data; the irreversible diagnostic fence blocked it"
        );
        while let Ok(action) = self.commands.try_recv() {
            assert_eq!(
                action.kind, "video-quality",
                "Unexpected controller command in offline UI probe: {}",
                action.kind
            );
            *self.command_counts.entry(action.kind).or_default() += 1;
        }
        self.previous = Some(output);
        self.frames += 1;
    }

    fn point_for_text(&self, needle: &str) -> Option<egui::Pos2> {
        self.previous
            .as_ref()?
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text.contains(needle) => {
                    let rect = egui::Rect::from_min_size(text.pos, text.galley.size())
                        .intersect(shape.clip_rect);
                    rect.is_positive().then_some(rect.center())
                }
                _ => None,
            })
    }
    fn point_for_video(&self, id: u64) -> Option<egui::Pos2> {
        let texture = self.app.video_textures.get(&id)?.id();
        self.previous
            .as_ref()?
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect
                        .brush
                        .as_ref()
                        .is_some_and(|brush| brush.fill_texture_id == texture) =>
                {
                    Some(rect.rect.intersect(shape.clip_rect).center())
                }
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                    Some(mesh.calc_bounds().intersect(shape.clip_rect).center())
                }
                _ => None,
            })
    }
    fn click(&self, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(self.point),
            egui::Event::PointerButton {
                pos: self.point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    fn cycle(&mut self, kind: &str) {
        *self.cycles.entry(kind.into()).or_default() += 1;
    }

    fn video_setup(&mut self) {
        self.state.voice_connected = true;
        self.state.voice_encryption_ready = true;
        self.state.call_channel_id = "20".into();
        self.state.call_instance_id = 7;
        self.state.call_status_label = "Offline synthetic frames".into();
        self.state.call_videos = [8, 9]
            .into_iter()
            .enumerate()
            .map(|(index, id)| CallVideo {
                id,
                user_id: (101 + index).to_string(),
                kind: if index == 0 { "screen" } else { "camera" }.into(),
                active: true,
                status: "playing".into(),
                label: format!("Synthetic video {id}"),
                ..Default::default()
            })
            .collect();
        let mut shared = self.shared.lock().unwrap();
        for id in [8, 9] {
            assert!(shared.activate(id));
        }
    }
    fn video_frames(&mut self) {
        self.video_sequence += 1;
        let mut shared = self.shared.lock().unwrap();
        for id in shared.active_ids() {
            let mut rgba = vec![0u8; 256 * 144 * 4];
            for pixel in rgba.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[
                    (self.video_sequence % 255) as u8,
                    128,
                    id as u8 * 20,
                    255,
                ]);
            }
            assert!(shared.publish(VideoFrame {
                subscription_id: id,
                sequence: self.video_sequence,
                width: 256,
                height: 144,
                rgba
            }));
        }
    }
    fn emoji_setup(&mut self, animate: bool) {
        let mut list = self.base_messages.to_vec();
        list.last_mut().unwrap().content = "Animated fixture ".into();
        for index in 0..12 {
            let emoji = StatusEmoji {
                id: (9000 + index).to_string(),
                name: format!("Offline{index}"),
                animated: true,
            };
            let url = emoji.image_url().unwrap();
            let poster = url.replace("&animated=true", "");
            if !self.emoji_static_seeded {
                self.app.pictures.seed_sized_fixture(&self.ctx, &poster, 64);
            }
            if animate {
                self.app
                    .pictures
                    .seed_emoji_animation_sized_fixture(&self.ctx, &url, 8, 32);
            }
            list.last_mut()
                .unwrap()
                .content
                .push_str(&format!("<a:{}:{}>", emoji.name, emoji.id));
        }
        self.state.messages = list.into();
        self.emoji_static_seeded = true;
    }

    /// One deterministic frame. Active modes repeat open/use/close at the same
    /// 60 Hz schedule; external code owns pacing and process measurement.
    pub fn step(&mut self, mode: &str, iteration: usize) {
        let phase = iteration % 12;
        let mut events = Vec::new();
        match mode {
            "baseline" | "cleanup" => {}
            "mentions" => {
                if phase == 0 {
                    self.app.prompt_for(
                        "Editar mensaje",
                        "Offline mention document",
                        "edit",
                        "3019",
                        &self.mention_source,
                        "20",
                        true,
                    );
                    self.cycle("mentions");
                }
                if phase == 7 {
                    self.app.dialog = None;
                    self.app.prompt = String::new();
                }
            }
            "dialogs" => {
                if phase == 0 {
                    self.app.prompt_for(
                        "Editar mensaje",
                        "Offline long editor",
                        "edit",
                        "3019",
                        &"Long offline document line\n".repeat(100),
                        "20",
                        true,
                    );
                    self.cycle("dialogs");
                }
                if phase == 4 {
                    self.app.dialog =
                        Some(Dialog::Forward(self.base_messages.last().unwrap().clone()));
                    self.app.forward_targets = self.state.direct_messages.clone();
                }
                if phase == 9 {
                    self.app.dialog = None;
                    self.app.prompt = String::new();
                    self.app.forward_targets = Vec::new();
                }
            }
            "video" => {
                if phase == 0 {
                    self.video_setup();
                    self.cycle("video");
                }
                if phase < 9 {
                    self.video_frames();
                }
                if phase == 1 {
                    self.point = self
                        .point_for_video(8)
                        .expect("Synthetic video thumbnail visible");
                    events = self.click(true);
                }
                if phase == 2 {
                    events = self.click(false);
                }
                if phase == 3 {
                    assert!(self.expanded());
                    self.point = self
                        .point_for_video(8)
                        .expect("Expanded synthetic video visible");
                    events = vec![
                        egui::Event::PointerMoved(self.point),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0., 700.),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ];
                }
                if phase == 4 {
                    events = self.click(true);
                }
                if phase == 5 {
                    events = vec![egui::Event::PointerMoved(self.point + egui::vec2(30., 18.))];
                }
                if phase == 6 {
                    self.point += egui::vec2(30., 18.);
                    events = self.click(false);
                }
                if phase == 7 {
                    events = vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }];
                }
                if phase == 8 {
                    self.shared.lock().unwrap().remove(8);
                    self.state.call_videos.retain(|v| v.id != 8);
                    events = vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }];
                }
                if phase == 9 {
                    self.shared.lock().unwrap().clear();
                    self.state.call_videos.clear();
                    self.state.voice_connected = false;
                }
            }
            "emoji" => {
                if phase == 0 {
                    self.emoji_setup(false);
                    self.cycle("emoji");
                }
                if (1..=7).contains(&phase) {
                    if phase == 1 {
                        self.emoji_setup(true);
                    }
                    self.point = self
                        .point_for_text("Animated fixture")
                        .expect("Synthetic emoji message visible");
                    events = vec![egui::Event::PointerMoved(self.point)];
                }
                if phase == 8 {
                    events = vec![egui::Event::PointerMoved(egui::pos2(1098., 1.))];
                }
                if phase == 9 {
                    self.state.messages = self.base_messages.clone();
                }
            }
            _ => panic!("Unsupported diagnostic mode: {mode}"),
        }
        self.frame(events);
        if mode == "video" && phase >= 9 {
            assert!(self.app.video_textures.is_empty() && !self.expanded());
            self.cleanup_checks += 1;
        }
        if mode == "emoji" && phase >= 8 {
            assert_eq!(self.app.pictures.custom_animation_metrics()["bytes"], 0);
            self.cleanup_checks += 1;
        }
        if (mode == "mentions" && phase >= 7) || (mode == "dialogs" && phase >= 9) {
            assert!(
                egui::TextEdit::load_state(&self.ctx, egui::Id::new("edit-mention-text")).is_none()
            );
            self.cleanup_checks += 1;
        }
    }
    fn expanded(&self) -> bool {
        self.ctx
            .data(|data| {
                data.get_temp::<super::voice::CallExpansion>(egui::Id::new("call-expanded-video"))
            })
            .is_some()
    }
    pub fn cleanup(&mut self) {
        self.app.dialog = None;
        self.app.prompt = String::new();
        self.app.forward_targets = Vec::new();
        self.state.messages = self.base_messages.clone();
        self.state.call_videos.clear();
        self.state.voice_connected = false;
        self.shared.lock().unwrap().clear();
        self.app.pictures.clear_account();
        self.emoji_static_seeded = false;
        for _ in 0..3 {
            self.frame(vec![egui::Event::PointerMoved(egui::pos2(1098., 1.))]);
        }
        assert!(self.app.video_textures.is_empty() && !self.expanded());
        assert_eq!(self.app.pictures.custom_animation_metrics()["bytes"], 0);
        assert!(
            egui::TextEdit::load_state(&self.ctx, egui::Id::new("edit-mention-text")).is_none()
        );
        self.cleanup_checks += 1;
    }
    pub fn metrics(&self) -> Value {
        let manager = self.ctx.tex_manager();
        let manager = manager.read();
        json!({"frames":self.frames,"cycles":self.cycles,"cleanupChecks":self.cleanup_checks,
            "controllerCommandsRecordedNotExecuted":self.command_counts,"videoTextureHandles":self.app.video_textures.len(),
            "videoBufferedBytes":self.shared.lock().unwrap().buffered_bytes(),"expandedVideo":self.expanded(),
            "dialogOpen":self.app.dialog.is_some(),"editStateRetained":egui::TextEdit::load_state(&self.ctx,egui::Id::new("edit-mention-text")).is_some(),
            "customAnimation":self.app.pictures.custom_animation_metrics(),"textureHandles":manager.num_allocated(),
            "nominalTextureBytes":manager.allocated().map(|(_,meta)|meta.bytes_used()).sum::<usize>(),
            "pendingImageRequests":self.app.pictures.pending_fixture_requests(),"accountUsed":false,"controllerCreated":false,
            "networkFenceIrreversible":true,"networkAttemptsBlocked":resources::probe_network_attempts(),
            "networkRequestsExecuted":false,"captureEngineCreated":false,"audioDeviceOpened":false,"gpuRendererCreated":false})
    }
}
