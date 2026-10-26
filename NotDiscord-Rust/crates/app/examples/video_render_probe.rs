//! Synthetic native renderer evidence, without a controller, account or devices.
#[cfg(debug_assertions)]
mod probe {
    use eframe::egui;
    use notdiscord_core::{models::*, video::*};
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex, mpsc},
    };
    struct App {
        ui: notdiscord::ui::NotDiscordUi,
        paths: Option<Vec<PathBuf>>,
        _commands: tokio::sync::mpsc::UnboundedReceiver<UiAction>,
    }
    impl eframe::App for App {
        fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
            [0.; 4]
        }
        fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
            eframe::App::update(&mut self.ui, ctx, frame);
            if let Some(paths) = self.paths.take() {
                self.ui.set_fixture_draft(paths);
            }
        }
    }
    pub fn run() -> eframe::Result {
        let output = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .expect("Output PNG required");
        let root = output.parent().unwrap().join("synthetic-video-data");
        std::fs::create_dir_all(&root).unwrap();
        let zoom = std::env::args()
            .nth(2)
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(1.);
        std::fs::write(root.join("ui-settings.json"),serde_json::to_vec(&serde_json::json!({"Background":"#320354","Separator":"#79747D","AccentColor":"#25133E","Opacity":1,"Zoom":zoom,"ChatSize":17})).unwrap()).unwrap();
        let wide = root.join("Imagen horizontal.png");
        image::RgbaImage::from_fn(600, 30, |x, _| {
            if x < 300 {
                image::Rgba([220u8, 60, 70, 255])
            } else {
                image::Rgba([40, 170, 230, 255])
            }
        })
        .save(&wide)
        .unwrap();
        let tall = root.join("Imagen vertical.png");
        image::RgbaImage::from_fn(40, 160, |_, y| {
            if y < 80 {
                image::Rgba([40u8, 200, 80, 255])
            } else {
                image::Rgba([220, 180, 40, 255])
            }
        })
        .save(&tall)
        .unwrap();
        let person = |id: &str, name: &str| Person {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        };
        let mut state = AppSnapshot {
            signed_in: true,
            demo: true,
            view: "chat".into(),
            selected_channel_id: "200".into(),
            selected_channel_kind: "group".into(),
            channel_name: "Prueba sintética".into(),
            current_user: Some(person("100", "Persona local")),
            channel_access: ChannelAccess::private(),
            voice_connected: true,
            voice_encryption_ready: true,
            call_channel_name: "Vídeo sintético · sin dispositivos".into(),
            call_status_label: "Cámara local y pantalla remota simuladas".into(),
            ..Default::default()
        };
        state.direct_messages.push(NavItem {
            id: "200".into(),
            name: state.channel_name.clone(),
            kind: "group".into(),
            ..Default::default()
        });
        state.call_participants = vec![
            CallParticipant {
                user: person("100", "Persona local"),
                is_self: true,
                ..Default::default()
            },
            CallParticipant {
                user: person("101", "Persona remota"),
                ..Default::default()
            },
        ];
        state.call_videos = vec![
            CallVideo {
                id: 1,
                user_id: "100".into(),
                label: "Vista previa local".into(),
                kind: "camera".into(),
                local: true,
                active: true,
                ..Default::default()
            },
            CallVideo {
                id: 2,
                user_id: "101".into(),
                label: "Pantalla remota".into(),
                kind: "screen".into(),
                active: true,
                ..Default::default()
            },
        ];
        let shared = Arc::new(Mutex::new(LatestFrames::default()));
        for id in [1, 2] {
            let mut store = shared.lock().unwrap();
            store.activate(id);
            let rgba = (0..(320 * 180))
                .flat_map(|index| {
                    let x = index % 320;
                    let y = index / 320;
                    match (id, x < 160, y < 90) {
                        (1, true, true) => [240, 40, 40, 255],
                        (1, false, true) => [40, 220, 60, 255],
                        (1, _, _) => [40, 80, 240, 255],
                        (2, true, _) => [230, 210, 30, 255],
                        _ => [160, 40, 200, 255],
                    }
                })
                .collect();
            store.publish(VideoFrame {
                subscription_id: id,
                sequence: 1,
                width: 320,
                height: 180,
                rgba,
            });
        }
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1360., 860.])
                .with_transparent(true)
                .with_decorations(false),
            ..Default::default()
        };
        eframe::run_native(
            "NotDiscord · vista sintética",
            options,
            Box::new(move |cc| {
                let (command, commands) = tokio::sync::mpsc::unbounded_channel();
                let (tx, rx) = mpsc::channel();
                tx.send(notdiscord::ui::UiUpdate::Snapshot(state)).unwrap();
                tx.send(notdiscord::ui::UiUpdate::VideoFrames(shared))
                    .unwrap();
                let mut ui = notdiscord::ui::NotDiscordUi::new(cc, root, command, rx);
                ui.set_preview(output);
                Ok(Box::new(App {
                    ui,
                    paths: Some(vec![wide, tall]),
                    _commands: commands,
                }))
            }),
        )
    }
}
#[cfg(debug_assertions)]
fn main() -> eframe::Result {
    probe::run()
}
#[cfg(not(debug_assertions))]
fn main() {
    eprintln!("Synthetic fixture utility is available in debug builds only.");
}
