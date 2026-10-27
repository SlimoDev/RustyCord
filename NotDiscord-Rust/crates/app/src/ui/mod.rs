mod chat;
mod autoscroll;
mod overlay;
mod appearance;
mod custom_text;
mod mentions;
mod markdown_cache;
mod text_budget;
mod txt_import;
mod sound_settings;
mod media_limit;
mod dialogs;
mod guild_settings;
mod font_files;
mod icons;
mod incoming;
mod member_demand;
mod moderation;
mod group_members;
mod notification_activation;
mod resources;
mod profile_editor;
mod shell;
mod theme;
mod video;
mod voice;
#[cfg(feature = "offline-probes")]
pub mod ui7_probe;

fn cjk_scripts(text: &str) -> u8 {
    text.chars().fold(0, |scripts, ch| {
        scripts
            | match ch as u32 {
                0x1100..=0x11ff | 0x3130..=0x318f | 0xac00..=0xd7af => 2,
                0x2e80..=0x312f
                | 0x3190..=0x9fff
                | 0xf900..=0xfaff
                | 0xff00..=0xffef
                | 0x20000..=0x323af => 1,
                _ => 0,
            }
    })
}
fn snapshot_scripts(snapshot: &AppSnapshot) -> u8 {
    let mut scripts = cjk_scripts(&snapshot.channel_name) | cjk_scripts(&snapshot.status)
        | cjk_scripts(&snapshot.custom_activity_label);
    for item in snapshot.guilds.iter().chain(&snapshot.channels).chain(&snapshot.direct_messages) {
        scripts |= cjk_scripts(&item.name) | cjk_scripts(&item.custom_status) | cjk_scripts(&item.activity_text);
    }
    for person in snapshot.search_people.iter().chain(snapshot.current_user.iter()).chain(snapshot.profile_user.iter()) {
        scripts |= cjk_scripts(&person.name) | cjk_scripts(&person.username);
    }
    for member in snapshot.guild_members.iter().chain(&snapshot.group_members) {
        scripts |= cjk_scripts(&member.user.name) | cjk_scripts(&member.custom_status);
        for activity in &member.activities { scripts |= cjk_scripts(&activity.name) | cjk_scripts(&activity.details); }
    }
    for message in snapshot.messages.iter().chain(&snapshot.pinned_messages) {
        scripts |= cjk_scripts(&message.content) | cjk_scripts(&message.author.name);
        for embed in &message.embeds {
            for text in [&embed.title, &embed.description, &embed.author, &embed.provider, &embed.footer] {
                scripts |= cjk_scripts(text);
            }
            for field in &embed.fields { scripts |= cjk_scripts(&field.name) | cjk_scripts(&field.value); }
        }
        for component in &message.components { scripts |= cjk_scripts(&component.text); }
        for previous in &message.previous_contents { scripts |= cjk_scripts(previous); }
    }
    for call in &snapshot.incoming_calls {
        scripts |= cjk_scripts(&call.channel_name);
        for person in call.caller.iter().chain(&call.participants).chain(call.call_participants.iter().map(|participant| &participant.user)) {
            scripts |= cjk_scripts(&person.name) | cjk_scripts(&person.username);
        }
    }
    scripts
}
fn presence_label(value: &str) -> &str {
    match value {
        "online" => "En línea",
        "idle" => "Ausente",
        "dnd" => "No molestar",
        "offline" | "invisible" => "Desconectado",
        _ => "Estado no disponible",
    }
}

use eframe::egui::{self, Color32, RichText};
use notdiscord_core::models::*;
use resources::Pictures;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};
use theme::{Preferences, Theme};

struct BorderedTextEdit<'a>(egui::TextEdit<'a>);
fn bordered(edit: egui::TextEdit<'_>) -> BorderedTextEdit<'_> {
    BorderedTextEdit(edit)
}
impl egui::Widget for BorderedTextEdit<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.scope(|ui| {
            let line = ui.visuals().window_stroke.color;
            let accent = ui.visuals().hyperlink_color;
            // TextEdit paints its hint with weak_text_color. A global text
            // override would bake the normal ink into that hint's galley.
            ui.visuals_mut().override_text_color = None;
            let widgets = &mut ui.visuals_mut().widgets;
            widgets.inactive.bg_stroke = egui::Stroke::new(1_f32, line);
            widgets.hovered.bg_stroke = egui::Stroke::new(1.5_f32, accent);
            widgets.active.bg_stroke = egui::Stroke::new(1.5_f32, accent);
            widgets.open.bg_stroke = egui::Stroke::new(1.5_f32, accent);
            ui.add(self.0)
        })
        .inner
    }
}

fn localise<Tz: chrono::TimeZone>(timestamp: &Timestamp, timezone: &Tz) -> chrono::DateTime<Tz> {
    timestamp.with_timezone(timezone)
}
fn local_date(timestamp: &Timestamp) -> chrono::NaiveDate {
    localise(timestamp, &chrono::Local).date_naive()
}
fn local_time(timestamp: &Timestamp) -> String {
    localise(timestamp, &chrono::Local)
        .format("%H:%M")
        .to_string()
}
fn spanish_date(date: chrono::NaiveDate, separator: bool) -> String {
    use chrono::Datelike;
    const MONTHS: [&str; 12] = [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ];
    let month = MONTHS[date.month0() as usize];
    if separator {
        format!("{} de {} de {}", date.day(), month, date.year())
    } else {
        format!("{} {} {}", date.day(), month, date.year())
    }
}
fn profile_date(timestamp: &Timestamp) -> String {
    spanish_date(local_date(timestamp), false)
}

pub enum UiUpdate {
    Snapshot(AppSnapshot),
    LatestSnapshot(std::sync::Arc<std::sync::Mutex<Option<AppSnapshot>>>),
    VideoFrames(notdiscord_core::video::SharedVideoFrames),
    Data(String, Value),
    Error(String),
}
#[derive(Clone, Default)]
struct Mention {
    start: usize,
    end: usize,
    id: String,
    label: String,
}
#[derive(Clone, Default)]
struct Draft {
    text: String,
    reply: Option<MessageReplyPreview>,
    paths: Vec<PathBuf>,
    revision: u64,
    mentions: Vec<Mention>,
    captured_at: Option<Instant>,
}
#[derive(Default)]
struct ChatPosition {
    offset: f32,
    at_bottom: bool,
    distance_to_bottom: Option<f32>,
    anchor: String,
    anchor_top: f32,
}
#[derive(Clone)]
enum Dialog {
    Settings,
    Sounds,
    Media,
    Members,
    DmProfile,
    GuildInvite(String),
    Sources,
    Cameras,
    CamerasPreview,
    Prompt {
        title: String,
        description: String,
        action: UiAction,
        multiline: bool,
    },
    Confirm {
        title: String,
        description: String,
        action: UiAction,
    },
    Forward(ChatMessage),
    Invite(String),
}
#[derive(Default)]
struct Picker {
    gif: bool,
    query: String,
    category: String,
    guild: String,
    items: Vec<Value>,
    categories: Vec<Value>,
    note: String,
    limit: u32,
    pending: bool,
    react: Option<(String, String)>,
}
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct PickerValue {
    value: String,
    name: String,
    image_url: String,
}
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct PickerHistory {
    emoji_recent: Vec<PickerValue>,
    gif_favorites: Vec<PickerValue>,
    gif_recent: Vec<PickerValue>,
}
struct Viewer {
    url: String,
    // Local sources enter only through an already prepared attachment, never
    // by interpreting a message URL as a filesystem path.
    local_path: Option<PathBuf>,
    zoom: f32,
    pan: egui::Vec2,
}
#[derive(Clone)]
struct NotificationPolicy {
    account: String,
    generation: u64,
    epoch: u64,
    presence: String,
}
pub struct NotDiscordUi {
    state: AppSnapshot,
    command: tokio::sync::mpsc::UnboundedSender<UiAction>,
    updates: Receiver<UiUpdate>,
    updater: crate::updates::UpdateManager,
    latest_snapshot: Option<std::sync::Arc<std::sync::Mutex<Option<AppSnapshot>>>>,
    data_dir: PathBuf,
    ctx: egui::Context,
    notifications: crate::platform::Notifications,
    sounds: crate::sounds::SoundEngine,
    sound_preferences: crate::sounds::SoundPreferences,
    sound_importer: crate::sounds::import::SoundImporter,
    preview: Option<(PathBuf, u8, Instant)>,
    preview_delay: Duration,
    prefs: Preferences,
    settings_backup: Option<Preferences>,
    theme: Theme,
    backdrop: crate::backdrop::Backdrop,
    incoming_popup: incoming::IncomingPopup,
    latest_scroll_version: u64,
    pictures: Pictures,
    video_frames: Option<notdiscord_core::video::SharedVideoFrames>,
    video_textures: HashMap<u64, egui::TextureHandle>,
    video_visibility: Option<bool>,
    member_demand: Option<(String, String, String)>,
    logo_texture: Option<egui::TextureHandle>,
    drafts: HashMap<String, Draft>,
    draft_order: VecDeque<String>,
    positions: HashMap<String, ChatPosition>,
    last_channel: String,
    last_view: String,
    search: String,
    search_focused: bool,
    search_open: bool,
    search_cursor: usize,
    search_rect: egui::Rect,
    search_suggestions: Vec<notdiscord_core::search::SearchSuggestion>,
    search_signature: String,
    fonts: egui::FontDefinitions,
    cjk_loaded: u8,
    pending_scripts: u8,
    snapshot_fonts_pending: bool,
    dm_sidebar_profile: Option<ProfileDetails>,
    dm_sidebar_user_id: String,
    dm_sidebar_loading: bool,
    dialog: Option<Dialog>,
    prompt: String,
    dialog_data: Value,
    dialog_error: String,
    picker: Option<Picker>,
    picker_history: PickerHistory,
    picker_account: String,
    txt_import: Option<txt_import::Importer>,
    profile_tab: usize,
    profile_editor: Option<profile_editor::Editor>,
    forward_scope: String,
    forward_targets: Vec<NavItem>,
    forward_selected: Vec<String>,
    forward_query: String,
    forward_pending: bool,
    forward_result: Value,
    viewer: Option<Viewer>,
    error: String,
    submissions: HashMap<String, (String, u64)>,
    submitted_paths: HashMap<String, Vec<PathBuf>>,
    // Captured sends own their text/files independently of the next draft.
    sent_drafts: HashMap<String, Draft>,
    failed_submissions: HashSet<String>,
    owned_temporary: HashSet<PathBuf>,
    focus_id: String,
    focus_until: Option<Instant>,
    focus_version: u64,
    focus_scroll_pending: bool,
    pending_scroll: Option<f32>,
    restore_anchor: Option<(String, f32)>,
    scroll_bottom: bool,
    mention_index: usize,
    mention_closed: bool,
    composer_cursor: usize,
    row_heights: HashMap<String, f32>,
    nav_heights: HashMap<String, (u64, f32)>,
    // Search metadata borrows the bundled TSV; opening the picker does not
    // duplicate thousands of labels into separate heap allocations.
    emojis: Vec<(&'static str, &'static str, &'static str)>,
    emoji_textures: HashMap<(String, u32), egui::TextureHandle>,
    emoji_weight: usize,
    emoji_request: std::sync::mpsc::SyncSender<resources::EmojiRequest>,
    emoji_ready: Receiver<resources::EmojiResult>,
    emoji_pending: HashMap<(String, u32), u64>,
    emoji_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    emoji_failed: HashMap<(String, u32), Instant>,
    media_settings: Value,
    devices: Value,
    media_section: String,
    window_title: String,
    last_typing: Instant,
    share_height: i32,
    share_fps: i32,
    share_audio: bool,
    share_loopback: bool,
}
impl NotDiscordUi {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        data_dir: PathBuf,
        command: tokio::sync::mpsc::UnboundedSender<UiAction>,
        updates: Receiver<UiUpdate>,
    ) -> Self {
        Self::from_context(&cc.egui_ctx, data_dir, command, updates)
    }
    fn from_context(
        ctx: &egui::Context,
        data_dir: PathBuf,
        command: tokio::sync::mpsc::UnboundedSender<UiAction>,
        updates: Receiver<UiUpdate>,
    ) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        for path in [
            r"C:\Windows\Fonts\segoeui.ttf",
            r"C:\Windows\Fonts\seguisym.ttf",
            r"C:\Windows\Fonts\seguiemj.ttf",
        ] {
            if let Some(data) = font_files::load(path) {
                let name = path.to_string();
                fonts.font_data.insert(name.clone(), data.into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push(name);
            }
        }
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            if family.len() > 1 {
                let index = family
                    .iter()
                    .position(|x| x.ends_with("segoeui.ttf"))
                    .unwrap_or(0);
                let font = family.remove(index);
                family.insert(0, font);
            }
        }
        theme::configure_author_font(&mut fonts);
        ctx.set_fonts(fonts.clone());
        let prefs = Preferences::load(&data_dir);
        let sound_preferences = crate::sounds::SoundPreferences::load(&data_dir);
        let theme = Theme::from_preferences(&prefs);
        theme.apply(ctx, &prefs);
        // Unicode search metadata is needed only while its picker is open.
        let emojis = Vec::new();
        let pictures = Pictures::new(ctx.clone());
        let emoji_generation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let (emoji_request, emoji_ready) = resources::emoji_worker(ctx.clone(), pictures.visual_suspension(), emoji_generation.clone());
        let logo_texture = image::load_from_memory_with_format(
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/discord.ico"
            )),
            image::ImageFormat::Ico,
        )
        .ok()
        .map(|im| {
            let im = im.thumbnail(96, 96).into_rgba8();
            ctx.load_texture(
                "discord-logo",
                egui::ColorImage::from_rgba_unmultiplied(
                    [im.width() as usize, im.height() as usize],
                    &im,
                ),
                egui::TextureOptions::LINEAR,
            )
        });
        let media_settings = serde_json::to_value(
            crate::storage::read_json::<crate::storage::MediaPreferences>(
                &data_dir.join("media-settings.json"),
                32768,
            )
            .normalize(),
        )
        .unwrap_or_default();
        Self {
            state: AppSnapshot::welcome(),
            command,
            updates,
            updater: Default::default(),
            latest_snapshot: None,
            data_dir,
            ctx: ctx.clone(),
            notifications: crate::platform::Notifications::new(),
            sounds: crate::sounds::SoundEngine::default(),
            sound_preferences,
            sound_importer: Default::default(),
            preview: None,
            preview_delay: Duration::from_millis(500),
            prefs,
            settings_backup: None,
            theme,
            backdrop: crate::backdrop::Backdrop::default(),
            incoming_popup: incoming::IncomingPopup::default(),
            latest_scroll_version: 0,
            pictures,
            video_frames: None,
            video_textures: HashMap::new(),
            video_visibility: None,
            member_demand: None,
            logo_texture,
            drafts: HashMap::new(),
            draft_order: VecDeque::new(),
            positions: HashMap::new(),
            last_channel: String::new(),
            last_view: String::new(),
            search: String::new(),
            search_focused: false,
            search_open: false,
            search_cursor: 0,
            search_rect: egui::Rect::NOTHING,
            search_suggestions: Vec::new(),
            search_signature: String::new(),
            fonts,
            cjk_loaded: 0,
            pending_scripts: 0,
            snapshot_fonts_pending: false,
            dm_sidebar_profile: None,
            dm_sidebar_user_id: String::new(),
            dm_sidebar_loading: false,
            dialog: None,
            prompt: String::new(),
            dialog_data: Value::Null,
            dialog_error: String::new(),
            picker: None,
            picker_history: PickerHistory::default(),
            picker_account: String::new(),
            txt_import: None,
            profile_tab: 0,
            profile_editor: None,
            forward_scope: String::new(),
            forward_targets: Vec::new(),
            forward_selected: Vec::new(),
            forward_query: String::new(),
            forward_pending: false,
            forward_result: Value::Null,
            viewer: None,
            error: String::new(),
            submissions: HashMap::new(),
            submitted_paths: HashMap::new(),
            sent_drafts: HashMap::new(),
            failed_submissions: HashSet::new(),
            owned_temporary: HashSet::new(),
            focus_id: String::new(),
            focus_until: None,
            focus_version: 0,
            focus_scroll_pending: false,
            pending_scroll: None,
            restore_anchor: None,
            scroll_bottom: true,
            mention_index: 0,
            mention_closed: false,
            composer_cursor: 0,
            row_heights: HashMap::new(),
            nav_heights: HashMap::new(),
            emojis,
            emoji_textures: HashMap::new(),
            emoji_weight: 0,
            emoji_request,
            emoji_ready,
            emoji_pending: HashMap::new(),
            emoji_generation,
            emoji_failed: HashMap::new(),
            media_settings,
            devices: Value::Null,
            media_section: String::new(),
            window_title: String::new(),
            last_typing: Instant::now() - Duration::from_secs(10),
            share_height: 720,
            share_fps: 30,
            share_audio: false,
            share_loopback: false,
        }
    }
    pub fn set_preview(&mut self, path: PathBuf) {
        self.preview = Some((path, 0, Instant::now()));
        self.ctx.request_repaint();
    }
    pub fn set_preview_delay(&mut self, milliseconds: u64) {
        if self.preview.is_some() {
            self.preview_delay = Duration::from_millis(milliseconds.clamp(500, 10000));
        }
    }
    #[cfg(any(debug_assertions, feature = "offline-probes"))]
    pub fn set_fixture_viewer(&mut self, open: bool) {
        if self.state.demo {
            if open {
                let url = "https://cdn.discordapp.com/attachments/1/2/synthetic.png";
                self.pictures.seed_fixture(&self.ctx, url);
                self.viewer = Some(Viewer {
                    url: url.into(),
                    local_path: None,
                    zoom: 1.,
                    pan: egui::Vec2::ZERO,
                });
            } else {
                if let Some(viewer) = self.viewer.take() {
                    self.pictures.release_viewer(&viewer.url, viewer.local_path.as_deref());
                }
                self.pictures.stop();
            }
            self.ctx.request_repaint();
        }
    }
    #[cfg(any(debug_assertions, feature = "offline-probes"))]
    pub fn set_fixture_viewer_file(&mut self, path: PathBuf) {
        if self.state.demo {
            let url = "https://cdn.discordapp.com/attachments/1/2/synthetic.png";
            self.pictures.seed_fixture_file(url, path);
            self.viewer = Some(Viewer {
                url: url.into(),
                local_path: None,
                zoom: 1.,
                pan: egui::Vec2::ZERO,
            });
            self.ctx.request_repaint();
        }
    }
    #[cfg(debug_assertions)]
    pub fn set_fixture_draft(&mut self, paths: Vec<PathBuf>) {
        if self.preview.is_some() && self.state.demo && !self.state.selected_channel_id.is_empty() {
            self.drafts.insert(
                self.state.selected_channel_id.clone(),
                Draft {
                    paths: paths.into_iter().take(10).collect(),
                    ..Default::default()
                },
            );
            self.ctx.request_repaint();
        }
    }
    pub fn readiness(&self) -> Value {
        serde_json::json!({"signedIn":self.state.signed_in,"gatewayReady":self.state.connection=="En línea","voiceAvailable":self.state.voice_available,"viewIsChat":self.state.view=="chat","permissionsKnown":self.state.channel_access.known,"visibleMessages":self.state.messages.len(),"nativeBlurActive":self.backdrop.active(),"nativeBlurError":self.backdrop.error,"nativeBlurWindow":self.backdrop.native_window()})
    }
    fn ensure_script_fonts(&mut self, requested: u8) {
        self.pending_scripts |= requested;
        if self.pictures.visuals_suspended() { return; }
        let needed = std::mem::take(&mut self.pending_scripts) & !self.cjk_loaded;
        if needed == 0 {
            return;
        }
        for (bit, paths) in [
            (
                1,
                &[
                    r"C:\Windows\Fonts\YuGothM.ttc",
                    r"C:\Windows\Fonts\YuGothR.ttc",
                    r"C:\Windows\Fonts\msgothic.ttc",
                    r"C:\Windows\Fonts\msyh.ttc",
                    r"C:\Windows\Fonts\msjh.ttc",
                ][..],
            ),
            (2, &[r"C:\Windows\Fonts\malgun.ttf"][..]),
        ] {
            if needed & bit == 0 {
                continue;
            }
            if let Some((path, mut data)) = paths
                .iter()
                .find_map(|path| font_files::load(path).map(|data| (*path, data)))
            {
                data.index = 0;
                self.fonts.font_data.insert(path.into(), data.into());
                self.fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push(path.into());
            }
            self.cjk_loaded |= bit;
        }
        theme::configure_author_font(&mut self.fonts);
        self.ctx.set_fonts(self.fonts.clone());
        self.ctx.request_repaint();
    }
    fn send(
        &mut self,
        kind: &str,
        id: impl Into<String>,
        text: impl Into<String>,
        extra: impl Into<String>,
        channel: &str,
    ) {
        if self
            .command
            .send(UiAction {
                kind: kind.into(),
                id: id.into(),
                text: text.into(),
                extra: extra.into(),
                channel_id: channel.into(),
            })
            .is_err()
        {
            self.error = "El servicio de la aplicación se cerró.".into();
        }
    }
    fn simple(&mut self, kind: &str) {
        self.send(kind, "", "", "", "");
    }
    fn prompt_for(
        &mut self,
        title: &str,
        description: &str,
        kind: &str,
        id: &str,
        text: &str,
        channel: &str,
        multiline: bool,
    ) {
        self.prompt = text.into();
        self.dialog_error.clear();
        self.dialog = Some(Dialog::Prompt {
            title: title.into(),
            description: description.into(),
            action: UiAction {
                kind: kind.into(),
                id: id.into(),
                channel_id: channel.into(),
                ..Default::default()
            },
            multiline,
        });
    }
    fn confirm(&mut self, title: &str, description: &str, action: UiAction) {
        self.dialog = Some(Dialog::Confirm {
            title: title.into(),
            description: description.into(),
            action,
        });
    }
    fn draft(&mut self, channel: &str) -> Option<&mut Draft> {
        if channel.is_empty() {
            return None;
        }
        if !self.drafts.contains_key(channel) {
            if self.drafts.len() >= 1024 {
                self.error =
                    "Hay 1024 borradores abiertos. Borra uno antes de preparar otro.".into();
                return None;
            }
            self.draft_order.push_back(channel.into());
            self.drafts.insert(channel.into(), Draft::default());
        }
        self.drafts.get_mut(channel)
    }
    fn cleanup_temporary(&mut self, paths: &[PathBuf]) {
        let parent = self.data_dir.join("draft-attachments");
        for path in paths {
            if self.owned_temporary.remove(path)
                && path.parent() == Some(parent.as_path())
                && path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
                && path.extension().is_some_and(|s| s == "png")
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    fn cleanup_unreferenced_temporary(&mut self, paths: &[PathBuf]) {
        let unused = paths.iter().filter(|path| !self.drafts.values().chain(self.sent_drafts.values())
            .any(|draft| draft.paths.contains(path))
            && !self.viewer.as_ref().is_some_and(|viewer| viewer.local_path.as_ref() == Some(*path)))
            .cloned().collect::<Vec<_>>();
        self.cleanup_temporary(&unused);
    }
    fn process_updates(&mut self, ctx: &egui::Context) {
        let mut latest = self
            .latest_snapshot
            .as_ref()
            .and_then(|slot| slot.lock().ok()?.take());
        for _ in 0..128 {
            let update = if let Some(snapshot) = latest.take() {
                UiUpdate::Snapshot(snapshot)
            } else {
                let Ok(update) = self.updates.try_recv() else {
                    break;
                };
                update
            };
            match update {
                UiUpdate::LatestSnapshot(slot) => {
                    self.latest_snapshot = Some(slot);
                    ctx.request_repaint();
                }
                UiUpdate::VideoFrames(frames) => {
                    self.video_textures.clear();
                    self.video_frames = Some(frames);
                    self.video_visibility = None;
                }
                UiUpdate::Snapshot(mut snapshot) => {
                    if snapshot.notification_generation != self.state.notification_generation
                        || snapshot.signed_in != self.state.signed_in
                        || snapshot.current_user.as_ref().map(|person| &person.id)
                            != self.state.current_user.as_ref().map(|person| &person.id) {
                        chat::clear_reaction_users(ctx);
                        chat::clear_history_page_request(ctx);
                        markdown_cache::clear(ctx);
                        if let Some(importer) = &mut self.txt_import { importer.cancel(); }
                        self.clear_edit_prompt(ctx);
                    }
                    // This ordered policy lane can be newer than a coalesced
                    // snapshot that was already flushed before its barrier.
                    // Keep just the policy fields; all other snapshot state is
                    // still applied normally.
                    let policy_id = egui::Id::new("notification-policy-barrier");
                    if let Some(policy) = ctx.data(|data| data.get_temp::<NotificationPolicy>(policy_id)) {
                        if snapshot.notification_generation == policy.generation
                            && snapshot.current_user.as_ref().is_some_and(|user| user.id == policy.account) {
                            if snapshot.notification_epoch <= policy.epoch {
                                snapshot.self_presence = policy.presence;
                                snapshot.notification_epoch = policy.epoch;
                            }
                        } else {
                            ctx.data_mut(|data| data.remove::<NotificationPolicy>(policy_id));
                        }
                    }
                    self.observe_sound_snapshot(&snapshot);
                    if self.follows_nearby_new_messages(ctx, &snapshot) {
                        self.prepare_latest_scroll_for(&snapshot.selected_channel_id);
                    } else if snapshot.view == "chat"
                        && snapshot.selected_channel_id == self.last_channel
                        && !chat::same_message_storage(&self.state, &snapshot)
                        && snapshot.messages != self.state.messages
                        && snapshot.focus_message_version == self.focus_version
                        && self.focus_id.is_empty()
                    {
                        if let Some(position) = self
                            .positions
                            .get(&snapshot.selected_channel_id)
                            .filter(|position| !position.at_bottom && !position.anchor.is_empty())
                        {
                            self.restore_anchor =
                                Some((position.anchor.clone(), position.anchor_top));
                        }
                    }
                    let mut avatars = HashMap::<String, Option<String>>::new();
                    for person in snapshot
                        .current_user
                        .iter()
                        .chain(snapshot.profile_user.iter())
                        .chain(snapshot.search_people.iter())
                        .chain(snapshot.guild_members.iter().map(|m| &m.user))
                        .chain(snapshot.group_members.iter().map(|m| &m.user))
                        .chain(snapshot.relationships.iter().map(|r| &r.user))
                    {
                        if !person.id.is_empty() {
                            avatars
                                .entry(person.id.clone())
                                .or_insert_with(|| person.avatar_url.clone());
                        }
                        if avatars.len() >= 512 {
                            break;
                        }
                    }
                    ctx.data_mut(|data| {
                        data.insert_temp(egui::Id::new("canonical-avatar-urls"), avatars)
                    });
                    if snapshot.search_people != self.state.search_people
                        || snapshot.channels != self.state.channels
                    {
                        self.search_signature.clear();
                    }
                    if self.cjk_loaded != 3 {
                        if self.pictures.visuals_suspended() {
                            self.snapshot_fonts_pending = true;
                        } else {
                            self.ensure_script_fonts(snapshot_scripts(&snapshot));
                            self.snapshot_fonts_pending = false;
                        }
                    }
                    let channel = snapshot.selected_channel_id.clone();
                    if channel != self.last_channel || snapshot.view != self.last_view {
                        chat::clear_history_page_request(ctx);
                        chat::clear_reaction_users(ctx);
                        self.pictures.stop();
                        self.dm_sidebar_profile = None;
                        self.dm_sidebar_user_id.clear();
                        self.dm_sidebar_loading = false;
                        self.search_open = false;
                        self.mention_closed = false;
                        if snapshot.view == "pins" {
                            self.pending_scroll = Some(0.);
                            self.scroll_bottom = false;
                        } else if snapshot.view == "chat" {
                            if let Some(pos) = self.positions.get(&channel) {
                                self.pending_scroll = Some(pos.offset);
                                self.scroll_bottom = pos.at_bottom;
                                self.restore_anchor = if pos.at_bottom || pos.anchor.is_empty() {
                                    None
                                } else {
                                    Some((pos.anchor.clone(), pos.anchor_top))
                                };
                            } else {
                                self.pending_scroll = None;
                                self.scroll_bottom = true;
                                self.restore_anchor = None;
                            }
                        }
                        self.last_channel = channel;
                        self.last_view = snapshot.view.clone();
                    }
                    if snapshot.focus_message_version != self.focus_version
                        && !snapshot.focus_message_id.is_empty()
                    {
                        self.focus_version = snapshot.focus_message_version;
                        self.focus_id = snapshot.focus_message_id.clone();
                        self.focus_scroll_pending = true;
                        self.focus_until = Some(Instant::now() + Duration::from_secs(2));
                        self.scroll_bottom = false;
                    }
                    let account = snapshot
                        .current_user
                        .as_ref()
                        .map(|p| p.id.as_str())
                        .unwrap_or("");
                    if account != self.picker_account {
                        self.emoji_generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                        self.emoji_pending.clear();
                        self.profile_editor = None;
                        self.sound_importer.cancel();
                        self.viewer = None;
                        let owned = self.owned_temporary.iter().cloned().collect::<Vec<_>>();
                        self.cleanup_temporary(&owned);
                        self.picker_account = account.into();
                        self.picker_history = self.read_picker_history();
                        self.picker = None;
                        self.dialog = None;
                        self.forward_pending = false;
                        self.forward_result = Value::Null;
                        self.forward_targets.clear();
                        self.forward_selected.clear();
                        self.dm_sidebar_profile = None;
                        self.dm_sidebar_user_id.clear();
                        self.dm_sidebar_loading = false;
                        self.video_textures.clear();
                        self.emoji_failed.clear();
                        self.drafts.clear();
                        self.draft_order.clear();
                        self.positions.clear();
                        self.row_heights.clear();
                        self.nav_heights.clear();
                        self.submissions.clear();
                        self.submitted_paths.clear();
                        self.sent_drafts.clear();
                        self.failed_submissions.clear();
                        self.viewer = None;
                        self.emojis = Vec::new();
                        self.pictures.clear_account();
                        if snapshot.signed_in
                            && self.preview.is_none()
                            && notdiscord_core::parsing::valid_id(account)
                        {
                            self.pictures
                                .configure_avatar_cache(&self.data_dir, account);
                        }
                    }
                    let latest_scroll = snapshot.latest_messages_version
                        != self.latest_scroll_version
                        && snapshot.latest_messages_version != 0
                        && snapshot.latest_messages_channel_id == snapshot.selected_channel_id
                        && snapshot.view == "chat"
                        && self.state.view == "chat"
                        && snapshot.selected_channel_id == self.state.selected_channel_id
                        && snapshot.notification_generation == self.state.notification_generation
                        && snapshot.current_user.as_ref().map(|user| &user.id)
                            == self.state.current_user.as_ref().map(|user| &user.id);
                    self.latest_scroll_version = snapshot.latest_messages_version;
                    self.state = snapshot;
                    if latest_scroll && self.state.view == "chat" {
                        self.prepare_latest_scroll();
                    }
                }
                UiUpdate::Error(e) => self.error = e,
                UiUpdate::Data(kind, data) => self.data_update(&kind, data),
            }
        }
        self.pictures.poll(ctx);
        if self.pictures.visuals_suspended() {
            self.video_textures.clear();
            if let Some(frames) = &self.video_frames {
                frames.lock().unwrap_or_else(|error| error.into_inner()).drain();
            }
            self.emoji_pending.clear();
            while self.emoji_ready.try_recv().is_ok() {}
            return;
        }
        self.poll_video_frames(ctx);
        while let Ok((generation, key, result)) = self.emoji_ready.try_recv() {
            if generation != self.emoji_generation.load(std::sync::atomic::Ordering::Acquire)
                || self.emoji_pending.get(&key) != Some(&generation) { continue; }
            self.emoji_pending.remove(&key);
            if let Some((w, h, rgba)) = result {
                if self.emoji_textures.contains_key(&key) { continue; }
                if self.emoji_weight + rgba.len() > 2 * 1024 * 1024 {
                    self.emoji_textures.clear();
                    self.emoji_weight = 0;
                }
                let texture = ctx.load_texture(
                    format!("emoji:{:?}", key),
                    egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                    egui::TextureOptions::LINEAR,
                );
                self.emoji_weight += rgba.len();
                self.emoji_textures.insert(key, texture);
            } else {
                if self.emoji_failed.len() >= 256 {
                    if let Some(old) = self
                        .emoji_failed
                        .iter()
                        .min_by_key(|(_, at)| **at)
                        .map(|(key, _)| key.clone())
                    {
                        self.emoji_failed.remove(&old);
                    }
                }
                self.emoji_failed.insert(key, Instant::now());
            }
        }
    }
    fn data_update(&mut self, kind: &str, data: Value) {
        if kind == "edit-result" { self.edit_result(&data); return; }
        if kind == "history-page-result" {chat::finish_history_page_request(&self.ctx,&data);return;}
        if kind == "reaction-users" {
            if self.state.signed_in && self.cjk_loaded != 3 && data["event"] != "invalidate"
                && data["invalidated"] != true && s(&data,"error").is_empty()
            {
                let scripts=data["users"].as_array().into_iter().flatten().take(100)
                    .fold(0,|scripts,person|scripts|cjk_scripts(s(person,"Name"))
                        |cjk_scripts(s(person,"DisplayName"))|cjk_scripts(s(person,"Username")));
                self.ensure_script_fonts(scripts);
            }
            chat::reaction_users_result(&self.ctx,&data);
            return;
        }
        if kind == "notification-activate" {
            self.activate_message_notification(data);
            return;
        }
        if kind == "notification-policy" {
            let account = s(&data, "account_id");
            let presence = s(&data, "presence");
            if self.state.signed_in && !account.is_empty()
                && self.state.current_user.as_ref().is_some_and(|user| user.id == account)
                && notdiscord_core::settings_proto::valid_presence(&presence)
                && data["generation"].as_u64() == Some(self.state.notification_generation)
                && let Some(epoch) = data["epoch"].as_u64().filter(|epoch| *epoch >= self.state.notification_epoch) {
                self.state.self_presence = presence.to_owned();
                self.state.notification_epoch = epoch;
                self.ctx.data_mut(|data| data.insert_temp(egui::Id::new("notification-policy-barrier"), NotificationPolicy {
                    account: account.into(), generation: self.state.notification_generation,
                    epoch, presence: presence.into(),
                }));
            }
            return;
        }
        if kind == "stream-viewers" { self.ctx.data_mut(|d|d.insert_temp(egui::Id::new("stream-viewers"),data)); return; }
        match kind {
            "deleted-log-purged" => {
                let channel = s(&data, "channelId");
                let message = s(&data, "messageId");
                if notdiscord_core::parsing::valid_id(channel) && notdiscord_core::parsing::valid_id(message) {
                    self.pictures.purge_deleted_message_preview(channel, message);
                    self.forget_deleted_message_context(channel, message);
                    for draft in self.drafts.values_mut().chain(self.sent_drafts.values_mut()) {
                        if let Some(reply) = draft.reply.as_mut().filter(|reply| reply.channel_id == channel && reply.message_id == message) {
                            reply.author = None;
                            reply.content.clear();
                            reply.mentions.clear();
                            reply.deleted = true;
                            reply.unavailable = true;
                        }
                    }
                }
            }
            "profile-editor-loaded" | "profile-edit-result" => self.profile_editor_update(kind,&data),
            "present" if data.as_bool() == Some(true) => self.prepare_latest_scroll(),
            "dm-sidebar-profile" => {
                let channel = s(&data, "channel_id");
                let profile = field(&data, "profile")
                    .and_then(|value| serde_json::from_value::<ProfileDetails>(value.clone()).ok());
                let user_id = profile
                    .as_ref()
                    .map(|profile| profile.user.id.as_str())
                    .unwrap_or_else(|| s(&data, "user_id"));
                if channel == self.state.selected_channel_id && user_id == self.dm_sidebar_user_id {
                    self.dm_sidebar_loading = false;
                    self.dm_sidebar_profile = profile;
                }
            }
            "cameras-preview" => {
                self.dialog_data = field(&data, "devices").cloned().unwrap_or(data);
                self.dialog = Some(Dialog::CamerasPreview);
                self.dialog_error.clear();
            }
            "send-ack" => {
                let submission = s(&data, "submissionId");
                let detached = self.sent_drafts.remove(submission).is_some();
                self.failed_submissions.remove(submission);
                if let Some((channel, revision)) = self.submissions.remove(submission) {
                    if self.state.view == "chat" && self.state.selected_channel_id == channel {
                        self.prepare_latest_scroll_for(&channel);
                    }
                    let paths = self.submitted_paths.remove(submission).unwrap_or_default();
                    if !detached && self
                        .drafts
                        .get(&channel)
                        .is_some_and(|d| d.revision == revision)
                    {
                        self.drafts.remove(&channel);
                    } else if !detached && let Some(draft) = self.drafts.get_mut(&channel) {
                        draft.paths.retain(|path| !paths.contains(path));
                    }
                    self.cleanup_unreferenced_temporary(&paths);
                }
            }
            "send-error" => {
                let submission = s(&data, "submissionId");
                if let Some((channel, revision)) = self.submissions.get(submission) {
                    if channel == s(&data, "channelId")
                        && field(&data, "revision").and_then(Value::as_u64) == Some(*revision)
                    {
                        self.failed_submissions.insert(submission.to_owned());
                        self.error =
                            "No se confirmó el envío. Puedes reintentar o recuperar el mensaje pendiente; el nuevo borrador se conserva."
                                .into();
                    }
                }
            }
            "gifs" | "gif-catalog" => {
                if let Some(p) = &mut self.picker {
                    p.items = field(&data, "Items")
                        .and_then(Value::as_array)
                        .cloned()
                        .or_else(|| data.as_array().cloned())
                        .unwrap_or_default();
                    p.categories = field(&data, "Categories")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    p.note = s(&data, "Note").into();
                    p.pending = false;
                    for item in &p.items {
                        resources::remember_source(s(item, "Url"), s(item, "AnimationUrl"));
                    }
                }
            }
            "emojis" | "guild-emojis" => {
                if let Some(p) = &mut self.picker {
                    p.items = data
                        .as_array()
                        .cloned()
                        .or_else(|| field(&data, "Items").and_then(Value::as_array).cloned())
                        .unwrap_or_default();
                    p.pending = false;
                }
            }
            "forward-channels" => {
                if s(&data, "guild_id") == self.forward_scope {
                    self.forward_targets = field(&data, "channels")
                        .and_then(|value| serde_json::from_value(value.clone()).ok())
                        .unwrap_or_default();
                }
            }
            "forward-result" => {
                if s(&data, "channel_id") == s(&self.forward_result, "channel_id")
                    && s(&data, "message_id") == s(&self.forward_result, "message_id")
                {
                    self.forward_pending = false;
                    self.forward_result = data;
                }
            }
            "guild-invite-channels" => {
                let guild = s(&data, "guildId").to_string();
                self.dialog_data = data;
                self.prompt.clear();
                self.dialog = Some(Dialog::GuildInvite(guild));
            }
            "devices-loading" => self.ctx.data_mut(|state| state.insert_temp(egui::Id::new("devices-loading"), data.as_bool().unwrap_or(false))),
            "voice-permissions" => self.ctx.data_mut(|state| state.insert_temp(egui::Id::new("voice-permissions"), data)),
            "devices" => {
                self.devices = data;
                self.ctx.data_mut(|state| state.insert_temp(egui::Id::new("devices-loading"), false));
            }
            "sources" => {
                self.dialog_data = data;
                self.prompt.clear();
                self.share_height = 720;
                self.share_fps = 30;
                self.share_audio = false;
                self.share_loopback = false;
                self.dialog = Some(Dialog::Sources);
            }
            "cameras" => {
                self.dialog_data = data;
                self.prompt.clear();
                self.dialog = Some(Dialog::Cameras);
            }
            "media-settings" => self.media_settings = data,
            "mic-test" => self
                .ctx
                .data_mut(|state| state.insert_temp(egui::Id::new("mic-test-state"), data)),
            "invite-preview" => {
                let code = s(&data, "code").to_owned();
                self.dialog_data = data;
                self.dialog = Some(Dialog::Invite(code));
            }
            "open-official-invite" => {
                let result = crate::platform::official_invite_url(s(&data, "url"))
                    .and_then(|url| crate::platform::open_link(&url));
                if let Err(error) = result {
                    self.error = error;
                }
            }
            "open-official-security" => {
                if let Err(error) = crate::platform::open_link("https://discord.com/app") {
                    self.error = error;
                }
            }
            "notice" => {
                self.automatic_notice(data);
            }
            "notice-read" => {
                if let Err(error) = self
                    .notifications
                    .clear_read(s(&data, "ChannelId"), s(&data, "MessageId"))
                {
                    self.error = error;
                }
            }
            "sound" => {
                self.automatic_sound(&data);
            }
            _ => {
                self.dialog_data = data;
            }
        }
    }
    fn navigate(&mut self, item: &NavItem) {
        let anchor=self.positions.get(&item.id).map(|p|serde_json::json!({"anchorMessageId":p.anchor,"anchorOffset":p.anchor_top,"atBottom":p.at_bottom}).to_string()).unwrap_or_default();
        self.send("channel", &item.id, "", anchor, "");
    }
    fn people_menu(&mut self, ui: &mut egui::Ui, p: &Person, state: &AppSnapshot) {
        if ui.button("Ver perfil").clicked() {
            self.send("profile", &p.id, "", "", &state.selected_channel_id);
            ui.close();
        }
        self.draw_add_to_call(ui, p, state);
        self.draw_member_moderation_menu(ui, p, state);
        self.draw_group_member_removal(ui, p, state);
        if p.id
            != state
                .current_user
                .as_ref()
                .map(|u| u.id.as_str())
                .unwrap_or("")
        {
            if ui.button("Enviar mensaje").clicked() {
                self.send("open-dm", &p.id, "", "", "");
                ui.close();
            }
            let relationship = state
                .relationships
                .iter()
                .find(|r| r.user.id == p.id)
                .map(|r| r.kind)
                .unwrap_or(0);
            match relationship {
                0 if !p.is_bot => {
                    if ui.button("Añadir amigo").clicked() {
                        self.send("add-friend-id", &p.id, "", "", "");
                        ui.close();
                    }
                }
                3 => {
                    if ui.button("Aceptar solicitud").clicked() {
                        self.send("accept-friend", &p.id, "", "", "");
                        ui.close();
                    }
                }
                4 => {
                    ui.add_enabled(false, egui::Button::new("Solicitud enviada"));
                }
                2 => {
                    ui.add_enabled(false, egui::Button::new("Usuario bloqueado"));
                }
                _ => {}
            }
        }
        if ui.button("Copiar ID").clicked() {
            ui.ctx().copy_text(p.id.clone());
            ui.close();
        }
    }
    fn avatar(
        &mut self,
        ui: &mut egui::Ui,
        p: &Person,
        presence: &str,
        size: f32,
    ) -> egui::Response {
        let response = self.pictures.avatar(ui, p, size);
        if !presence.is_empty() {
            let color = presence_color(presence);
            let pos = response.rect.right_bottom() - egui::vec2(size * 0.12, size * 0.12);
            ui.painter()
                .circle_filled(pos, size * 0.14, self.theme.panel);
            ui.painter().circle_filled(pos, size * 0.105, color);
        }
        response
    }
    fn nav_avatar(&mut self, ui: &mut egui::Ui, item: &NavItem, size: f32) -> egui::Response {
        if item.kind == "dm" && !item.recipient_id.is_empty() {
            return self.avatar(
                ui,
                &Person {
                    id: item.recipient_id.clone(),
                    name: item.name.clone(),
                    avatar_url: item.icon_url.clone(),
                    ..Default::default()
                },
                "",
                size,
            );
        }
        if item
            .icon_url
            .as_deref()
            .is_some_and(notdiscord_core::parsing::trusted_media)
        {
            return self.pictures.image(
                ui,
                item.icon_url.as_deref().unwrap(),
                egui::vec2(size, size),
                true,
            );
        }
        let mut photos: Vec<String> = Vec::new();
        for url in item
            .recipient_avatars
            .iter()
            .filter(|url| notdiscord_core::parsing::trusted_media(url))
        {
            if !photos.contains(url) {
                photos.push(url.clone());
            }
            if photos.len() == 4 {
                break;
            }
        }
        if photos.len() == 1 {
            return self
                .pictures
                .image(ui, &photos[0], egui::vec2(size, size), true);
        }
        let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
        if !ui.is_rect_visible(rect) {
            return response;
        }
        ui.painter()
            .circle_filled(rect.center(), size / 2., self.theme.accent_surface);
        if photos.is_empty() {
            use unicode_segmentation::UnicodeSegmentation;
            let initial = item
                .name
                .graphemes(true)
                .next()
                .unwrap_or("N")
                .to_uppercase();
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                initial,
                egui::FontId::proportional((size * 0.46).max(13.)),
                Color32::WHITE,
            );
            return response;
        }
        let columns = 2;
        let rows = if photos.len() <= 2 { 1 } else { 2 };
        let cell = egui::vec2(size / 2., size / rows as f32);
        for (index, url) in photos.iter().enumerate() {
            let Some(texture) = self
                .pictures
                .texture_sized(url, (size * ui.ctx().pixels_per_point()).ceil() as u32)
            else {
                continue;
            };
            let col = index % columns;
            let row = index / columns;
            let cell_min = rect.min + egui::vec2(col as f32 * cell.x, row as f32 * cell.y);
            let cell_ratio = cell.x / cell.y;
            let image_ratio = texture.size_vec2().x / texture.size_vec2().y;
            let extent = if image_ratio > cell_ratio {
                egui::vec2(cell_ratio / image_ratio, 1.)
            } else {
                egui::vec2(1., image_ratio / cell_ratio)
            };
            let uv_min = (egui::vec2(1., 1.) - extent) / 2.;
            let (start, end) = if rows == 1 {
                if col == 0 {
                    (std::f32::consts::FRAC_PI_2, std::f32::consts::PI * 1.5)
                } else {
                    (-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2)
                }
            } else {
                match index {
                    0 => (std::f32::consts::PI, std::f32::consts::PI * 1.5),
                    1 => (-std::f32::consts::FRAC_PI_2, 0.),
                    2 => (std::f32::consts::FRAC_PI_2, std::f32::consts::PI),
                    _ => (0., std::f32::consts::FRAC_PI_2),
                }
            };
            let mut mesh = egui::Mesh::with_texture(texture.id());
            let uv = |p: egui::Pos2| {
                egui::pos2(
                    uv_min.x + (p.x - cell_min.x) / cell.x * extent.x,
                    uv_min.y + (p.y - cell_min.y) / cell.y * extent.y,
                )
            };
            let center = rect.center();
            mesh.vertices.push(egui::epaint::Vertex {
                pos: center,
                uv: uv(center),
                color: Color32::WHITE,
            });
            for step in 0..=24 {
                let angle = start + (end - start) * step as f32 / 24.;
                let p = center + egui::vec2(angle.cos(), angle.sin()) * size / 2.;
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: p,
                    uv: uv(p),
                    color: Color32::WHITE,
                });
                if step > 0 {
                    mesh.indices.extend_from_slice(&[0, step, step + 1]);
                }
            }
            ui.painter().add(egui::Shape::mesh(mesh));
        }
        response
    }
    fn emoji_button(&mut self, ui: &mut egui::Ui, value: &str, size: f32) -> egui::Response {
        egui::Frame::new()
            .fill(self.theme.surface)
            .corner_radius(4)
            .inner_margin(egui::Margin::same(4))
            .show(ui, |ui| {
                self.emoji(
                    ui,
                    &StatusEmoji {
                        name: value.into(),
                        ..Default::default()
                    },
                    size,
                )
            })
            .response
            .interact(egui::Sense::click())
    }
    pub(super) fn emoji_section<R>(&mut self, ui: &mut egui::Ui, id_salt: impl std::hash::Hash,
        add_contents: impl FnOnce(&mut Self,&mut egui::Ui)->R) -> egui::InnerResponse<R> {
        self.pictures.begin_emoji_section();
        let result=ui.scope_builder(egui::UiBuilder::new().id_salt(id_salt),|ui|add_contents(self,ui));
        self.pictures.end_emoji_section(ui,result.response.rect);
        result
    }
    fn emoji(&mut self, ui: &mut egui::Ui, emoji: &StatusEmoji, size: f32) -> egui::Response {
        self.emoji_at_resolution(ui,emoji,size,32)
    }
    fn emoji_at_resolution(&mut self, ui: &mut egui::Ui, emoji: &StatusEmoji, size: f32, source_side: u32) -> egui::Response {
        let response=self.emoji_at_resolution_inner(ui,emoji,size,source_side);
        autoscroll::exclude(ui,response.rect);
        response
    }
    fn emoji_at_resolution_inner(&mut self, ui: &mut egui::Ui, emoji: &StatusEmoji, size: f32, source_side: u32) -> egui::Response {
        if !ui.is_rect_visible(egui::Rect::from_min_size(
            ui.cursor().min,
            egui::Vec2::splat(size),
        )) {
            return ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::hover()).1;
        }
        if let Some(url) = emoji.image_url() {
            let url=if source_side==32 {url}else{url.replace("?size=32",&format!("?size={source_side}"))};
            let deferred=self.pictures.has_emoji_section();
            let playing = emoji.animated && self.prefs.allow_gif_playback && self.pictures.custom_emoji_playing()
                && !ui.ctx().input(|input|input.viewport().minimized.unwrap_or(false));
            let animated = if playing && !deferred {
                self.pictures.animated_emoji(ui.ctx(), &url)
            } else { None };
            // A paused custom emoji requests the first static frame, not an
            // animated response which would be decoded just to discard frames.
            let static_url = if emoji.animated { url.replace("&animated=true", "") } else { url.clone() };
            if let Some(texture) = animated {
                ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(size, size))).on_hover_text(format!(":{}:", emoji.name))
            } else if let Some(texture)=self.pictures.texture_sized(&static_url,(size*ui.ctx().pixels_per_point()).ceil() as u32) {
                let (rect,response)=ui.allocate_exact_size(egui::Vec2::splat(size),egui::Sense::hover());
                let shape=ui.painter().add(egui::Shape::image(texture.id(),rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),Color32::WHITE));
                if deferred && emoji.animated && self.prefs.allow_gif_playback {
                    self.pictures.stage_emoji(ui.painter().clone(),shape,rect,url);
                }
                response.on_hover_text(format!(":{}:",emoji.name))
            } else if self.pictures.image_error(&static_url, (size * ui.ctx().pixels_per_point()).ceil() as u32).is_some() {
                ui.label(RichText::new(format!(":{}:", emoji.name)).size(size.min(self.prefs.chat_size))).on_hover_text("No se pudo cargar el emoji personalizado.")
            } else {
                self.pictures.image(ui, &static_url, egui::vec2(size, size), false).on_hover_text(format!(":{}:", emoji.name))
            }
        } else {
            let key = (
                emoji.name.clone(),
                (size * ui.ctx().pixels_per_point()).ceil().clamp(16., 192.) as u32,
            );
            if !self.emoji_textures.contains_key(&key)
                && !self.emoji_pending.contains_key(&key)
                && !self.pictures.visuals_suspended()
                && self
                    .emoji_failed
                    .get(&key)
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(60))
            {
                let generation = self.emoji_generation.load(std::sync::atomic::Ordering::Acquire);
                if self.emoji_request.try_send((generation, key.clone())).is_ok() {
                    self.emoji_pending.insert(key.clone(), generation);
                }
            }
            if let Some(t) = self.emoji_textures.get(&key) {
                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(size, size)))
            } else if self.emoji_failed.contains_key(&key) && !self.emoji_pending.contains_key(&key) {
                // Preserve readable fallback on systems where native color
                // rasterization actually failed; never preload it while pending.
                ui.label(RichText::new(&emoji.name).size(size))
            } else {
                // Do not rasterize the same Unicode glyph into egui's font
                // atlas while DirectWrite is preparing its color bitmap.
                let (rect, response) = ui.allocate_exact_size(
                    egui::Vec2::splat(size),
                    egui::Sense::hover(),
                );
                let tint = self.theme.muted.gamma_multiply(0.45);
                for offset in [-0.18, 0., 0.18] {
                    ui.painter().circle_filled(
                        rect.center() + egui::vec2(size * offset, 0.),
                        (size * 0.045).max(1.),
                        tint,
                    );
                }
                response.on_hover_text(&emoji.name)
            }
        }
    }
}
impl eframe::App for NotDiscordUi {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.updater.poll(ctx,self.preview.is_none());
        if self.updater.take_shutdown_request() {
            match self.prepare_orderly_close() {
                Ok(())=>ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(error)=>self.updater.cancel_prepared(error),
            }
        }
        crate::platform::remember_root_window(ctx, frame);
        let minimized = ctx.input(|input| input.viewport().minimized.unwrap_or(false));
        let suspension_changed = self.pictures.visuals_suspended() != minimized;
        self.pictures.set_visual_suspended(minimized);
        if minimized && suspension_changed {
            self.emoji_generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            self.emoji_pending.clear();
            chat::suspend_interactions(ctx);
            dialogs::suspend_search_keyboard(ctx);
            egui::Popup::close_all(ctx);
            self.search_open = false;
            self.search_focused = false;
            self.mention_closed = true;
            self.picker = None;
            self.emojis = Vec::new();
            ctx.memory_mut(|memory| {
                if let Some(focused) = memory.focused() { memory.surrender_focus(focused); }
            });
        }
        if !matches!(self.dialog,Some(Dialog::Sounds)) {self.sound_importer.cancel();}
        if let Some(result)=self.sound_importer.poll() {
            match result {
                Ok(imported)=>{let mut setting=self.sound_preferences.setting(imported.event);setting.file=Some(imported.path);setting.source_name=imported.source_name;self.sound_preferences.events.insert(imported.event,setting);self.sounds.clear_cache();self.dialog_error=if imported.reused{"Sonido importado reutilizado. Guarda para conservarlo."}else{"Sonido convertido. Guarda para conservarlo."}.into();},
                Err(error)=>self.dialog_error=error,
            }
        }
        self.pictures.begin_frame();
        self.pictures.set_download_limit(self.prefs.image_download_limit_bytes());
        if !minimized { chat::prepare_composer_input(ctx); }
        if let Some((path, _, _)) = &self.preview {
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let pixels = image
                        .pixels
                        .iter()
                        .flat_map(|p| p.to_array())
                        .collect::<Vec<_>>();
                    if image::save_buffer(
                        path,
                        &pixels,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    )
                    .is_ok()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
        self.process_updates(ctx);
        if !minimized {
            let scripts = if self.snapshot_fonts_pending && self.cjk_loaded != 3 {
                snapshot_scripts(&self.state)
            } else { 0 };
            self.snapshot_fonts_pending = false;
            self.ensure_script_fonts(scripts);
        }
        if self.preview.is_none() && self.state.signed_in && self.video_frames.is_some() {
            let visible = !minimized
                && ((self.state.view == "chat"
                    && voice::call_panel_visible(&self.state))
                    || matches!(
                        self.dialog,
                        Some(
                            Dialog::Media
                                | Dialog::Sources
                                | Dialog::Cameras
                                | Dialog::CamerasPreview
                        )
                    ));
            if self.video_visibility != Some(visible) {
                self.send(
                    "video-visible",
                    "",
                    if visible { "true" } else { "false" },
                    "",
                    "",
                );
                self.video_visibility = Some(visible);
            }
        }
        self.check_selection_preview_closed();
        self.set_selection_preview_suspended(minimized);
        if !minimized && !self.background_blocked() {
            self.poll_clipboard_attachments();
        }
        self.check_mic_test_dialog_closed();
        if !minimized && self.cjk_loaded != 3 {
            let scripts = ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|event| {
                        if let egui::Event::Text(text) | egui::Event::Paste(text) = event {
                            Some(cjk_scripts(text))
                        } else {
                            None
                        }
                    })
                    .fold(0, |a, b| a | b)
            });
            self.ensure_script_fonts(scripts);
        }
        if self.focus_until.is_some_and(|end| Instant::now() > end) {
            self.focus_id.clear();
            self.focus_until = None;
        }
        let zoom = (!minimized && !self.background_blocked()).then(|| ctx.input(|i| {
            if i.modifiers.ctrl {
                if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                    Some(self.prefs.zoom + 0.1)
                } else if i.key_pressed(egui::Key::Minus) {
                    Some(self.prefs.zoom - 0.1)
                } else if i.key_pressed(egui::Key::Num0) {
                    Some(1.)
                } else {
                    None
                }
            } else {
                None
            }
        })).flatten();
        if let Some(zoom) = zoom {
            self.prefs.zoom = zoom.clamp(0.8, 1.5);
            let _ = self.prefs.save(&self.data_dir);
            self.theme.apply(ctx, &self.prefs);
        }
        let state = std::mem::replace(&mut self.state, AppSnapshot::welcome());
        ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("chat-visibility"),
                (String::new(), String::new(), false),
            )
        });
        member_demand::begin_frame(ctx);
        autoscroll::begin_frame(ctx);
        if !minimized {
            self.dismiss_outside_overlays(ctx);
            self.draw_modal_barrier(ctx);
            self.draw_shell(ctx, &state);
            self.draw_dialog(ctx, &state);
            self.draw_picker(ctx, &state);
            self.draw_viewer(ctx);
        }
        self.update_member_demand(ctx, &state);
        self.check_selection_preview_closed();
        // Incoming-call windows and their sounds remain useful while the main
        // window is minimized; only its ordinary visual surfaces are suspended.
        self.draw_incoming_popup(ctx, &state);
        autoscroll::end_frame(ctx);
        self.sync_ring_sound(&state);
        self.backdrop.update(
            frame,
            self.prefs.blur,
            self.prefs.opacity,
            !minimized,
        );
        if state.signed_in && self.preview.is_none() {
            let (channel, message, bottom) = ctx
                .data(|data| {
                    data.get_temp::<(String, String, bool)>(egui::Id::new("chat-visibility"))
                })
                .unwrap_or_default();
            let reading = bottom
                && !minimized
                && !state.busy
                && self.dialog.is_none()
                && self.viewer.is_none()
                && ctx.input(|input| input.focused)
                && crate::platform::foreground_is_current_process();
            let account = state
                .current_user
                .as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_default();
            let report = (account, channel.clone(), message.clone(), reading);
            let previous = ctx.data(|data| {
                data.get_temp::<(String, String, String, bool)>(egui::Id::new(
                    "last-chat-visibility",
                ))
            });
            if previous.as_ref() != Some(&report) {
                self.send(
                    "chat-visibility",
                    &message,
                    if reading { "reading" } else { "" },
                    "",
                    &channel,
                );
                ctx.data_mut(|data| {
                    data.insert_temp(egui::Id::new("last-chat-visibility"), report)
                });
            }
        }
        self.pictures
            .finish_frame(minimized);
        if !minimized { theme::apply_interactive_cursor(ctx); }
        self.state = state;
        if let Some((_, frame, started)) = &mut self.preview {
            if *frame != u8::MAX {
                *frame = frame.saturating_add(1).min(u8::MAX - 1);
                if *frame >= 12 && started.elapsed() >= self.preview_delay {
                    *frame = u8::MAX;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::UserData::default(),
                    ));
                } else {
                    ctx.request_repaint_after(Duration::from_millis(16));
                }
            }
        }
    }
}
impl Drop for NotDiscordUi {
    fn drop(&mut self) {
        let owned = self.owned_temporary.iter().cloned().collect::<Vec<_>>();
        self.cleanup_temporary(&owned);
    }
}
fn field<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key)
        .or_else(|| {
            let mut chars = key.chars();
            let lower = chars
                .next()
                .map(|c| c.to_lowercase().to_string() + chars.as_str())?;
            v.get(lower)
        })
        .or_else(|| {
            let mut snake = String::new();
            for (i, c) in key.chars().enumerate() {
                if c.is_uppercase() && i > 0 {
                    snake.push('_')
                }
                snake.extend(c.to_lowercase());
            }
            v.get(snake)
        })
}
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    field(v, key).and_then(Value::as_str).unwrap_or("")
}
fn short(text: &str, max: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    if text.graphemes(true).count() <= max {
        text.into()
    } else {
        text.graphemes(true)
            .take(max.saturating_sub(1))
            .chain(["…"])
            .collect()
    }
}
fn presence_color(value: &str) -> Color32 {
    match value {
        "online" => Color32::from_rgb(35, 165, 89),
        "idle" => Color32::from_rgb(240, 178, 50),
        "dnd" => Color32::from_rgb(242, 63, 67),
        _ => Color32::from_rgb(128, 132, 142),
    }
}
fn activity(a: &PresenceActivity) -> String {
    let prefix = match a.kind {
        2 => "",
        1 => "Transmitiendo ",
        3 => "Viendo ",
        5 => "Compitiendo en ",
        _ => "Jugando a ",
    };
    let mut text = format!("{}{}", prefix, a.name);
    if !a.details.is_empty() {
        text.push_str(" · ");
        text.push_str(&a.details)
    }
    if !a.state.is_empty() {
        text.push_str(" — ");
        text.push_str(&a.state)
    }
    text
}
static EMOJI_CATALOG_TSV: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/emoji-catalog.tsv"
    ));
fn load_emoji_catalog() -> Vec<(&'static str, &'static str, &'static str)> {
    EMOJI_CATALOG_TSV
    .lines()
    .filter(|l| !l.starts_with('#'))
    .filter_map(|l| {
        let mut fields = l.splitn(3, '\t');
        Some((fields.next()?, fields.next()?, fields.next()?))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_use_local_day_and_spanish_months_without_changing_host_timezone() {
        let fixture: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../evidence/ui-date-fixture.json"
        )))
        .unwrap();
        let timestamp: Timestamp = fixture["timestamp"].as_str().unwrap().parse().unwrap();
        let lima = chrono::FixedOffset::east_opt(fixture["offsetSeconds"].as_i64().unwrap() as i32)
            .unwrap();
        let local = localise(&timestamp, &lima);
        assert_eq!(
            local.date_naive().to_string(),
            fixture["localDay"].as_str().unwrap()
        );
        assert_eq!(
            local.format("%H:%M").to_string(),
            fixture["localTime"].as_str().unwrap()
        );
        assert_eq!(
            spanish_date(local.date_naive(), true),
            fixture["separator"].as_str().unwrap()
        );
        assert_eq!(
            spanish_date(local.date_naive(), false),
            fixture["profile"].as_str().unwrap()
        );
        let same_local_day: Timestamp = "2026-10-19T23:00:00Z".parse().unwrap();
        assert_eq!(
            localise(&same_local_day, &lima).date_naive(),
            local.date_naive()
        );
        let next_local_day: Timestamp = "2026-10-20T05:00:00Z".parse().unwrap();
        assert_ne!(
            localise(&next_local_day, &lima).date_naive(),
            local.date_naive()
        );
        for (month, name) in [
            (1, "enero"),
            (2, "febrero"),
            (3, "marzo"),
            (4, "abril"),
            (5, "mayo"),
            (6, "junio"),
            (7, "julio"),
            (8, "agosto"),
            (9, "septiembre"),
            (10, "octubre"),
            (11, "noviembre"),
            (12, "diciembre"),
        ] {
            assert_eq!(
                spanish_date(
                    chrono::NaiveDate::from_ymd_opt(2026, month, 1).unwrap(),
                    true
                ),
                format!("1 de {name} de 2026")
            );
        }
    }
    pub(super) fn harness() -> (
        NotDiscordUi,
        std::sync::mpsc::Sender<UiUpdate>,
        tokio::sync::mpsc::UnboundedReceiver<UiAction>,
    ) {
        let (tx, commands) = tokio::sync::mpsc::unbounded_channel();
        let (updates, rx) = std::sync::mpsc::channel();
        let app = NotDiscordUi::from_context(
            &egui::Context::default(),
            std::env::temp_dir().join(format!("notdiscord-ui-test-{}", uuid::Uuid::new_v4())),
            tx,
            rx,
        );
        (app, updates, commands)
    }
    fn signed_in(account: &str) -> AppSnapshot {
        AppSnapshot {
            signed_in: true,
            current_user: Some(Person {
                id: account.into(),
                ..Default::default()
            }),
            view: "chat".into(),
            selected_channel_id: "channel".into(),
            ..AppSnapshot::welcome()
        }
    }
    #[test]
    fn late_ack_preserves_new_text_and_attachments() {
        let (mut app, _, _) = harness();
        app.drafts.insert(
            "channel".into(),
            Draft {
                text: "texto nuevo".into(),
                revision: 8,
                paths: vec![PathBuf::from("new.png")],
                ..Default::default()
            },
        );
        app.submissions.insert("sent".into(), ("channel".into(), 7));
        app.data_update(
            "send-ack",
            serde_json::json!({"submissionId":"sent","channelId":"channel","revision":7}),
        );
        assert_eq!(app.drafts["channel"].text, "texto nuevo");
        assert_eq!(app.drafts["channel"].paths.len(), 1);
    }
    #[test]
    fn observed_cjk_versions_request_fonts_even_when_current_content_is_latin() {
        let (mut app, updates, _) = harness();
        let mut snapshot = signed_in("first");
        snapshot.messages = vec![ChatMessage { id:"1".into(), channel_id:"channel".into(),
            content:"New Latin text".into(), previous_contents:vec!["日本語".into()], ..Default::default() }].into();
        updates.send(UiUpdate::Snapshot(snapshot)).unwrap();
        let ctx = app.ctx.clone();
        app.process_updates(&ctx);
        assert_eq!(app.cjk_loaded & 1, 1);
    }
    #[test]
    fn ack_only_clears_its_sent_revision_and_channel() {
        let (mut app, _, _) = harness();
        app.drafts.insert(
            "channel".into(),
            Draft {
                text: "enviado".into(),
                revision: 7,
                ..Default::default()
            },
        );
        app.drafts.insert(
            "other".into(),
            Draft {
                text: "conservar".into(),
                ..Default::default()
            },
        );
        app.submissions.insert("sent".into(), ("channel".into(), 7));
        app.data_update("send-ack", serde_json::json!({"submissionId":"sent"}));
        assert!(!app.drafts.contains_key("channel"));
        assert_eq!(app.drafts["other"].text, "conservar");
    }
    #[test]
    fn replacing_account_clears_ephemeral_state() {
        let (mut app, updates, _) = harness();
        updates
            .send(UiUpdate::Snapshot(signed_in("first")))
            .unwrap();
        let ctx = app.ctx.clone();
        app.process_updates(&ctx);
        app.draft("channel").unwrap().text = "privado".into();
        app.positions.insert(
            "channel".into(),
            ChatPosition {
                offset: 120.,
                anchor: "message".into(),
                ..Default::default()
            },
        );
        updates
            .send(UiUpdate::Snapshot(signed_in("second")))
            .unwrap();
        app.process_updates(&ctx);
        assert!(app.drafts.is_empty());
        assert!(app.positions.is_empty());
    }
    #[test]
    fn pins_begin_at_top_and_preserve_chat_anchor() {
        let (mut app, updates, _) = harness();
        let ctx = app.ctx.clone();
        updates.send(UiUpdate::Snapshot(signed_in("one"))).unwrap();
        app.process_updates(&ctx);
        app.positions.insert(
            "channel".into(),
            ChatPosition {
                offset: 420.,
                anchor: "message".into(),
                anchor_top: -4.,
                at_bottom: false,
                distance_to_bottom: None,
            },
        );
        let mut pins = signed_in("one");
        pins.view = "pins".into();
        updates.send(UiUpdate::Snapshot(pins)).unwrap();
        app.process_updates(&ctx);
        assert_eq!(app.pending_scroll, Some(0.));
        assert_eq!(app.positions["channel"].anchor, "message");
        updates.send(UiUpdate::Snapshot(signed_in("one"))).unwrap();
        app.process_updates(&ctx);
        assert_eq!(app.restore_anchor, Some(("message".into(), -4.)));
    }
    #[test]
    fn opening_source_or_camera_picker_does_not_start_capture() {
        let (mut app, _, mut commands) = harness();
        app.data_update(
            "sources",
            serde_json::json!([{"id":"screen-handle:1","name":"Test","type":"screen"}]),
        );
        assert!(matches!(app.dialog, Some(Dialog::Sources)));
        assert!(app.prompt.is_empty());
        assert!(!app.share_audio);
        assert!(commands.try_recv().is_err());
        app.data_update(
            "cameras",
            serde_json::json!([{"id":"camera-guid","name":"Test"}]),
        );
        assert!(matches!(app.dialog, Some(Dialog::Cameras)));
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn activity_truncation_does_not_split_emoji_sequences() {
        let family = "👨‍👩‍👧‍👦";
        let text = family.repeat(25);
        let clipped = short(&text, 20);
        use unicode_segmentation::UnicodeSegmentation;
        assert_eq!(clipped.graphemes(true).count(), 20);
        assert_eq!(clipped, format!("{}…", family.repeat(19)));
    }
    #[test]
    fn emoji_catalog_is_bundled_not_runtime_source_path() {
        let catalog = load_emoji_catalog();
        assert!(catalog.len() > 1000);
        assert!(
            catalog
                .iter()
                .any(|e| e.0 == "😀" && e.2.contains("sonriendo"))
        );
    }
    #[test]
    fn unicode_catalog_rows_borrow_the_bundled_metadata_instead_of_allocating_strings() {
        let source = EMOJI_CATALOG_TSV;
        let start = source.as_ptr() as usize;
        let end = start + source.len();
        let catalog = load_emoji_catalog();
        assert!(catalog.len() > 3000);
        for (value, category, search) in &catalog {
            for field in [value, category, search] {
                let pointer = field.as_ptr() as usize;
                assert!(pointer >= start && pointer + field.len() <= end);
            }
        }
        assert!(catalog.capacity() * std::mem::size_of::<(&str, &str, &str)>() < source.len());
    }
    #[test]
    fn pending_unicode_emoji_uses_geometry_without_adding_a_fallback_font_glyph() {
        let (mut app, _, _) = harness();
        let ctx = app.ctx.clone();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.emoji(ui, &StatusEmoji { name: "😀".into(), ..Default::default() }, 26.);
            });
        });
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("😀"))));
        assert!(output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::Circle(_))).count() >= 3);
        let key = app.emoji_pending.keys().next().cloned().unwrap();
        app.emoji_pending.remove(&key);
        app.emoji_failed.insert(key, Instant::now());
        let failed = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.emoji(ui, &StatusEmoji { name: "😀".into(), ..Default::default() }, 26.);
            });
        });
        assert!(failed.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("😀"))), "native failure retains the existing readable fallback");
    }
    #[test]
    fn data_contract_accepts_pascal_camel_and_snake() {
        let v = serde_json::json!({"ThumbnailUrl":"a","animationUrl":"b","has_more":true});
        assert_eq!(s(&v, "ThumbnailUrl"), "a");
        assert_eq!(s(&v, "AnimationUrl"), "b");
        assert_eq!(field(&v, "HasMore").and_then(Value::as_bool), Some(true));
    }
    #[test]
    fn visual_fixture_decodes_without_private_state() {
        let bytes = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../evidence/ui-fixture.json"
        ));
        let state: AppSnapshot =
            serde_json::from_str(bytes.trim_start_matches('\u{feff}')).unwrap();
        assert!(state.demo);
        assert_eq!(state.view, "chat");
        assert!(state.messages.iter().any(|m| m.forwarded));
        assert!(state.messages.iter().any(|m| m.reply_preview.is_some()));
        assert!(state.guild_members.iter().any(|m| !m.activities.is_empty()));
    }
    #[test]
    fn draft_capacity_never_evicts_existing_work() {
        let (mut app, _, _) = harness();
        for index in 0..1024 {
            app.drafts.insert(
                index.to_string(),
                Draft {
                    text: format!("borrador {index}"),
                    ..Default::default()
                },
            );
        }
        assert!(app.draft("1025").is_none());
        assert_eq!(app.drafts.len(), 1024);
        assert_eq!(app.drafts["0"].text, "borrador 0");
        assert!(app.error.contains("1024"));
    }
    #[test]
    fn ack_removes_sent_files_but_keeps_later_files_and_text() {
        let (mut app, _, _) = harness();
        let old = PathBuf::from("old.png");
        let new = PathBuf::from("new.png");
        app.drafts.insert(
            "channel".into(),
            Draft {
                text: "texto nuevo".into(),
                revision: 8,
                paths: vec![old.clone(), new.clone()],
                ..Default::default()
            },
        );
        app.submissions.insert("sent".into(), ("channel".into(), 7));
        app.submitted_paths.insert("sent".into(), vec![old]);
        app.data_update("send-ack", serde_json::json!({"submissionId":"sent"}));
        assert_eq!(app.drafts["channel"].paths, vec![new]);
        assert_eq!(app.drafts["channel"].text, "texto nuevo");
    }
    #[test]
    fn only_owned_clipboard_temporaries_are_deleted() {
        let (mut app, _, _) = harness();
        let parent = app.data_dir.join("draft-attachments");
        std::fs::create_dir_all(&parent).unwrap();
        let owned = parent.join(format!("{}.png", uuid::Uuid::new_v4()));
        let user = parent.join("user-image.png");
        std::fs::write(&owned, b"test").unwrap();
        std::fs::write(&user, b"keep").unwrap();
        app.owned_temporary.insert(owned.clone());
        app.cleanup_temporary(&[owned.clone(), user.clone()]);
        assert!(!owned.exists());
        assert!(user.exists());
        std::fs::remove_file(user).unwrap();
        std::fs::remove_dir(parent).unwrap();
        std::fs::remove_dir(&app.data_dir).unwrap();
    }
    #[test]
    fn open_local_viewer_holds_pasted_file_until_its_last_reference_closes() {
        let (mut app, _, _) = harness();
        let parent = app.data_dir.join("draft-attachments");
        std::fs::create_dir_all(&parent).unwrap();
        let owned = parent.join(format!("{}.png", uuid::Uuid::new_v4()));
        std::fs::write(&owned, b"offline local source").unwrap();
        app.owned_temporary.insert(owned.clone());
        app.viewer = Some(Viewer { url: String::new(), local_path: Some(owned.clone()), zoom: 1., pan: egui::Vec2::ZERO });
        app.cleanup_unreferenced_temporary(&[owned.clone()]);
        assert!(owned.exists(), "a pending acknowledgement must not delete the open preview source");
        let ctx = app.ctx.clone();
        let _ = ctx.run(egui::RawInput {
            events: vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            ..Default::default()
        }, |ctx| app.draw_viewer(ctx));
        assert!(app.viewer.is_none());
        assert!(!owned.exists(), "closing releases a source no remaining draft references");
        std::fs::remove_dir(parent).unwrap();
        std::fs::remove_dir(&app.data_dir).unwrap();
    }
    #[test]
    fn local_viewer_and_draft_survive_navigation_but_do_not_cross_accounts() {
        let (mut app, updates, _) = harness();
        let ctx = app.ctx.clone();
        updates.send(UiUpdate::Snapshot(signed_in("offline-first"))).unwrap();
        app.process_updates(&ctx);
        let parent = app.data_dir.join("draft-attachments");
        std::fs::create_dir_all(&parent).unwrap();
        let pasted = parent.join(format!("{}.png", uuid::Uuid::new_v4()));
        let imported = parent.join("user-original.png");
        std::fs::write(&pasted, b"pasted offline").unwrap();
        std::fs::write(&imported, b"user original").unwrap();
        app.owned_temporary.insert(pasted.clone());
        app.drafts.insert("channel".into(), Draft { text: "Keep while navigating".into(), paths: vec![pasted.clone(), imported.clone()], ..Default::default() });
        app.viewer = Some(Viewer { url: String::new(), local_path: Some(pasted.clone()), zoom: 2., pan: egui::vec2(12., 15.) });
        let mut next = signed_in("offline-first");
        next.selected_channel_id = "other-channel".into();
        updates.send(UiUpdate::Snapshot(next)).unwrap();
        app.process_updates(&ctx);
        assert_eq!(app.viewer.as_ref().unwrap().local_path.as_ref(), Some(&pasted));
        assert_eq!(app.drafts["channel"].text, "Keep while navigating");
        assert!(pasted.exists());
        updates.send(UiUpdate::Snapshot(signed_in("offline-second"))).unwrap();
        app.process_updates(&ctx);
        assert!(app.viewer.is_none());
        assert!(app.drafts.is_empty());
        assert!(!pasted.exists());
        assert_eq!(std::fs::read(&imported).unwrap(), b"user original");
        std::fs::remove_file(imported).unwrap();
        std::fs::remove_dir(parent).unwrap();
        std::fs::remove_dir(&app.data_dir).unwrap();
    }
    #[test]
    fn notification_policy_barrier_accepts_only_current_session_valid_monotonic_policy() {
        let (mut app, _, _) = harness();
        app.state = signed_in("1");
        app.state.notification_generation = 7;
        app.state.notification_epoch = 3;
        app.state.self_presence = "dnd".into();
        let valid = serde_json::json!({"account_id":"1","generation":7,"epoch":3,"presence":"online"});
        let mut rejected = Vec::new();
        for (field, value) in [("account_id", serde_json::json!("2")), ("generation", serde_json::json!(6)),
            ("generation", serde_json::json!(8)), ("epoch", serde_json::json!(2)), ("presence", serde_json::json!("invalid"))] {
            let mut packet = valid.clone(); packet[field] = value; rejected.push(packet);
        }
        let mut missing_generation = valid.clone(); missing_generation.as_object_mut().unwrap().remove("generation"); rejected.push(missing_generation);
        let mut missing_epoch = valid.clone(); missing_epoch.as_object_mut().unwrap().remove("epoch"); rejected.push(missing_epoch);
        for packet in rejected {
            app.data_update("notification-policy", packet);
            assert_eq!((app.state.self_presence.as_str(), app.state.notification_epoch), ("dnd", 3));
        }
        app.data_update("notification-policy", valid);
        assert_eq!((app.state.self_presence.as_str(), app.state.notification_epoch), ("online", 3));
        app.state.signed_in = false;
        app.data_update("notification-policy", serde_json::json!({"account_id":"1","generation":7,"epoch":4,"presence":"dnd"}));
        assert_eq!((app.state.self_presence.as_str(), app.state.notification_epoch), ("online", 3));
    }
    #[test]
    fn latest_snapshot_slot_coalesces_without_queueing_old_states() {
        let (mut app, updates, _) = harness();
        let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(signed_in("first"))));
        updates
            .send(UiUpdate::LatestSnapshot(slot.clone()))
            .unwrap();
        let ctx = app.ctx.clone();
        app.process_updates(&ctx);
        *slot.lock().unwrap() = Some(signed_in("latest"));
        app.process_updates(&ctx);
        assert_eq!(app.state.current_user.as_ref().unwrap().id, "latest");
        assert!(slot.lock().unwrap().is_none());
    }
    #[test]
    fn fixture_draws_without_emitting_account_actions() {
        let (mut app, _, mut commands) = harness();
        let fixture: AppSnapshot = serde_json::from_str(
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../evidence/ui-fixture.json"
            ))
            .trim_start_matches('\u{feff}'),
        )
        .unwrap();
        let ctx = app.ctx.clone();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400., 900.),
                )),
                ..Default::default()
            },
            |ctx| app.draw_shell(ctx, &fixture),
        );
        assert!(output.shapes.len() > 30);
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn safety_banner_renders_without_network_actions_or_changes_to_a_draft() {
        let (mut app, _, mut commands) = harness();
        let mut state = signed_in("42");
        state.network_safety = notdiscord_core::safety::SafetyState::VerificationRequired;
        state.network_safety_message = "Synthetic verification stop".into();
        app.drafts.insert(
            "channel".into(),
            Draft {
                text: "Synthetic unsent draft".into(),
                ..Default::default()
            },
        );
        let ctx = app.ctx.clone();
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400., 900.),
                    )),
                    ..Default::default()
                },
                |ctx| app.draw_shell(ctx, &state),
            );
            assert!(!output.shapes.is_empty());
        }
        assert!(
            commands.try_recv().is_err(),
            "Rendering a stop must not retry, open a browser or sign in"
        );
        assert_eq!(app.drafts["channel"].text, "Synthetic unsent draft");
        assert_eq!(
            state.network_safety,
            notdiscord_core::safety::SafetyState::VerificationRequired
        );
    }
    #[test]
    fn cjk_fonts_load_only_on_demand_and_contain_japanese_glyphs() {
        let (mut app, updates, _) = harness();
        assert_eq!(app.cjk_loaded, 0);
        assert!(
            !app.fonts
                .font_data
                .keys()
                .any(|name| name.ends_with("YuGothM.ttc"))
        );
        let mut incoming = AppSnapshot {
            incoming_calls: vec![IncomingCall {
                channel_id: "50".into(),
                channel_name: "Latin group".into(),
                caller: Some(Person {
                    name: "Latin caller".into(),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..AppSnapshot::welcome()
        };
        updates.send(UiUpdate::Snapshot(incoming.clone())).unwrap();
        let context = app.ctx.clone();
        app.process_updates(&context);
        assert_eq!(
            app.cjk_loaded, 0,
            "A Latin incoming caller preloaded CJK fonts"
        );
        incoming.incoming_calls[0].caller.as_mut().unwrap().name = "永遠の希望".into();
        updates.send(UiUpdate::Snapshot(incoming)).unwrap();
        app.process_updates(&context);
        assert_eq!(app.cjk_loaded, 1);
        let font = app
            .fonts
            .font_data
            .iter()
            .find(|(name, _)| name.ends_with("YuGothM.ttc"))
            .unwrap()
            .1;
        assert_eq!(font.index, 0);
        let ctx = app.ctx.clone();
        let mut covered = false;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            covered = ctx.fonts_mut(|fonts| {
                fonts.has_glyphs(&egui::FontId::proportional(16.), "永遠の希望")
            });
            assert!(
                ctx.fonts_mut(|fonts| fonts.has_glyphs(&egui::FontId::proportional(16.), "⌄➤⌕"))
            );
            let galley = ctx.fonts_mut(|fonts| {
                fonts.layout_no_wrap(
                    "永遠の希望".into(),
                    egui::FontId::proportional(16.),
                    Color32::WHITE,
                )
            });
            assert_eq!(
                galley
                    .rows
                    .iter()
                    .map(|row| row.glyphs.len())
                    .sum::<usize>(),
                5
            );
        });
        assert!(covered);
    }
    #[test]
    fn large_navigation_renders_only_visible_rows() {
        let (mut app, _, mut commands) = harness();
        let mut state = signed_in("self");
        state.view = "friends".into();
        state.direct_messages = (0..1000)
            .map(|index| NavItem {
                id: index.to_string(),
                name: format!("Persona {index}"),
                kind: "dm".into(),
                ..Default::default()
            })
            .collect();
        let ctx = app.ctx.clone();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400., 900.),
                )),
                ..Default::default()
            },
            |ctx| app.draw_shell(ctx, &state),
        );
        assert!(!app.nav_heights.is_empty());
        assert!(app.nav_heights.len() < 30);
        assert!(commands.try_recv().is_err());
    }
}
