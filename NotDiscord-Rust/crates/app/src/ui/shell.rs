use super::*;
use eframe::egui::{self, Align, Layout, Vec2};

#[derive(Clone)]
struct FriendsFilter {
    online_only: bool,
    query: String,
}
impl Default for FriendsFilter {
    fn default() -> Self { Self { online_only: true, query: String::new() } }
}
fn friends_visible<'a>(state: &'a AppSnapshot, filter: &FriendsFilter) -> Vec<&'a Relationship> {
    let query = filter.query.trim().trim_start_matches('@').to_lowercase();
    state
        .relationships
        .iter()
        .filter(|relation| {
            let category = if state.view == "requests" {
                matches!(relation.kind, 3 | 4)
            } else {
                relation.kind == 1
            };
            let connected = state.view == "requests" || !filter.online_only
                || state
                    .user_presences
                    .get(&relation.user.id)
                    .is_some_and(|presence| {
                        matches!(presence.status.as_str(), "online" | "idle" | "dnd")
                    });
            let person = &relation.user;
            category
                && (state.view == "requests" || connected)
                && (query.is_empty()
                    || [
                        person.name.as_str(),
                        person.display_name.as_str(),
                        person.username.as_str(),
                    ]
                    .iter()
                    .any(|name| name.to_lowercase().contains(&query)))
        })
        .collect()
}

fn has_status(text: &str, emoji: Option<&StatusEmoji>) -> bool {
    !text.trim().is_empty() || emoji.is_some()
}
fn mic_test_blocks_calls(ctx: &egui::Context) -> bool {
    ctx.data(|data|data.get_temp::<Value>(egui::Id::new("mic-test-state")))
        .is_some_and(|state|state["active"]==true || state["pending"]==true)
}
fn nav_metrics(item: &NavItem) -> (u8, f32) {
    if matches!(item.kind.as_str(), "category" | "section") {
        return (128, 35.);
    }
    let private = matches!(item.kind.as_str(), "dm" | "group");
    let custom = has_status(&item.custom_status, item.custom_status_emoji.as_ref());
    let secondary = custom
        || !item.activity_text.is_empty()
        || !item.subtitle.is_empty()
        || !item.activities.is_empty();
    let flags = u8::from(secondary) | u8::from(item.has_active_call) * 2 | u8::from(private) * 4;
    (
        flags,
        if private {
            if item.has_active_call && secondary {
                70.
            } else {
                56.
            }
        } else {
            38.
        },
    )
}
pub(super) fn fitted_text(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    text: &str,
    size: f32,
    color: Color32,
) -> egui::Response {
    let mut job = egui::text::LayoutJob::simple(
        text.split_whitespace().collect::<Vec<_>>().join(" "),
        egui::FontId::proportional(size),
        color,
        rect.width().max(0.),
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let position = rect.left_center() - egui::vec2(0., galley.size().y / 2.);
    ui.painter()
        .with_clip_rect(rect)
        .galley(position, galley, color);
    ui.interact(
        rect,
        ui.id()
            .with(("fit-text", rect.left().to_bits(), rect.top().to_bits())),
        egui::Sense::hover(),
    )
    .on_hover_text(text)
}
fn paint_activity(ui: &egui::Ui, rect: egui::Rect, music: bool) {
    let color = Color32::from_rgb(35, 165, 90);
    let stroke = egui::Stroke::new(1.6_f32, color);
    let point = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + rect.width() * x / 24.,
            rect.top() + rect.height() * y / 24.,
        )
    };
    if music {
        ui.painter()
            .line_segment([point(8., 19.), point(8., 5.)], stroke);
        ui.painter()
            .line_segment([point(8., 5.), point(20., 2.)], stroke);
        ui.painter()
            .line_segment([point(20., 2.), point(20., 16.)], stroke);
        ui.painter().add(egui::Shape::ellipse_filled(
            point(5., 19.),
            egui::vec2(rect.width() * 0.16, rect.height() * 0.11),
            color,
        ));
        ui.painter().add(egui::Shape::ellipse_filled(
            point(17., 16.),
            egui::vec2(rect.width() * 0.16, rect.height() * 0.11),
            color,
        ));
    } else {
        ui.painter().add(egui::Shape::closed_line(
            vec![
                point(5., 5.),
                point(19., 5.),
                point(23., 19.),
                point(20., 22.),
                point(14., 17.),
                point(10., 17.),
                point(4., 22.),
                point(1., 19.),
            ],
            stroke,
        ));
        ui.painter()
            .line_segment([point(7., 8.), point(7., 14.)], stroke);
        ui.painter()
            .line_segment([point(4., 11.), point(10., 11.)], stroke);
        ui.painter().circle_filled(point(16., 10.), 1.1, color);
        ui.painter().circle_filled(point(19., 13.), 1.1, color);
    }
}
enum NavigationRow<'a> {
    Section(&'static str),
    Item(&'a NavItem),
}
fn category_key(state: &AppSnapshot) -> egui::Id {
    egui::Id::new(("collapsed-channel-categories", state.current_user.as_ref().map(|user| user.id.as_str()).unwrap_or(""), &state.selected_guild_id))
}
fn hidden_by_category(ctx: &egui::Context, state: &AppSnapshot, item: &NavItem) -> bool {
    if item.parent_id.is_empty() { return false; }
    ctx.data_mut(|data| {
        let collapsed = data.get_temp_mut_or_default::<HashSet<String>>(category_key(state));
        let mut parent = item.parent_id.as_str();
        for _ in 0..16 {
            if parent.is_empty() { break; }
            if collapsed.contains(parent) { return true; }
            let Some(item) = state.channels.iter().find(|item| item.id == parent) else { break; };
            parent = item.parent_id.as_str();
        }
        false
    })
}
fn own_game(state: &AppSnapshot) -> Option<&str> {
    (state.signed_in && state.game_activity_running && state.game_activity_configured && state.game_activity_enabled
        && matches!(state.self_presence.as_str(), "online" | "idle" | "dnd") && !state.custom_activity_label.trim().is_empty())
        .then_some(state.custom_activity_label.as_str())
}
pub(super) fn member_game(state:&AppSnapshot,id:&str,presence:&str,activities:&[PresenceActivity])->String {
    if matches!(presence,"offline"|"invisible") {return String::new();}
    let is_self=state.current_user.as_ref().is_some_and(|user|user.id==id);
    if is_self&&state.game_activity_configured {
        return own_game(state).map(|game|format!("Jugando a {game}")).unwrap_or_default();
    }
    activities.iter().find(|activity|matches!(activity.kind,0|5))
        .map(|activity|notdiscord_core::parsing::presence_summary(std::slice::from_ref(activity))).unwrap_or_default()
}
pub(super) fn guild_voice_game(state:&AppSnapshot,member:&notdiscord_core::guild_settings::GuildVoiceMember)->String {
    let cached=state.guild_members.iter().find(|person|person.user.id==member.user.id);
    let live=state.user_presences.get(&member.user.id);
    let presence=live.map(|presence|presence.status.as_str()).or_else(||cached.map(|member|member.presence.as_str())).unwrap_or("");
    let activities=live.map(|presence|presence.activities.as_slice()).or_else(||cached.map(|member|member.activities.as_slice())).unwrap_or(&[]);
    member_game(state,&member.user.id,presence,activities)
}
pub(super) fn draw_member_voice_flags(ui:&mut egui::Ui,rect:egui::Rect,state:&AppSnapshot,
    guild:&str,user:&str,channel:Option<&str>)->f32 {
    let snapshot=state.guild_voice_members.iter().find(|member|member.guild_id==guild&&member.user.id==user
        && channel.is_none_or(|channel|member.channel_id==channel)
        && state.channels.iter().any(|item|item.id==member.channel_id));
    let active=!guild.is_empty()&&state.call_guild_id==guild&&(state.voice_connected||state.voice_connecting)
        &&channel.is_none_or(|channel|state.call_channel_id==channel);
    let participant=active.then(||state.call_participants.iter().find(|person|person.user.id==user)).flatten();
    let (muted,deafened,camera,screen)=if let Some(person)=participant {
        (person.self_mute||person.server_mute,person.self_deaf||person.server_deaf,
            person.self_video||person.is_self&&state.camera_enabled,
            person.screen_sharing||person.is_self&&state.screen_sharing)
    } else if let Some(member)=snapshot {
        (member.muted||member.self_mute||member.server_mute,member.deafened||member.self_deaf||member.server_deaf,
            member.self_video,member.screen_sharing)
    } else {(false,false,false,false)};
    let mut right=rect.right()-4.;
    for (visible,icon,slash,label) in [(screen,Icon::Screen,false,"Compartiendo pantalla"),
        (camera,Icon::Video,false,"Cámara activa"),(deafened,Icon::Headphones,true,"Ensordecido"),
        (muted,Icon::Mic,true,"Micrófono silenciado")] {
        if visible {
            let flag=egui::Rect::from_min_size(egui::pos2(right-18.,rect.top()+5.),egui::vec2(18.,18.));
            ui.scope_builder(egui::UiBuilder::new().max_rect(flag),|ui|icon_button(ui,icon,18.,true,slash).on_hover_text(label));
            right-=20.;
        }
    }
    right
}
fn left_footer_height(state: &AppSnapshot, compact: bool) -> f32 {
    (if state.signed_in { 78. } else { 110. })
        + if own_game(state).is_some() { if compact { 42. } else { 64. } } else { 0. }
        + if state.voice_connected || state.voice_connecting { 64. } else { 0. }
        + if !state.voice_error.is_empty() { if compact { 86. } else { 122. } } else { 0. }
}
fn navigation_rows(state: &AppSnapshot) -> Vec<NavigationRow<'_>> {
    if !state.selected_guild_id.is_empty() {
        let hidden = state.guilds.iter().find(|guild|guild.id==state.selected_guild_id)
            .is_some_and(|guild|guild.guild_notifications.hide_muted_channels);
        if !hidden { return state.channels.iter().map(NavigationRow::Item).collect(); }
        let index = state.channels.iter().map(|item|(item.id.as_str(),item)).collect::<HashMap<_,_>>();
        let mut protected = HashSet::new();
        let mut selected = index.get(state.selected_channel_id.as_str()).copied();
        for _ in 0..16 {
            let Some(channel) = selected else { break; };
            protected.insert(channel.id.as_str());
            selected = index.get(channel.parent_id.as_str()).copied();
        }
        return state.channels.iter().filter(|item| !hidden_muted_channel(item,&index,&protected)).map(NavigationRow::Item).collect();
    }
    let mut rows = Vec::with_capacity(state.direct_messages.len() + 3);
    for (label, category) in [("LLAMADAS", 0), ("FIJADOS", 1), ("MENSAJES DIRECTOS", 2)] {
        let matches = |item: &&NavItem| match category {
            0 => item.has_active_call,
            1 => item.is_pinned && !item.has_active_call,
            _ => !item.is_pinned && !item.has_active_call,
        };
        if category == 2 || state.direct_messages.iter().any(|item| matches(&item)) {
            rows.push(NavigationRow::Section(label));
            rows.extend(
                state
                    .direct_messages
                    .iter()
                    .filter(matches)
                    .map(NavigationRow::Item),
            );
        }
    }
    rows
}
fn hidden_muted_channel(item: &NavItem, index: &HashMap<&str,&NavItem>, protected: &HashSet<&str>) -> bool {
    // Keep the open channel and its category reachable. A category mute also
    // hides its descendants; bounded traversal tolerates malformed cycles.
    if protected.contains(item.id.as_str()) { return false; }
    let mut current = Some(item);
    for _ in 0..16 {
        let Some(channel) = current else { break; };
        if channel.is_muted { return true; }
        current = index.get(channel.parent_id.as_str()).copied();
    }
    false
}
#[derive(Clone, Copy)]
pub(super) enum Icon {
    Phone,
    Video,
    Screen,
    Chat,
    Pin,
    Members,
    AddMember,
    Mic,
    Headphones,
    Activity,
    Settings,
}
pub(super) fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    disabled: bool,
    slash: bool,
) -> egui::Response {
    let disabled=disabled||!ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), if disabled {egui::Sense::hover()} else {egui::Sense::click()});
    let response=if disabled {response} else {response.on_hover_cursor(egui::CursorIcon::PointingHand)};
    let p = ui.painter();
    let fill = if response.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        ui.visuals().faint_bg_color
    };
    p.rect_filled(rect, 4., fill);
    let r = rect.shrink(size * 0.20);
    let color = if disabled {
        ui.visuals().weak_text_color()
    } else {
        ui.visuals().text_color()
    };
    let stroke = egui::Stroke::new(1.7_f32, color);
    let point = |x: f32, y: f32| {
        egui::pos2(
            r.left() + r.width() * x / 24.,
            r.top() + r.height() * y / 24.,
        )
    };
    let line =
        |a: (f32, f32), b: (f32, f32)| p.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    match icon {
        Icon::Phone => {
            super::icons::paint_call(p, r, color);
        }
        Icon::Video => {
            p.rect_stroke(
                egui::Rect::from_min_max(point(2., 5.), point(16., 19.)),
                1.,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.add(egui::Shape::closed_line(
                vec![
                    point(16., 9.),
                    point(23., 5.),
                    point(23., 19.),
                    point(16., 15.),
                ],
                stroke,
            ));
        }
        Icon::Screen => {
            p.rect_stroke(egui::Rect::from_min_max(point(2.,3.),point(22.,17.)),1.,stroke,egui::StrokeKind::Inside);
            line((12.,17.),(12.,22.));line((6.,22.),(18.,22.));
        }
        Icon::Chat => {
            p.add(egui::Shape::closed_line(vec![point(3.,3.),point(21.,3.),point(21.,17.),
                point(11.,17.),point(5.,22.),point(5.,17.),point(3.,17.)],stroke));
            line((7.,8.),(17.,8.));line((7.,12.),(15.,12.));
        }
        Icon::Pin => super::icons::paint_pin(p, r, color),
        Icon::Members | Icon::AddMember => {
            p.circle_stroke(point(9., 7.), r.width() * 0.15, stroke);
            p.add(egui::Shape::line(
                vec![
                    point(2., 22.),
                    point(3., 16.),
                    point(6., 14.),
                    point(12., 14.),
                    point(16., 17.),
                    point(16., 22.),
                ],
                stroke,
            ));
            if matches!(icon, Icon::AddMember) {
                line((19., 9.), (19., 19.));
                line((14., 14.), (24., 14.));
            } else {
                p.circle_stroke(point(19., 6.), r.width() * 0.11, stroke);
                line((20., 12.), (23., 17.));
                line((23., 17.), (23., 22.));
            }
        }
        Icon::Mic => {
            p.rect_stroke(
                egui::Rect::from_min_max(point(8., 1.), point(16., 15.)),
                5.,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.add(egui::Shape::line(
                vec![
                    point(4., 10.),
                    point(5., 16.),
                    point(9., 19.),
                    point(15., 19.),
                    point(19., 16.),
                    point(20., 10.),
                ],
                stroke,
            ));
            line((12., 19.), (12., 23.));
            line((7., 23.), (17., 23.));
        }
        Icon::Headphones => {
            p.add(egui::Shape::line(
                vec![
                    point(2., 21.),
                    point(2., 11.),
                    point(4., 5.),
                    point(9., 2.),
                    point(15., 2.),
                    point(20., 5.),
                    point(22., 11.),
                    point(22., 21.),
                ],
                stroke,
            ));
            p.rect_stroke(
                egui::Rect::from_min_max(point(2., 12.), point(7., 22.)),
                1.,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                egui::Rect::from_min_max(point(17., 12.), point(22., 22.)),
                1.,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Activity => {
            p.add(egui::Shape::closed_line(
                vec![
                    point(5., 5.),
                    point(19., 5.),
                    point(23., 19.),
                    point(20., 22.),
                    point(14., 17.),
                    point(10., 17.),
                    point(4., 22.),
                    point(1., 19.),
                ],
                stroke,
            ));
            line((7., 8.), (7., 14.));
            line((4., 11.), (10., 11.));
            p.circle_filled(point(16., 10.), 1.2, color);
            p.circle_filled(point(19., 13.), 1.2, color);
        }
        Icon::Settings => {
            p.circle_stroke(point(12., 12.), r.width() * 0.24, stroke);
            p.circle_stroke(point(12., 12.), r.width() * 0.43, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                line(
                    (12. + a.cos() * 9., 12. + a.sin() * 9.),
                    (12. + a.cos() * 12., 12. + a.sin() * 12.),
                );
            }
        }
    }
    if slash {
        p.line_segment(
            [point(1., 23.), point(23., 1.)],
            egui::Stroke::new(2.6_f32, Color32::from_rgb(242, 63, 67)),
        );
    }
    response
}
impl NotDiscordUi {
    fn presence_line(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        status: &str,
        emoji: Option<&StatusEmoji>,
        activities: &[PresenceActivity],
        activity_fallback: &str,
        presence_fallback: &str,
    ) {
        let custom = has_status(status, emoji);
        let mut badges: Vec<(bool, String)> = Vec::new();
        for current in activities.iter().filter(|current| current.kind != 4) {
            let music = current.kind == 2;
            let details = activity(current);
            if let Some((_, text)) = badges.iter_mut().find(|(kind, _)| *kind == music) {
                text.push('\n');
                text.push_str(&details);
            } else {
                badges.push((music, details));
            }
        }
        if badges.is_empty() && !activity_fallback.is_empty() {
            badges.push((
                activity_fallback.starts_with('♫') || activity_fallback.contains("Spotify"),
                activity_fallback.to_string(),
            ));
        }
        let detail = if custom {
            if status.trim().is_empty() {
                presence_fallback.to_string()
            } else {
                status.to_string()
            }
        } else {
            badges
                .first()
                .map(|(_, text)| text.lines().next().unwrap_or("").to_string())
                .unwrap_or_else(|| presence_fallback.to_string())
        };
        let mut left = rect.left();
        if let Some(emoji) = emoji {
            let picture = egui::Rect::from_center_size(
                egui::pos2(left + 7., rect.center().y),
                egui::vec2(14., 14.),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(picture)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    self.emoji(ui, emoji, 14.);
                },
            );
            left += 18.;
        }
        for (index, (music, tooltip)) in badges.iter().take(if custom { 2 } else { 1 }).enumerate()
        {
            let icon = egui::Rect::from_center_size(
                egui::pos2(left + 7., rect.center().y),
                egui::vec2(14., 14.),
            );
            paint_activity(ui, icon, *music);
            ui.interact(
                icon,
                ui.id().with(("activity", index, rect.top().to_bits())),
                egui::Sense::hover(),
            )
            .on_hover_text(tooltip);
            left += 18.;
        }
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(left.min(rect.right()), rect.top()),
            rect.right_bottom(),
        );
        fitted_text(ui, text_rect, &detail, 12., self.theme.muted);
    }
    pub(super) fn draw_shell(&mut self, ctx: &egui::Context, state: &AppSnapshot) {
        let title = if state.selected_guild_id.is_empty() {
            crate::brand::NAME.to_string()
        } else {
            state
                .guilds
                .iter()
                .find(|g| g.id == state.selected_guild_id)
                .map(|g| g.name.clone())
                .map(|name|format!("{} · {}",crate::brand::NAME,name))
                .unwrap_or_else(|| crate::brand::NAME.into())
        };
        let window_title=format!("{} — {}",title,state.channel_name);
        if self.window_title != window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(window_title.clone()));
            self.window_title = window_title;
        }
        egui::TopBottomPanel::top("window-chrome")
            .exact_height(32.)
            .frame(
                egui::Frame::new()
                    .fill(self.theme.base)
                    .inner_margin(egui::Margin::symmetric(8, 0)),
            )
            .show(ctx, |ui| {
                if self.background_blocked() { ui.disable(); }
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("●").color(self.theme.accent))
                        .on_hover_text(format!("{} · {}", state.connection, state.status));
                    ui.label(
                        RichText::new(format!("{} — {}", title, state.channel_name)).size(12.),
                    );
                    let drag = ui.allocate_response(
                        egui::vec2((ui.available_width() - 110.).max(0.), 26.),
                        egui::Sense::drag(),
                    );
                    if drag.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag)
                    }
                    if drag.double_clicked() {
                        let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                    }
                    if ui.button("—").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                    if ui.button("□").clicked() {
                        let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                    }
                    if ui.button("×").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        let header_split = ctx
            .data(|data| data.get_temp::<f32>(egui::Id::new("shell-header-split")))
            .unwrap_or(378.);
        let header = egui::TopBottomPanel::top("header")
            .exact_height(54.)
            .frame(
                egui::Frame::new()
                    .fill(self.theme.panel)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ctx, |ui| {
                if self.background_blocked() { ui.disable(); }
                ui.horizontal_centered(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(
                            (header_split - ui.cursor().left() - ui.spacing().item_spacing.x)
                                .max(200.),
                            38.,
                        ),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 10.;
                            self.logo(ui, 27.);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 38.),
                                egui::Sense::hover(),
                            );
                            let mut job = egui::text::LayoutJob::simple(
                                title.clone(),
                                egui::FontId::proportional(16.),
                                self.theme.ink,
                                rect.width(),
                            );
                            job.wrap.max_rows = 1;
                            job.wrap.break_anywhere = true;
                            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                            let position =
                                rect.left_center() - egui::vec2(0., galley.size().y / 2.);
                            ui.painter().with_clip_rect(rect).galley(
                                position,
                                galley,
                                self.theme.ink,
                            );
                        },
                    );
                    ui.add_space(18.);
                    self.draw_header(ui, state);
                });
            });
        if state.network_safety != notdiscord_core::safety::SafetyState::Allowed {
            egui::TopBottomPanel::top("network-safety-banner")
                .frame(
                    egui::Frame::new()
                        .fill(self.theme.panel)
                        .stroke(egui::Stroke::new(1.0_f32, self.theme.line))
                        .inner_margin(egui::Margin::symmetric(12, 8)),
                )
                .show(ctx, |ui| {
                if self.background_blocked() { ui.disable(); }
                    ui.horizontal_wrapped(|ui| {
                        ui.label(&state.network_safety_message);
                        if ui.button("Abrir Discord").clicked() {
                            self.simple("open-security-help");
                        }
                        if state.network_safety
                            == notdiscord_core::safety::SafetyState::AuthenticationRequired
                            && ui.button("Iniciar sesión").clicked()
                        {
                            self.simple("login");
                        }
                        if state.network_safety
                            == notdiscord_core::safety::SafetyState::SynchronizationRequired
                            && ui.button("Volver a conectar").clicked()
                        {
                            self.simple("login");
                        }
                    });
                });
        }
        if !self.error.is_empty() {
            egui::TopBottomPanel::bottom("status-bar")
                .exact_height(26.)
                .frame(
                    egui::Frame::new()
                        .fill(self.theme.panel)
                        .stroke(egui::Stroke::new(1.0_f32, self.theme.line))
                        .inner_margin(egui::Margin::symmetric(12, 3)),
                )
                .show(ctx, |ui| {
                if self.background_blocked() { ui.disable(); }
                    let width = ui.available_width();
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 20.), egui::Sense::hover());
                    let close = egui::Rect::from_min_max(
                        egui::pos2(rect.right() - 22., rect.top()),
                        rect.right_bottom(),
                    );
                    fitted_text(
                        ui,
                        egui::Rect::from_min_max(
                            rect.left_top(),
                            egui::pos2(close.left() - 5., rect.bottom()),
                        ),
                        &self.error,
                        12.,
                        self.theme.muted,
                    );
                    ui.scope_builder(egui::UiBuilder::new().max_rect(close), |ui| {
                        if ui.add_sized([22., 20.], egui::Button::new("×")).clicked() {
                            self.error.clear();
                        }
                    });
                });
        }
        let rail = egui::SidePanel::left("server-rail")
            .exact_width(86.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(self.theme.rail)
                    .inner_margin(egui::Margin::symmetric(8, 8)),
            )
            .show(ctx, |ui| {
                if self.background_blocked() { ui.disable(); }
                let footer = left_footer_height(state, ctx.content_rect().height() < 420.);
                let available = ui.available_rect_before_wrap();
                let reserved = egui::Rect::from_min_max(available.min, egui::pos2(available.right(), (available.bottom() - footer).max(available.top())));
                let mut content = ui.new_child(egui::UiBuilder::new().id_salt("rail-content").max_rect(reserved));
                let ui = &mut content;
                ui.set_clip_rect(ui.clip_rect().intersect(reserved));
                #[cfg(test)] ctx.data_mut(|data| data.insert_temp(egui::Id::new("rail-reserved-rect"), ui.clip_rect()));
                // Both nested scroll areas share the rail's reserved gutter.
                // Floating bars keep either one from reserving another gutter;
                // the 56-point tiles end before the 8-point scrollbar track.
                ui.spacing_mut().scroll.floating = true;
                ui.spacing_mut().scroll.floating_allocated_width = 0.;
                theme::vertical_scroll(ui)
                    .id_salt("server-scroll")
                    .auto_shrink([false, true])
                    .max_height(ui.available_height().max(0.))
                    .show(ui, |ui| {
                        let (home_rect, home) =
                            ui.allocate_exact_size(egui::Vec2::splat(56.), egui::Sense::click());
                        let home_hover = ui.rect_contains_pointer(home_rect);
                        if home_hover || state.selected_guild_id.is_empty() {
                            ui.painter().rect_filled(
                                home_rect,
                                5.,
                                if home_hover {
                                    self.theme.surface
                                } else {
                                    self.theme.selected
                                },
                            );
                        }
                        let home_logo = ui
                            .scope_builder(
                                egui::UiBuilder::new()
                                    .max_rect(home_rect.shrink(4.))
                                    .layout(Layout::left_to_right(Align::Center)),
                                |ui| self.logo(ui, 48.),
                            )
                            .inner;
                        if home
                            .union(home_logo)
                            .on_hover_text("Inicio · mensajes y amigos")
                            .clicked()
                        {
                            self.send("guild", "", "", "", "");
                        }
                        ui.advance_cursor_after_rect(home_rect);
                        ui.add_space(8.);
                        let mut call_ids = HashSet::new();
                        for item in state.direct_messages.iter().filter(|d| d.has_active_call) {
                            call_ids.insert(item.id.clone());
                            let r = self.rail_item(
                                ui,
                                item,
                                48.,
                                item.id == state.selected_channel_id,
                                true,
                                item.mention_count,
                            );
                            if r.clicked() {
                                self.navigate(item);
                            }
                            r.context_menu(|ui| self.nav_menu(ui, item, state));
                        }
                        let mut attention_seen = call_ids.clone();
                        for call in state
                            .incoming_calls
                            .iter()
                            .filter(|c| c.kind != "guild" && !call_ids.contains(&c.channel_id))
                        {
                            if !attention_seen.insert(call.channel_id.clone()) {
                                continue;
                            }
                            let nav = NavItem {
                                id: call.channel_id.clone(),
                                name: call.channel_name.clone(),
                                kind: call.kind.clone(),
                                icon_url: call.icon_url.clone(),
                                recipient_avatars: call
                                    .participants
                                    .iter()
                                    .filter_map(|p| p.avatar_url.clone())
                                    .take(4)
                                    .collect(),
                                ..Default::default()
                            };
                            let r = self.rail_item(
                                ui,
                                &nav,
                                48.,
                                call.channel_id == state.selected_channel_id,
                                true,
                                1,
                            );
                            if r.clicked() {
                                self.send("channel", &call.channel_id, "", "", "");
                            }
                        }
                        for item in state
                            .direct_messages
                            .iter()
                            .filter(|d| d.unread && !attention_seen.contains(&d.id))
                        {
                            let r = self.rail_item(
                                ui,
                                item,
                                48.,
                                item.id == state.selected_channel_id,
                                false,
                                item.mention_count.max(1),
                            );
                            if r.clicked() {
                                self.navigate(item);
                            }
                            r.context_menu(|ui| self.nav_menu(ui, item, state));
                        }
                        ui.separator();
                        theme::vertical_scroll(ui)
                            .id_salt("guilds")
                            .auto_shrink([false, true])
                            .show_rows(ui, 56., state.guilds.len(), |ui, range| {
                                for guild in &state.guilds[range] {
                                    let r = self.rail_item(
                                        ui,
                                        guild,
                                        48.,
                                        guild.id == state.selected_guild_id,
                                        false,
                                        if guild.unread { 1 } else { 0 },
                                    );
                                    if r.clicked() {
                                        self.send("guild", &guild.id, "", "", "");
                                    }
                                    r.context_menu(|ui| self.nav_menu(ui, guild, state));
                                }
                            });
                    });
            });
        let navigation = egui::SidePanel::left("conversation-sidebar")
            .exact_width(292.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(self.theme.panel)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                if self.background_blocked() { ui.disable(); }
                let compact_footer = ctx.content_rect().height() < 420.;
                let footer = left_footer_height(state, compact_footer);
                let available = ui.available_rect_before_wrap();
                let reserved = egui::Rect::from_min_max(available.min, egui::pos2(available.right(), (available.bottom() - footer).max(available.top())));
                let mut content = ui.new_child(egui::UiBuilder::new().id_salt("conversation-content").max_rect(reserved));
                let ui = &mut content;
                ui.set_clip_rect(ui.clip_rect().intersect(reserved));
                #[cfg(test)] ctx.data_mut(|data| data.insert_temp(egui::Id::new("navigation-reserved-rect"), ui.clip_rect()));
                if state.selected_guild_id.is_empty() {
                    for (id, label) in [("friends", "☺  Amigos"), ("requests", "↗  Solicitudes")]
                    {
                        if ui
                            .add_sized(
                                [ui.available_width(), if compact_footer { 26. } else { 40. }],
                                egui::Button::new(label).fill(if state.view == id {
                                    self.theme.selected
                                } else {
                                    self.theme.surface
                                }),
                            )
                            .clicked()
                        {
                            self.send("view", id, "", "", "");
                        }
                    }
                    if ui
                        .add_sized(
                            [ui.available_width(), 30.],
                            egui::Button::new("+ Añadir amigo"),
                        )
                        .clicked()
                    {
                        self.prompt_for(
                            "Añadir amigo",
                            "Nombre de usuario de Discord (sin @)",
                            "add-friend",
                            "",
                            "",
                            "",
                            false,
                        );
                    }
                    ui.add_space(3.);
                } else {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 30.),
                        egui::Sense::hover(),
                    );
                    fitted_text(
                        ui,
                        rect.shrink2(egui::vec2(5., 0.)),
                        "CANALES",
                        12.,
                        self.theme.muted,
                    );
                }
                let rows = navigation_rows(state);
                let collapsed=ui.ctx().data(|data|data.get_temp::<HashSet<String>>(category_key(state))).unwrap_or_default();
                let projection=list_projection::navigation(ui.ctx(),state,collapsed);
                let reserve = 36.;
                theme::vertical_scroll(ui)
                    .id_salt(("channels", &state.selected_guild_id))
                    .max_height((ui.available_height() - reserve).max(0.))
                    .show_viewport(ui, |ui, viewport| {
                        let mut content_y = 0.;
                        for row in &rows {
                            match row {
                                NavigationRow::Section(label) => {
                                    let estimate = 35.;
                                    if content_y + estimate >= viewport.min.y - 80.
                                        && content_y <= viewport.max.y + 80.
                                    {
                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(ui.available_width(), 30.),
                                            egui::Sense::hover(),
                                        );
                                        fitted_text(
                                            ui,
                                            egui::Rect::from_min_max(
                                                rect.left_top() + egui::vec2(5., 10.),
                                                rect.right_bottom(),
                                            ),
                                            label,
                                            12.,
                                            self.theme.muted,
                                        );
                                    } else {
                                        ui.add_space(estimate);
                                    }
                                    content_y += estimate;
                                }
                                NavigationRow::Item(item) => {
                                    if projection.hidden.contains(&item.id) { continue; }
                                    let (flags, base) = nav_metrics(item);
                                    let voices=projection.voice.get(&item.id).filter(|_|matches!(item.kind.as_str(),"voice"|"stage"));
                                    let count=voices.map(|voices|voices.rows.len()).unwrap_or(0);
                                    let games=voices.map(|voices|voices.games).unwrap_or(0);
                                    let flags = u64::from(flags) | ((count as u64) << 8) | ((games as u64)<<32);
                                    let base=base+voices.map(|voices|voices.height+count as f32*ui.spacing().item_spacing.y).unwrap_or(0.);
                                    let estimate = self
                                        .nav_heights
                                        .get(&item.id)
                                        .filter(|(old, _)| *old == flags)
                                        .map(|(_, height)| *height)
                                        .unwrap_or(base + ui.spacing().item_spacing.y);
                                    if content_y + estimate >= viewport.min.y - 80.
                                        && content_y <= viewport.max.y + 80.
                                    {
                                        let start = ui.cursor().top();
                                        self.nav_row(ui, item, state);
                                        if let Some(voices)=voices { self.draw_guild_voice_members(ui, item, state, &voices.rows); }
                                        let height = (ui.cursor().top() - start).max(18.);
                                        self.nav_heights.insert(item.id.clone(), (flags, height));
                                        content_y += height;
                                    } else {
                                        ui.add_space(estimate);
                                        content_y += estimate;
                                    }
                                }
                            }
                        }
                    });
                if self.nav_heights.len() > 2048 {
                    let valid = rows
                        .iter()
                        .filter_map(|row| match row {
                            NavigationRow::Item(item) => Some(item.id.as_str()),
                            _ => None,
                        })
                        .collect::<HashSet<_>>();
                    self.nav_heights.retain(|id, _| valid.contains(id.as_str()));
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    if ui
                        .add_sized(
                            [ui.available_width(), 28.],
                            egui::Button::new("↳ Unirse con invitación"),
                        )
                        .clicked()
                    {
                        self.prompt_for(
                            "Unirse con invitación",
                            "Enlace o código de invitación",
                            "invite-preview",
                            "",
                            "",
                            "",
                            false,
                        );
                    }
                });
            });
        let mut members_left = None;
        let dm_sidebar = state.selected_guild_id.is_empty() && state.selected_channel_kind == "dm";
        let sidebar_visible = if dm_sidebar {
            self.prefs.show_dm_profile
        } else if state.selected_guild_id.is_empty() {
            self.prefs.show_group_members
        } else {
            self.prefs.show_server_members
        };
        if sidebar_visible
            && ctx.content_rect().width() - 364. >= 560.
            && state.view == "chat"
            && (dm_sidebar
                || !state.group_members.is_empty()
                || !state.guild_members.is_empty()
                || state.selected_channel_kind == "group"
                || !state.selected_guild_id.is_empty()
                    && state.channel_access.known
                    && state.channel_access.can_view
                    && state.channel_access.can_view_members)
        {
            let members_panel = egui::SidePanel::right("members-sidebar")
                .exact_width(222.)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(self.theme.panel)
                        .inner_margin(egui::Margin::same(16)),
                )
                .show(ctx, |ui| {
                if self.background_blocked() { ui.disable(); }
                    if dm_sidebar {
                        self.draw_dm_sidebar(ui, state);
                    } else {
                        self.draw_members(ui, state);
                    }
                });
            members_left = Some(members_panel.response.rect.left());
            member_demand::drawn(ctx, false);
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(self.theme.base)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                if self.background_blocked() { ui.disable(); }
                match state.view.as_str() {
                "chat" | "pins" | "search" | "history" => self.draw_chat(ui, state),
                "profile" => self.draw_profile(ui, state),
                "guild-settings" => self.draw_guild_settings(ui, state),
                "friends" | "requests" => self.draw_people(ui, state),
                _ => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.);
                        self.logo(ui, 64.);
                        ui.heading(crate::brand::NAME);
                        ui.label("Tus conversaciones, con una interfaz nativa.");
                        if ui.button("Iniciar sesión en Discord").clicked() {
                            self.simple("login");
                        }
                    });
                }
                }
            });
        let split = navigation.response.rect.right();
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("shell-header-split"), split));
        if (split - header_split).abs() > 0.1 {
            ctx.request_repaint();
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("shell-dividers"),
        ));
        let stroke = egui::Stroke::new(1.0_f32, self.theme.line);
        let top = header.response.rect.top();
        let body_top = header.response.rect.bottom();
        let bottom = navigation.response.rect.bottom();
        let viewport = ctx.content_rect();
        for y in [top, body_top] {
            painter.line_segment(
                [
                    egui::pos2(viewport.left(), y),
                    egui::pos2(viewport.right(), y),
                ],
                stroke,
            );
        }
        painter.line_segment([egui::pos2(split, top), egui::pos2(split, bottom)], stroke);
        painter.line_segment(
            [
                egui::pos2(rail.response.rect.right(), body_top),
                egui::pos2(rail.response.rect.right(), bottom),
            ],
            stroke,
        );
        if let Some(left) = members_left {
            painter.line_segment(
                [egui::pos2(left, body_top), egui::pos2(left, bottom)],
                stroke,
            );
        }
        self.draw_account(ctx, state);
    }
    fn logo(&mut self, ui: &mut egui::Ui, size: f32) -> egui::Response {
        if let Some(texture) = &self.logo_texture {
            return ui.add(
                egui::Image::new(texture)
                    .fit_to_exact_size(Vec2::splat(size))
                    .sense(egui::Sense::click()),
            );
        }
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
        let p = ui.painter();
        p.circle_filled(rect.center(), size * 0.48, self.theme.accent);
        let r = rect.shrink(size * 0.24);
        let pts = vec![
            egui::pos2(r.left(), r.bottom()),
            egui::pos2(r.left() + r.width() * 0.10, r.top() + r.height() * 0.18),
            egui::pos2(r.left() + r.width() * 0.30, r.top()),
            egui::pos2(r.left() + r.width() * 0.70, r.top()),
            egui::pos2(r.right() - r.width() * 0.10, r.top() + r.height() * 0.18),
            egui::pos2(r.right(), r.bottom()),
            egui::pos2(r.right() - r.width() * 0.25, r.bottom() + r.height() * 0.10),
            egui::pos2(r.left() + r.width() * 0.25, r.bottom() + r.height() * 0.10),
        ];
        p.add(egui::Shape::convex_polygon(
            pts,
            Color32::WHITE,
            egui::Stroke::NONE,
        ));
        p.circle_filled(
            egui::pos2(r.left() + r.width() * 0.30, r.center().y),
            size * 0.055,
            self.theme.accent,
        );
        p.circle_filled(
            egui::pos2(r.left() + r.width() * 0.70, r.center().y),
            size * 0.055,
            self.theme.accent,
        );
        response
    }
    fn rail_item(
        &mut self,
        ui: &mut egui::Ui,
        item: &NavItem,
        size: f32,
        selected: bool,
        call: bool,
        count: i32,
    ) -> egui::Response {
        let (rect, response) =
            ui.allocate_exact_size(egui::Vec2::splat(size + 8.), egui::Sense::click());
        let hovered = ui.rect_contains_pointer(rect);
        if selected || hovered {
            ui.painter().rect_filled(
                rect,
                5.,
                if hovered {
                    self.theme.surface
                } else {
                    self.theme.selected
                },
            );
        }
        if selected {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(rect.left() - 8., rect.top() + 8.),
                    egui::vec2(4., rect.height() - 16.),
                ),
                3.,
                self.theme.ink,
            );
        }
        let picture = rect.shrink(4.);
        let avatar = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(picture)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| self.nav_avatar(ui, item, size),
            )
            .inner;
        if item.unread && item.kind == "guild" {
            ui.painter().rect_stroke(
                picture,
                24.,
                egui::Stroke::new(2.0_f32, self.theme.accent),
                egui::StrokeKind::Inside,
            );
        }
        if count > 0 {
            let center = picture.right_bottom() - egui::vec2(3., 3.);
            ui.painter().circle_filled(center, 12., self.theme.rail);
            ui.painter()
                .circle_filled(center, 10., Color32::from_rgb(218, 55, 66));
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                if count > 99 {
                    "99+".to_string()
                } else {
                    count.to_string()
                },
                egui::FontId::proportional(11.),
                Color32::WHITE,
            );
        }
        if call {
            ui.painter().circle_filled(
                picture.right_top() + egui::vec2(-4., 4.),
                9.,
                presence_color("online"),
            );
        }
        ui.advance_cursor_after_rect(rect);
        response.union(avatar).on_hover_text(format!(
            "{}{}{}",
            item.name,
            if item.unread {
                " · mensajes sin leer"
            } else {
                ""
            },
            if call { " · llamada en curso" } else { "" }
        ))
    }
    fn draw_header(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        let available = ui.available_width();
        let show_search = !state.selected_channel_id.is_empty()
            && matches!(state.view.as_str(), "chat" | "pins" | "search" | "history");
        let size = if available < 480. { 26. } else { 36. };
        let guild_voice = !state.selected_guild_id.is_empty()
            && state.selected_channel_kind == "voice";
        let calls = (state.selected_guild_id.is_empty() || guild_voice)
            && !state.selected_channel_id.is_empty()
            && state.view == "chat";
        let pins =
            !state.selected_channel_id.is_empty() && matches!(state.view.as_str(), "chat" | "pins");
        let add = state.selected_channel_kind == "group" && state.view == "chat";
        let members = (state.selected_channel_kind == "dm"
            || state.selected_channel_kind == "group"
            || !state.selected_guild_id.is_empty() && state.channel_access.can_view_members)
            && state.view == "chat";
        let buttons =
            usize::from(calls) * (if guild_voice { 1 } else { 2 })
                + usize::from(pins) + usize::from(add) + usize::from(members);
        let voice_label_extra = 0.;
        let search_width = if show_search {
            (available - voice_label_extra - buttons as f32 * (size + 3.) - 100.).clamp(100., 220.)
        } else {
            0.
        };
        let actions_width = search_width + voice_label_extra
            + if buttons > 0 {
                10. + buttons as f32 * size + (buttons - 1) as f32 * 3.
            } else {
                0.
            };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(available, 38.), egui::Sense::hover());
        let actions_left = (rect.right() - actions_width).max(rect.left());
        let mut text_left = rect.left();
        if state.selected_guild_id.is_empty() && state.view == "chat" {
            if let Some(item) = state
                .direct_messages
                .iter()
                .find(|item| item.id == state.selected_channel_id)
            {
                let picture = egui::Rect::from_center_size(
                    egui::pos2(text_left + 16., rect.center().y),
                    egui::vec2(32., 32.),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(picture)
                        .layout(Layout::left_to_right(Align::Center)),
                    |ui| {
                        self.nav_avatar(ui, item, 32.);
                    },
                );
                text_left += 42.;
            }
        }
        let heading = match state.view.as_str() {
            "friends" => "Amigos".to_string(),
            "requests" => "Solicitudes".to_string(),
            "profile" => "Perfil".to_string(),
            "guild-settings" => "Configuración del servidor".to_string(),
            "search" => "Resultados de búsqueda".to_string(),
            "pins" => "Mensajes fijados".to_string(),
            "history" => "Registro de esta sesión (RAM)".to_string(),
            _ => format!(
                "{}{}",
                if !state.selected_guild_id.is_empty() && !state.selected_channel_id.is_empty() {
                    "# "
                } else {
                    ""
                },
                state.channel_name
            ),
        };
        fitted_text(
            ui,
            egui::Rect::from_min_max(
                egui::pos2(text_left.min(actions_left), rect.top()),
                egui::pos2(
                    (actions_left - 12.).max(text_left.min(actions_left)),
                    rect.bottom(),
                ),
            ),
            &heading,
            17.,
            self.theme.ink,
        );
        if !show_search {
            self.search_open = false;
            self.search_focused = false;
            self.search_rect = egui::Rect::NOTHING;
            ui.advance_cursor_after_rect(rect);
            return;
        }
        let actions_rect =
            egui::Rect::from_min_max(egui::pos2(actions_left, rect.top()), rect.right_bottom());
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(actions_rect)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 3.;
                let active = state.signed_in && !state.busy;
                if calls {
                    let in_selected_call = state.call_channel_id == state.selected_channel_id
                        && (state.voice_connected || state.voice_connecting);
                    let call_busy = in_selected_call || mic_test_blocks_calls(ui.ctx());
                    let label = if in_selected_call && state.voice_connected { "Conectado a voz" }
                        else if in_selected_call { "Conectando a voz…" }
                        else if call_busy { "Detén la prueba del micrófono para conectar" }
                        else if state.voice_connected || state.voice_connecting { "Cambiar a este canal de voz" } else { "Conectar a voz" };
                    let call_allowed = active && state.voice_available && !mic_test_blocks_calls(ui.ctx()) && (!guild_voice || !in_selected_call);
                    let call = ui.add_enabled_ui(call_allowed, |ui| {
                        icon_button(ui, Icon::Phone, size, false, false)
                    }).inner.on_hover_text(if guild_voice { label } else { "Iniciar llamada de voz" });
                    call.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true,
                        if guild_voice { label } else { "Iniciar llamada de voz" }));
                    if call.clicked() && call_allowed {
                        self.send("voice", "", "", "", &state.selected_channel_id);
                    }
                    if !guild_voice {
                    let camera = icon_button(ui, Icon::Video, size, !active, false)
                        .on_hover_text("Iniciar videollamada");
                    if camera.clicked() && active && state.voice_available {
                        self.send("video-call", "", "", "", &state.selected_channel_id);
                    }
                    }
                }
                if pins
                    && icon_button(ui, Icon::Pin, size, !active, false)
                        .on_hover_text("Mensajes fijados")
                        .clicked()
                    && active
                {
                    self.send("pins", "", "", "", &state.selected_channel_id);
                }
                if add
                    && icon_button(ui, Icon::AddMember, size, !active, false)
                        .on_hover_text("Añadir personas al grupo")
                        .clicked()
                {
                    self.dialog_data = Value::Null;
                    self.prompt_for(
                        "Añadir persona al grupo",
                        "Selecciona una amistad",
                        "group-add",
                        "",
                        "",
                        "",
                        false,
                    );
                }
                if members
                    && icon_button(ui, Icon::Members, size, !active, false)
                        .on_hover_text(if state.selected_channel_kind == "dm" {
                            "Mostrar perfil de la persona"
                        } else {
                            "Mostrar miembros"
                        })
                        .clicked()
                {
                    let narrow = ui.ctx().content_rect().width() - 364. < 560.;
                    if state.selected_guild_id.is_empty() && state.selected_channel_kind == "dm" {
                        if narrow {
                            self.prefs.show_dm_profile = true;
                            self.dialog = Some(Dialog::DmProfile);
                        } else {
                            self.prefs.show_dm_profile = !self.prefs.show_dm_profile;
                        }
                    } else if state.selected_guild_id.is_empty() {
                        if narrow {
                            self.prefs.show_group_members = true;
                            self.dialog = Some(Dialog::Members);
                        } else {
                            self.prefs.show_group_members = !self.prefs.show_group_members;
                        }
                        if self.prefs.show_group_members {
                            self.simple("group-members");
                        }
                    } else {
                        if narrow {
                            self.prefs.show_server_members = true;
                            self.dialog = Some(Dialog::Members);
                        } else {
                            self.prefs.show_server_members = !self.prefs.show_server_members;
                        }
                    }
                    if let Err(error) = self.prefs.save(&self.data_dir) {
                        self.error = error;
                    }
                }
                if buttons > 0 {
                    ui.add_space(7.);
                }
                let search_ime=ui.is_enabled()&&chat::composer_ime_active(ui.ctx());
                let response = ui.add_sized(
                    [search_width, 32.],
                    bordered(
                        egui::TextEdit::singleline(&mut self.search)
                            .id(egui::Id::new("message-search"))
                            .hint_text("Buscar mensajes  ⌕"),
                    ),
                );
                self.search_rect = response.rect;
                if ui.is_enabled() {
                if response.has_focus() {
                    let frame=ui.ctx().cumulative_frame_nr();
                    ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("search-header-input-frame"),frame));
                }
                let cursor = egui::TextEdit::load_state(ui.ctx(), response.id)
                    .and_then(|state| state.cursor.char_range())
                    .map(|range| range.primary.index)
                    .unwrap_or_else(|| self.search.chars().count());
                if !search_ime && cursor != self.search_cursor && response.has_focus() {
                    self.search_open = true;
                }
                self.search_cursor = cursor;
                self.search_focused = response.has_focus();
                if !search_ime && (response.has_focus() || response.clicked() || response.changed()) {
                    self.search_open = true;
                }
                if !search_ime && ui.is_enabled() && (response.has_focus() || response.lost_focus())
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    self.send(
                        "search",
                        "",
                        self.search.clone(),
                        "",
                        &state.selected_channel_id,
                    );
                    self.search_open = false;
                }
                }
            },
        );
        ui.advance_cursor_after_rect(rect);
    }
    fn nav_row(&mut self, ui: &mut egui::Ui, item: &NavItem, state: &AppSnapshot) {
        self.emoji_section(ui, ("navigation-row-emojis", &item.id), |this, ui| this.nav_row_inner(ui, item, state));
    }
    fn nav_row_inner(&mut self, ui: &mut egui::Ui, item: &NavItem, state: &AppSnapshot) {
        if matches!(item.kind.as_str(), "category" | "section") {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.),
                if item.kind == "category" { egui::Sense::click() } else { egui::Sense::hover() });
            let response=if item.kind=="category" {response.on_hover_cursor(egui::CursorIcon::PointingHand)} else {response};
            if item.kind == "category" {
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &item.name));
                if response.clicked() {
                    ui.ctx().data_mut(|data| {
                        let values = data.get_temp_mut_or_default::<HashSet<String>>(category_key(state));
                        if !values.remove(&item.id) && values.len() < 2000 { values.insert(item.id.clone()); }
                    });
                    ui.ctx().request_repaint();
                }
            }
            let collapsed = ui.ctx().data(|data| data.get_temp::<HashSet<String>>(category_key(state))).unwrap_or_default().contains(&item.id);
            let label = format!(
                "{}{}",
                if item.kind == "category" { if collapsed { "▸ " } else { "▾ " } } else { "" },
                item.name
            );
            fitted_text(
                ui,
                egui::Rect::from_min_max(
                    rect.left_top() + egui::vec2(5., 10.),
                    rect.right_bottom(),
                ),
                &label,
                12.,
                self.theme.muted,
            );
            return;
        }
        let private = matches!(item.kind.as_str(), "dm" | "group");
        let (_, height) = nav_metrics(item);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::click(),
        );
        let response=response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let selected = item.id == state.selected_channel_id;
        let hovered = ui.rect_contains_pointer(rect);
        if selected || hovered {
            ui.painter().rect_filled(
                rect,
                5.,
                if hovered {
                    self.theme.surface
                } else {
                    self.theme.selected
                },
            );
        }
        let mut left = rect.left() + 8.;
        let mut right = rect.right() - 8.;
        let mut voice_join = None;
        if item.kind == "voice" {
            let connected = state.call_channel_id == item.id && state.voice_connected;
            let busy = (state.call_channel_id == item.id && (state.voice_connecting || state.voice_connected))
                || mic_test_blocks_calls(ui.ctx());
            let button_rect = egui::Rect::from_center_size(
                egui::pos2(right - 14., rect.center().y), egui::vec2(28., 28.));
            let join = ui.scope_builder(egui::UiBuilder::new().max_rect(button_rect), |ui| {
                ui.add_enabled_ui(state.signed_in && state.voice_available && !busy, |ui| icon_button(ui, Icon::Phone, 28., false, false)).inner
            }).inner;
            join.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, state.signed_in&&state.voice_available&&!busy,
                if connected { "Conectado a voz" } else { "Conectar al audio del canal" }));
            let clicked = join.clicked() && state.signed_in && state.voice_available && !busy;
            join.on_hover_text(if connected {"Ya estás conectado. Selecciona el nombre para abrir el chat."}
                else if busy {"La conexión ya está en curso o la prueba del micrófono está activa."}
                else if state.voice_connected || state.voice_connecting {"Cambiar el audio a este canal."}
                else {"Conectar al audio de este canal. El nombre abre solamente su chat."});
            #[cfg(test)] ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new(("guild-voice-join",&item.id)),button_rect));
            voice_join = Some((button_rect, clicked));
            right -= 34.;
        }
        let mut avatar = None;
        if private {
            let size = if self.prefs.compact { 34. } else { 40. };
            let picture = egui::Rect::from_center_size(
                egui::pos2(left + size / 2., rect.center().y),
                egui::Vec2::splat(size),
            );
            avatar = Some(
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(picture)
                        .layout(Layout::left_to_right(Align::Center)),
                    |ui| self.nav_avatar(ui, item, size),
                )
                .inner,
            );
            if !item.presence.is_empty() {
                let center = picture.right_bottom() - egui::vec2(4., 4.);
                ui.painter().circle_filled(center, 7., self.theme.panel);
                ui.painter()
                    .circle_filled(center, 5., presence_color(&item.presence));
            }
            left += size + 11.;
        } else {
            let symbol = egui::Rect::from_center_size(
                egui::pos2(left + 10., rect.center().y),
                egui::vec2(22., 22.),
            );
            fitted_text(
                ui,
                symbol,
                if matches!(item.kind.as_str(), "voice" | "stage") {
                    "◉"
                } else {
                    "#"
                },
                20.,
                self.theme.muted,
            );
            left += 30.;
        }
        if item.mention_count > 0 {
            let label = if item.mention_count > 99 {
                "99+".to_string()
            } else {
                item.mention_count.to_string()
            };
            let width = if item.mention_count > 99 { 27. } else { 21. };
            let badge = egui::Rect::from_center_size(
                egui::pos2(right - width / 2., rect.center().y),
                egui::vec2(width, 19.),
            );
            ui.painter()
                .rect_filled(badge, 9., self.theme.accent_surface);
            ui.painter().text(
                badge.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.),
                Color32::WHITE,
            );
            right -= width + 5.;
        } else if item.unread {
            ui.painter().circle_filled(
                egui::pos2(right - 4., rect.center().y),
                3.5,
                self.theme.ink,
            );
            right -= 12.;
        }
        if private && item.is_pinned {
            let pin = egui::Rect::from_center_size(
                egui::pos2(right - 7.5, rect.center().y),
                egui::vec2(15., 15.),
            );
            super::icons::paint_pin(ui.painter(), pin, self.theme.muted);
            right -= 20.;
        }
        let text_color = if item.is_muted || item.notifications_disabled {
            self.theme.muted
        } else if item.kind == "voice" && item.id == state.call_channel_id && state.voice_connected {
            Color32::from_rgb(35, 165, 90)
        } else {
            self.theme.ink
        };
        let width = (right - left).max(0.);
        if private {
            let fallback = if !item.subtitle.is_empty() {
                item.subtitle.as_str()
            } else if !item.presence.is_empty() {
                presence_label(&item.presence)
            } else {
                ""
            };
            let secondary = has_status(&item.custom_status, item.custom_status_emoji.as_ref())
                || !item.activities.is_empty()
                || !item.activity_text.is_empty()
                || !fallback.is_empty();
            let text_height = 20.
                + if secondary { 18. } else { 0. }
                + if item.has_active_call { 16. } else { 0. };
            let mut top = rect.center().y - text_height / 2.;
            fitted_text(
                ui,
                egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, 20.)),
                &item.name,
                16.,
                text_color,
            );
            top += 20.;
            if item.has_active_call {
                let call_rect = egui::Rect::from_min_size(
                    egui::pos2(left, top + 1.), egui::vec2(13., 13.),
                );
                super::icons::paint_call(ui.painter(), call_rect, self.theme.accent);
                fitted_text(
                    ui,
                    egui::Rect::from_min_size(egui::pos2(left + 17., top), egui::vec2((width - 17.).max(0.), 16.)),
                    if item.incoming_call_ringing {
                        "Llamada entrante"
                    } else {
                        "Llamada en curso"
                    },
                    12.,
                    self.theme.accent,
                );
                top += 16.;
            }
            if secondary {
                self.presence_line(
                    ui,
                    egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, 18.)),
                    &item.custom_status,
                    item.custom_status_emoji.as_ref(),
                    &item.activities,
                    &item.activity_text,
                    fallback,
                );
            }
        } else {
            fitted_text(
                ui,
                egui::Rect::from_min_max(
                    egui::pos2(left, rect.top()),
                    egui::pos2(right.max(left), rect.bottom()),
                ),
                &item.name,
                16.,
                text_color,
            );
        }
        if voice_join.as_ref().is_some_and(|(_,clicked)|*clicked) {
            self.navigate(item);
            self.send("voice", &item.id, "", "", &item.id);
        } else if response.clicked_by(egui::PointerButton::Primary)
            || avatar
                .as_ref()
                .is_some_and(|avatar| avatar.clicked_by(egui::PointerButton::Primary))
        {
            self.navigate(item);
        }
        if let Some(avatar) = avatar {
            avatar.context_menu(|ui| self.nav_menu(ui, item, state));
        }
        response.context_menu(|ui| self.nav_menu(ui, item, state));
        ui.advance_cursor_after_rect(rect);
    }
    fn nav_menu(&mut self, ui: &mut egui::Ui, item: &NavItem, state: &AppSnapshot) {
        if item.kind == "guild" {
            if ui.button("Abrir servidor").clicked() {
                self.send("guild", &item.id, "", "", "");
                ui.close();
            }
            if ui.button("Marcar como leído").clicked() {
                self.send("guild-mark-read", &item.id, "", "", "");
                ui.close();
            }
            if ui
                .button(if item.is_muted {
                    "Reactivar notificaciones"
                } else {
                    "Silenciar servidor"
                })
                .clicked()
            {
                self.send("guild-mute", &item.id, "", "", "");
                ui.close();
            }
            ui.menu_button("Ajustes de notificaciones", |ui| self.guild_notification_options(ui,item,state));
            let mut hidden = item.guild_notifications.hide_muted_channels;
            if ui.checkbox(&mut hidden,"Ocultar canales silenciados").on_hover_text("Preferencia local de este servidor. El canal abierto permanece visible.").changed() {
                self.send("guild-hide-muted",&item.id,hidden.to_string(),"","");
                ui.close();
            }
            if item.can_invite && ui.button("Invitar a personas").clicked() {
                self.send("guild-invite", &item.id, "", "", "");
                ui.close();
            }
            ui.separator();
            if ui.button("Configuración del servidor").clicked() {
                self.send("guild-settings", &item.id, "", "", "");
                ui.close();
            }
            if ui.button("Copiar ID del servidor").clicked() {
                ui.ctx().copy_text(item.id.clone());
                ui.close();
            }
            if ui
                .button(RichText::new("Salir del servidor").color(Color32::from_rgb(242, 63, 67)))
                .clicked()
            {
                self.confirm(
                    "Salir del servidor",
                    "¿Salir de este servidor? Para volver necesitarás otra invitación.",
                    UiAction::id("guild-leave", &item.id),
                );
                ui.close();
            }
            return;
        }
        let private = matches!(item.kind.as_str(), "dm" | "group");
        if ui
            .button(if private {
                "Abrir conversación"
            } else {
                "Abrir canal"
            })
            .clicked()
        {
            self.navigate(item);
            ui.close();
        }
        if private
            && ui
                .button(if item.is_pinned {
                    "Desfijar conversación"
                } else {
                    "Fijar conversación"
                })
                .clicked()
        {
            self.send("pin-dm", &item.id, "", "", "");
            ui.close();
        }
        if ui.button("Marcar como leído").clicked() {
            self.send("mark-read", &item.id, "", "", "");
            ui.close();
        }
        if private && ui.button("Cerrar conversación").clicked() {
            self.send("close-dm", &item.id, "", "", "");
            ui.close();
        }
        if private {
            ui.separator();
            if !item.recipient_id.is_empty() && ui.button("Ver perfil").clicked() {
                self.send(
                    "profile",
                    &item.recipient_id,
                    "",
                    "",
                    &state.selected_channel_id,
                );
                ui.close();
            }
            if ui
                .add_enabled(state.voice_available, egui::Button::new("Llamar"))
                .clicked()
            {
                self.send("call-dm", &item.id, "", "", &item.id);
                ui.close();
            }
        }
        ui.separator();
        if ui
            .button(if item.is_muted {
                "Reactivar notificaciones"
            } else {
                "Desactivar notificaciones"
            })
            .clicked()
        {
            self.send(
                "mute-channel",
                &item.id,
                (!item.is_muted).to_string(),
                "",
                "",
            );
            ui.close();
        }
        if private && !item.recipient_id.is_empty() {
            ui.separator();
            if ui
                .button(RichText::new("Bloquear").color(Color32::from_rgb(242, 63, 67)))
                .clicked()
            {
                self.confirm(
                    "Bloquear usuario",
                    "¿Bloquear a esta persona?",
                    UiAction::id("block-user", &item.recipient_id),
                );
                ui.close();
            }
            if state
                .relationships
                .iter()
                .any(|relation| relation.kind == 1 && relation.user.id == item.recipient_id)
                && ui
                    .button(RichText::new("Quitar amistad").color(Color32::from_rgb(242, 63, 67)))
                    .clicked()
            {
                self.confirm(
                    "Quitar amistad",
                    "¿Quitar esta persona de tus amigos?",
                    UiAction::id("remove-friend", &item.recipient_id),
                );
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Copiar ID").clicked() {
            ui.ctx().copy_text(item.id.clone());
            ui.close();
        }
    }
    pub(super) fn guild_notification_options(&mut self, ui: &mut egui::Ui, item: &NavItem, state: &AppSnapshot) {
        let settings = &item.guild_notifications;
        ui.add_enabled_ui(state.signed_in && !settings.pending && state.network_safety==notdiscord_core::safety::SafetyState::Allowed, |ui| {
            for (mode,label) in [(0,"Todos los mensajes"),(1,"Solo @menciones"),(2,"Nada")] {
                if ui.radio(settings.message_notifications==Some(mode),label).clicked() {
                    self.send("guild-notifications",&item.id,serde_json::json!({"message_notifications":mode}).to_string(),"","");
                    ui.close();
                }
            }
            if settings.message_notifications.is_none() { ui.small("Modo actual sin confirmar."); }
            ui.separator();
            for (key,label,flag,known) in [
                ("suppress_everyone","Suprimir @everyone y @here",settings.suppress_everyone,settings.suppress_everyone_known),
                ("suppress_roles","Suprimir menciones de roles",settings.suppress_roles,settings.suppress_roles_known),
            ] {
                let mut value = flag;
                if ui.add(egui::Checkbox::new(&mut value,label).indeterminate(!known)).changed() {
                    self.send("guild-notifications",&item.id,serde_json::json!({key:value}).to_string(),"","");
                    ui.close();
                }
            }
        });
        if settings.pending { ui.horizontal(|ui| {ui.spinner();ui.label("Guardando…");}); }
    }
    fn draw_account(&mut self, ctx: &egui::Context, state: &AppSnapshot) {
        let compact = ctx.content_rect().height() < 420.;
        let footer = egui::Area::new(egui::Id::new("account-panel"))
            .anchor(
                egui::Align2::LEFT_BOTTOM,
                egui::vec2(7., if self.error.is_empty() { -7. } else { -33. }),
            )
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                if self.background_blocked() { ui.disable(); }
                egui::Frame::new()
                    .fill(self.theme.panel)
                    .stroke(egui::Stroke::new(1.0_f32, self.theme.line))
                    .corner_radius(8)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(334.);
                        if !state.voice_error.is_empty() {
                            ui.horizontal(|ui| {
                                ui.colored_label(Color32::from_rgb(242, 63, 67), "Error de conexión de voz");
                                if ui.small_button("Cerrar aviso").clicked() { self.simple("dismiss-voice-error"); }
                            });
                            egui::ScrollArea::vertical().id_salt("voice-error-detail")
                                .max_height(if compact { 40. } else { 76. })
                                .auto_shrink([false, true]).show(ui, |ui| {
                                    ui.add(egui::Label::new(&state.voice_error).wrap());
                                });
                            ui.separator();
                        }
                        if let Some(game) = own_game(state) {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), if compact { 22. } else { 44. }), egui::Sense::hover());
                            paint_activity(ui, egui::Rect::from_min_size(rect.min + egui::vec2(2., 2.), egui::vec2(18., 18.)), false);
                            let text = egui::Rect::from_min_max(rect.min + egui::vec2(28., 0.), rect.max);
                            if compact { fitted_text(ui, text, &format!("Jugando a {game}"), 12., self.theme.ink); }
                            else {
                                fitted_text(ui, egui::Rect::from_min_size(text.min, egui::vec2(text.width(), 18.)), "ACTIVIDAD DE JUEGO", 11., self.theme.muted);
                                fitted_text(ui, egui::Rect::from_min_max(text.min + egui::vec2(0., 20.), text.max), game, 14., self.theme.ink);
                            }
                            ui.separator();
                        }
                        if state.voice_connected || state.voice_connecting {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(),44.), egui::Sense::hover());
                            let text = egui::Rect::from_min_max(rect.min, rect.max - egui::vec2(32., 0.));
                            let status = if !state.call_status_label.is_empty(){state.call_status_label.as_str()} else if state.voice_connected { "Voz conectada" } else { "Conectando a voz…" };
                            fitted_text(ui, egui::Rect::from_min_size(text.min, egui::vec2(text.width(), 20.)), status, 12., if state.voice_connected {Color32::from_rgb(35,165,90)} else {self.theme.accent});
                            let destination=state.guilds.iter().find(|guild|guild.id==state.call_guild_id).map(|guild|format!("{} / {}",guild.name,state.call_channel_name)).unwrap_or_else(||state.call_channel_name.clone());
                            let detail = format!("{} · {} participante(s)",destination,state.call_participants.len());
                            fitted_text(ui, egui::Rect::from_min_max(text.min + egui::vec2(0., 22.), text.max), &detail, 12., self.theme.muted);
                            let button = egui::Rect::from_center_size(egui::pos2(rect.right() - 13., rect.top() + 13.), egui::vec2(26., 26.));
                            let leave = ui.scope_builder(egui::UiBuilder::new().max_rect(button), |ui| icon_button(ui, Icon::Phone, 26., false, true)).inner.on_hover_text("Salir de la llamada");
                            leave.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Salir de la llamada"));
                            if leave.clicked() { self.simple("disconnect-voice"); }
                            ui.advance_cursor_after_rect(rect.union(button));
                            ui.separator();
                        }
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 1.;
                            ui.spacing_mut().button_padding = egui::vec2(1., 0.);
                            let user = state.current_user.clone().unwrap_or_default();
                            let avatar = self.avatar(ui, &user, &state.self_presence, 40.);
                            avatar.context_menu(|ui| self.account_menu(ui, state));
                            egui::Popup::menu(&avatar)
                                .id(avatar.id.with("account-left-menu"))
                                .show(|ui| self.account_menu(ui, state));
                            let name = ui.add_sized(
                                [128., 32.],
                                egui::Button::new(
                                    RichText::new(if user.name.is_empty() {
                                        "Sin sesión"
                                    } else {
                                        &user.name
                                    })
                                    .strong(),
                                )
                                .truncate(),
                            );
                            egui::Popup::menu(&name).show(|ui| self.account_menu(ui, state));
                            if icon_button(ui, Icon::Mic, 32., !state.signed_in, state.self_mute)
                                .on_hover_text("Silenciar micrófono")
                                .clicked()
                            {
                                self.simple("toggle-mute");
                            }
                            if ui
                                .add_sized([15., 32.], egui::Button::new("⌄"))
                                .on_hover_text("Dispositivo de entrada")
                                .clicked()
                            {
                                self.media_section = "input".into();
                                self.open_media();
                            }
                            if icon_button(
                                ui,
                                Icon::Headphones,
                                32.,
                                !state.signed_in,
                                state.self_deaf,
                            )
                            .on_hover_text("Ensordecer")
                            .clicked()
                            {
                                self.simple("toggle-deafen");
                            }
                            if ui
                                .add_sized([15., 32.], egui::Button::new("⌄"))
                                .on_hover_text("Dispositivo de salida")
                                .clicked()
                            {
                                self.media_section = "output".into();
                                self.open_media();
                            }
                            if icon_button(
                                ui,
                                Icon::Activity,
                                32.,
                                !state.game_activity_available,
                                !state.game_activity_enabled,
                            )
                            .on_hover_text("Mostrar actividad en Discord")
                            .clicked()
                                && state.game_activity_available
                            {
                                self.simple("toggle-game-activity");
                            }
                            if icon_button(ui, Icon::Settings, 32., false, false)
                                .on_hover_text("Ajustes")
                                .clicked()
                            {
                                self.settings_backup = Some(self.prefs.clone());
                                self.dialog = Some(Dialog::Settings);
                            }
                        });
                        if !state.signed_in && ui.button("Iniciar sesión").clicked() {
                            self.simple("login");
                        }
                    });
            });
        #[cfg(test)] ctx.data_mut(|data| data.insert_temp(egui::Id::new("left-footer-rect"), footer.response.rect));
        #[cfg(not(test))] let _ = footer;
    }
    fn account_menu(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        if let Some(me) = &state.current_user {
            if ui.button("Ver mi perfil").clicked() {
                self.send("profile", &me.id, "", "", &state.selected_channel_id);
                ui.close();
            }
        }
        for (value, label) in [
            ("online", "● En línea"),
            ("idle", "◐ Ausente"),
            ("dnd", "⛔ No molestar"),
            ("invisible", "○ Invisible"),
        ] {
            if ui.button(label).clicked() {
                self.send("status", "", value, "", "");
                ui.close();
            }
        }
        if state.signed_in && ui.button("Cerrar sesión").clicked() {
            self.confirm(
                "Cerrar sesión",
                "¿Cerrar la sesión de este cliente?",
                UiAction::new("logout"),
            );
            ui.close();
        }
    }
    pub(super) fn draw_members(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        if !state.selected_guild_id.is_empty()
            && (!state.channel_access.known
                || !state.channel_access.can_view
                || !state.channel_access.can_view_members)
        {
            ui.label("La lista de miembros no está disponible en este canal.");
            return;
        }
        let members = if state.selected_guild_id.is_empty() {
            &state.group_members
        } else {
            &state.guild_members
        };
        let total = if !state.selected_guild_id.is_empty() && state.guild_members_note.is_empty() && state.guild_members_total > 0 {
            state.guild_members_total as usize
        } else {
            members.len()
        };
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.), egui::Sense::hover());
        fitted_text(
            ui,
            rect,
            &format!("{} — {total}", if state.guild_members_note.is_empty()||members.is_empty() { "MIEMBROS" } else { "PARTICIPANTES" }),
            12.,
            self.theme.muted,
        );
        if !state.guild_members_note.is_empty() {
            ui.add(egui::Label::new(RichText::new(&state.guild_members_note).size(12.).color(self.theme.muted)).wrap());
        }
        let projection = list_projection::members(ui.ctx(), state);
        theme::vertical_scroll(ui)
            .id_salt(("members", &state.selected_channel_id))
            .max_height(
                (ui.available_height()
                    - if state.guild_members_has_more {
                        64.
                    } else if state.guild_members_partial {
                        30.
                    } else {
                        0.
                    })
                .max(30.),
            )
            .show_viewport(ui, |ui, viewport| {
                ui.spacing_mut().item_spacing.y = 0.;
                let visible=projection.visible((viewport.min.y-128.).max(0.),viewport.max.y+128.);
                if state.guild_member_list_bound && !visible.is_empty() {
                    member_demand::range(ui.ctx(),visible.start,visible.end.saturating_sub(1));
                }
                ui.add_space(projection.prefix[visible.start]);
                for row in &projection.rows[visible.clone()] {
                    let height = match row {
                        list_projection::MemberRow::Section { .. } => 38.,
                        _ => 64.,
                    };
                        match row {
                            list_projection::MemberRow::Section { label, color } => {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), height),
                                    egui::Sense::hover(),
                                );
                                fitted_text(
                                    ui,
                                    egui::Rect::from_min_max(
                                        rect.left_top() + egui::vec2(7., 14.),
                                        rect.right_bottom() - egui::vec2(2., 4.),
                                    ),
                                    label,
                                    12.,
                                    self.readable_role(*color),
                                );
                            }
                            list_projection::MemberRow::Member(index) => {
                                if let Some(member)=members.get(*index){self.member_row(ui, member, state);}else{ui.add_space(height);}
                            }
                            list_projection::MemberRow::Missing => {
                                let (rect,_)=ui.allocate_exact_size(egui::vec2(ui.available_width(),height),egui::Sense::hover());
                                fitted_text(ui,rect.shrink2(egui::vec2(8.,20.)),"Cargando miembro…",12.,self.theme.muted);
                            }
                        }
                }
                ui.add_space(projection.prefix.last().copied().unwrap_or(0.)-projection.prefix[visible.end]);
            });
        if state.guild_members_partial {
            ui.label(
                RichText::new("Lista parcial")
                    .size(12.)
                    .color(self.theme.muted),
            );
        }
        if state.guild_members_has_more && ui.button("Cargar más miembros").clicked() {
            self.simple("members-more");
        }
    }
    fn readable_role(&self, color: i32) -> Color32 {
        let chosen = role_color(color, self.theme.ink);
        let luminance = |c: Color32| {
            let linear = |v: u8| {
                let v = v as f64 / 255.;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
        };
        let a = luminance(chosen);
        let b = luminance(self.theme.panel);
        if (a.max(b) + 0.05) / (a.min(b) + 0.05) < 3. {
            self.theme.ink
        } else {
            chosen
        }
    }
    fn member_row(&mut self, ui: &mut egui::Ui, member: &ChannelMember, state: &AppSnapshot) {
        self.person_row(ui, member, state, 14., 36.);
    }
    fn person_row(
        &mut self,
        ui: &mut egui::Ui,
        member: &ChannelMember,
        state: &AppSnapshot,
        name_size: f32,
        avatar_size: f32,
    ) {
        self.emoji_section(ui, ("member-row-emojis", &member.user.id), |this, ui| this.person_row_inner(ui, member, state, name_size, avatar_size));
    }
    fn person_row_inner(&mut self, ui: &mut egui::Ui, member: &ChannelMember, state: &AppSnapshot, name_size: f32, avatar_size: f32) {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 64.), egui::Sense::click());
        let response=response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let live=state.user_presences.get(&member.user.id);
        let presence=live.map(|presence|presence.status.as_str()).unwrap_or(&member.presence);
        if ui.rect_contains_pointer(rect) {
            ui.painter().rect_filled(rect, 5., self.theme.surface);
        }
        let picture = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 8. + avatar_size / 2., rect.center().y),
            egui::Vec2::splat(avatar_size),
        );
        let avatar = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(picture)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| self.avatar(ui, &member.user, presence, avatar_size),
            )
            .inner;
        let left = picture.right() + 10.;
        let right=if state.selected_guild_id.is_empty(){rect.right()-4.}else{
            draw_member_voice_flags(ui,rect,state,&state.selected_guild_id,&member.user.id,None)
        };
        let width = (right - 4. - left).max(0.);
        let name = if !state.selected_guild_id.is_empty() {
            state.guild_display_name(&member.user)
        } else if member.nickname.is_empty() {
            member.user.name.as_str()
        } else {
            member.nickname.as_str()
        };
        let name = format!("{}{}", name, if member.is_owner { " ♛" } else { "" });
        let is_self = state.current_user.as_ref().is_some_and(|user| user.id == member.user.id);
        let local_game = is_self.then(|| own_game(state)).flatten();
        let activities = if matches!(presence, "offline" | "invisible") { &[][..] }
            else { live.map(|presence| presence.activities.as_slice()).unwrap_or(&member.activities) };
        let visible_activities: Vec<_> = activities.iter().take(8).filter(|activity|
            !(is_self && state.game_activity_configured && local_game.is_none() && matches!(activity.kind, 0 | 5)))
            .cloned().collect();
        let activities = visible_activities.as_slice();
        let game = member_game(state,&member.user.id,presence,activities);
        let custom = live.map(|presence| presence.custom_status.as_str()).unwrap_or(&member.custom_status);
        let emoji = live.map(|presence| presence.custom_status_emoji.as_ref()).unwrap_or(member.custom_status_emoji.as_ref());
        let extra_game = has_status(custom, emoji) && !game.is_empty();
        fitted_text(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(left, if extra_game { rect.top() + 3. } else { rect.center().y - 19. }),
                egui::vec2(width, 21.),
            ),
            &name,
            name_size,
            self.readable_role(member.role_color),
        );
        let fallback = presence_label(presence);
        self.presence_line(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(left, if extra_game { rect.top() + 24. } else { rect.center().y + 2. }),
                egui::vec2(width, 18.),
            ),
            custom,
            emoji,
            if local_game.is_some() { &[] } else { activities },
            &game,
            fallback,
        );
        if extra_game { fitted_text(ui, egui::Rect::from_min_size(egui::pos2(left, rect.top() + 43.), egui::vec2(width, 17.)), &game, 12., self.theme.muted); }
        if response.clicked() || avatar.clicked() {
            self.send(
                "profile",
                &member.user.id,
                "",
                "",
                &state.selected_channel_id,
            );
        }
        avatar.context_menu(|ui| self.people_menu(ui, &member.user, state));
        response.context_menu(|ui| self.people_menu(ui, &member.user, state));
        ui.advance_cursor_after_rect(rect);
    }
    fn draw_people(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        let filter_id = egui::Id::new((
            "friends-filter",
            state
                .current_user
                .as_ref()
                .map(|person| person.id.as_str())
                .unwrap_or(""),
        ));
        let mut filter = ui
            .ctx()
            .data_mut(|data| data.get_temp::<FriendsFilter>(filter_id))
            .unwrap_or_default();
        if state.view == "friends" {
            let width = (ui.available_width() - 190.).clamp(140., 420.);
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut filter.online_only, true, "Conectados");
                ui.selectable_value(&mut filter.online_only, false, "Todos");
                ui.add_sized(
                    [width, 32.],
                    bordered(
                        egui::TextEdit::singleline(&mut filter.query)
                            .id(filter_id.with("search"))
                            .hint_text("Buscar por nombre o usuario"),
                    ),
                );
                if !filter.query.is_empty()
                    && ui
                        .small_button("×")
                        .on_hover_text("Borrar búsqueda de amigos")
                        .clicked()
                {
                    filter.query.clear();
                }
            });
            ui.ctx()
                .data_mut(|data| data.insert_temp(filter_id, filter.clone()));
            ui.separator();
        }
        let requests_filter = FriendsFilter::default();
        let relationships = friends_visible(
            state,
            if state.view == "requests" {
                &requests_filter
            } else {
                &filter
            },
        );
        let count = relationships.len();
        let heading = if state.view == "requests" {
            "SOLICITUDES"
        } else if filter.online_only {
            "CONECTADOS"
        } else {
            "TODOS LOS AMIGOS"
        };
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.), egui::Sense::hover());
        fitted_text(
            ui,
            rect,
            &format!("{heading} — {count}"),
            12.,
            self.theme.muted,
        );
        if count == 0 {
            ui.label(if state.view == "requests" {
                "No hay solicitudes pendientes."
            } else if filter.query.trim().is_empty() {
                "No hay amigos en este filtro."
            } else {
                "No hay amigos que coincidan con la búsqueda."
            });
        }
        theme::vertical_scroll(ui).id_salt("friends").show_rows(
            ui,
            80.,
            relationships.len(),
            |ui, range| {
                for relation in &relationships[range] {
                    let presence = state.user_presences.get(&relation.user.id);
                    let member = ChannelMember {
                        user: relation.user.clone(),
                        presence: presence.map(|p| p.status.clone()).unwrap_or_default(),
                        custom_status: presence.map(|p| p.custom_status.clone()).unwrap_or_else(
                            || match relation.kind {
                                3 => "Solicitud recibida".into(),
                                4 => "Solicitud enviada".into(),
                                _ => String::new(),
                            },
                        ),
                        custom_status_emoji: presence.and_then(|p| p.custom_status_emoji.clone()),
                        activities: presence.map(|p| p.activities.clone()).unwrap_or_default(),
                        ..Default::default()
                    };
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 80.),
                        egui::Sense::hover(),
                    );
                    let actions_width = if relation.kind == 4 { 92. } else { 170. };
                    let actions = egui::Rect::from_min_max(
                        egui::pos2((rect.right() - actions_width).max(rect.left()), rect.top()),
                        rect.right_bottom(),
                    );
                    let identity = egui::Rect::from_min_max(
                        rect.left_top() + egui::vec2(0., 8.),
                        egui::pos2((actions.left() - 8.).max(rect.left()), rect.bottom() - 8.),
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(identity)
                            .layout(Layout::left_to_right(Align::Center)),
                        |ui| self.person_row(ui, &member, state, 16., 40.),
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(actions)
                            .layout(Layout::right_to_left(Align::Center)),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 7.;
                            let remove = match relation.kind {
                                1 => "Quitar",
                                4 => "Cancelar",
                                _ => "Rechazar",
                            };
                            if ui
                                .add_enabled(
                                    state.signed_in && !state.busy,
                                    egui::Button::new(remove).min_size(egui::vec2(74., 36.)),
                                )
                                .clicked()
                            {
                                self.confirm(
                                    "Modificar amistad",
                                    format!(
                                        "¿{} la relación con {}?",
                                        remove.to_lowercase(),
                                        relation.user.name
                                    )
                                    .as_str(),
                                    UiAction::id("remove-friend", &relation.user.id),
                                );
                            }
                            if relation.kind == 3 {
                                if ui
                                    .add_enabled(
                                        state.signed_in && !state.busy,
                                        egui::Button::new("Aceptar").min_size(egui::vec2(82., 36.)),
                                    )
                                    .clicked()
                                {
                                    self.send("accept-friend", &relation.user.id, "", "", "");
                                }
                            } else if relation.kind == 1
                                && ui
                                    .add_enabled(
                                        state.signed_in && !state.busy,
                                        egui::Button::new("Mensaje").min_size(egui::vec2(82., 36.)),
                                    )
                                    .clicked()
                            {
                                self.send("open-dm", &relation.user.id, "", "", "");
                            }
                        },
                    );
                    ui.painter().line_segment(
                        [rect.left_bottom(), rect.right_bottom()],
                        egui::Stroke::new(1.0_f32, self.theme.line),
                    );
                    ui.advance_cursor_after_rect(rect);
                }
            },
        );
    }
    pub(super) fn draw_dm_sidebar(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        self.emoji_section(ui,("dm-profile-emojis",&state.selected_channel_id),|this,ui|this.draw_dm_sidebar_inner(ui,state));
    }
    fn draw_dm_sidebar_inner(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        let Some(item) = state.direct_messages.iter().find(|item| {
            item.id == state.selected_channel_id
                && item.kind == "dm"
                && !item.recipient_id.is_empty()
        }) else {
            ui.label("No hay un perfil disponible para esta conversación.");
            return;
        };
        let id = &item.recipient_id;
        if self.dm_sidebar_user_id != *id {
            self.dm_sidebar_user_id = id.clone();
            self.dm_sidebar_profile = None;
            self.dm_sidebar_loading = true;
            self.send("dm-sidebar-profile", id, "", "", &state.selected_channel_id);
        }
        // Temporarily move the single cached profile so drawing avatars can borrow
        // self mutably without cloning the mutual-user/server lists every frame.
        let details = self
            .dm_sidebar_profile
            .take()
            .filter(|details| details.user.id == *id);
        let person = details
            .as_ref()
            .map(|details| details.user.clone())
            .or_else(|| {
                state
                    .relationships
                    .iter()
                    .find(|relation| relation.user.id == *id)
                    .map(|relation| relation.user.clone())
            })
            .unwrap_or(Person {
                id: id.clone(),
                name: item.name.clone(),
                display_name: item.name.clone(),
                avatar_url: item.icon_url.clone(),
                ..Default::default()
            });
        let live = state.user_presences.get(id);
        let presence = live
            .map(|presence| presence.status.as_str())
            .or_else(|| details.as_ref().map(|details| details.presence.as_str()))
            .unwrap_or("");
        theme::vertical_scroll(ui)
            .id_salt(("dm-sidebar", id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add_space(4.);
                self.pictures.profile_banner(ui, &person, details.as_ref(), self.prefs.allow_gif_playback);
                ui.add_space(8.);
                if self.profile_avatar_button(ui, &person, presence,80.).clicked() {
                    self.open_profile_avatar(&person);
                }
                ui.add_space(12.);
                let (name_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 28.),
                    egui::Sense::hover(),
                );
                let name = if person.display_name.is_empty() {
                    &person.name
                } else {
                    &person.display_name
                };
                fitted_text(ui, name_rect, name, 20., self.theme.ink);
                if !person.username.is_empty() {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 21.),
                        egui::Sense::hover(),
                    );
                    fitted_text(
                        ui,
                        rect,
                        &format!("@{}", person.username),
                        13.,
                        self.theme.muted,
                    );
                }
                let custom = live
                    .map(|presence| presence.custom_status.as_str())
                    .or_else(|| {
                        details
                            .as_ref()
                            .map(|details| details.custom_status.as_str())
                    })
                    .unwrap_or("");
                let emoji = live
                    .and_then(|presence| presence.custom_status_emoji.as_ref())
                    .or_else(|| {
                        details
                            .as_ref()
                            .and_then(|details| details.custom_status_emoji.as_ref())
                    });
                if has_status(custom, emoji) {
                    ui.add_space(8.);
                    ui.horizontal_wrapped(|ui| {
                        if let Some(emoji) = emoji {
                            self.emoji(ui, emoji, 17.);
                        }
                        ui.add(egui::Label::new(custom).wrap());
                    });
                }
                ui.add_space(12.);
                ui.separator();
                ui.add_space(8.);
                if let Some(details) = &details {
                    if !details.bio.is_empty() {
                        ui.small("SOBRE MÍ");
                        self.rich_custom_text(ui, &details.bio, state, self.prefs.chat_size);
                        ui.add_space(12.);
                    }
                    if let Some(created) = details.created_at {
                        ui.small("MIEMBRO DESDE");
                        ui.label(profile_date(&created));
                        ui.add_space(12.);
                    }
                    if let Some(since) = details.friends_since {
                        ui.small("AMIGOS DESDE");
                        ui.label(profile_date(&since));
                        ui.add_space(12.);
                    }
                } else if self.dm_sidebar_loading {
                    ui.label("Cargando perfil…");
                } else {
                    ui.small("El perfil no está disponible en este momento.");
                    if ui.button("Reintentar").clicked() {
                        self.dm_sidebar_loading = true;
                        self.send("dm-sidebar-profile", id, "", "", &state.selected_channel_id);
                    }
                }
                let activities = live
                    .map(|presence| presence.activities.as_slice())
                    .or_else(|| {
                        details
                            .as_ref()
                            .map(|details| details.activities.as_slice())
                    })
                    .unwrap_or(&[]);
                for entry in activities.iter().filter(|entry| entry.kind != 4).take(4) {
                    ui.small("ACTIVIDAD");
                    ui.horizontal(|ui| {
                        if !entry.large_image_url.is_empty() {
                            self.pictures.image(
                                ui,
                                &entry.large_image_url,
                                Vec2::splat(36.),
                                false,
                            );
                        }
                        ui.add(egui::Label::new(activity(entry)).wrap());
                    });
                    ui.add_space(10.);
                }
                if let Some(details) = &details {
                    if details.mutual_friends_available {
                        ui.label(format!("{} amigos en común", details.mutual_friends.len()));
                    }
                    if details.mutual_guilds_available {
                        ui.label(format!(
                            "{} servidores en común",
                            details.mutual_guilds.len()
                        ));
                    }
                }
                ui.add_space(10.);
                if ui.button("Ver perfil completo").clicked() {
                    self.send("profile", id, "", "", &state.selected_channel_id);
                }
                let relationship = state
                    .relationships
                    .iter()
                    .find(|relation| relation.user.id == *id)
                    .map(|relation| relation.kind)
                    .unwrap_or(0);
                if relationship == 0 && !person.is_bot && ui.button("Añadir amigo").clicked() {
                    self.send("add-friend-id", id, "", "", &state.selected_channel_id);
                }
            });
        self.dm_sidebar_profile = details;
    }
    fn draw_profile(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        self.emoji_section(ui,"profile-emojis",|this,ui|this.draw_profile_inner(ui,state));
    }
    fn draw_profile_inner(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        self.draw_chat_return_header(ui, "return-chat");
        let Some(person) = state.profile_user.as_ref() else {
            ui.label("Cargando perfil…");
            return;
        };
        theme::vertical_scroll(ui)
            .id_salt("profile")
            .show(ui, |ui| {
                ui.set_max_width(780.);
                ui.add_space(15.);
                let details = state.profile_details.as_ref();
                let webhook=details.is_some_and(|details|details.is_webhook);
                self.pictures.profile_banner(ui, person, details, self.prefs.allow_gif_playback);
                ui.add_space(10.);
                ui.horizontal(|ui| {
                    if self.profile_avatar_button(ui,person,details.map(|p|p.presence.as_str()).unwrap_or(""),88.).clicked() {self.open_profile_avatar(person);}
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(if !webhook && !state.selected_guild_id.is_empty()
                                && details.is_some_and(|details|details.guild_id==state.selected_guild_id) {
                                state.guild_display_name(person)
                            } else if person.display_name.is_empty() {
                                &person.name
                            } else {
                                &person.display_name
                            })
                            .size(27.)
                            .strong(),
                        );
                        ui.label(
                            RichText::new(format!("@{}", person.username)).color(self.theme.muted),
                        );
                        if webhook || person.is_bot {
                            ui.label(RichText::new(if webhook {"WEBHOOK"} else {"BOT / APLICACIÓN"}).small().strong().color(self.theme.accent));
                        }
                        if let Some(d) = details {
                            ui.horizontal(|ui| {
                                if let Some(e) = &d.custom_status_emoji {
                                    self.emoji(ui, e, 18.);
                                }
                                ui.label(&d.custom_status);
                            });
                        } else {
                            ui.label(&state.profile_subtitle);
                        }
                    });
                });
                if !state.profile_subtitle.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&state.profile_subtitle).color(self.theme.muted)).wrap());
                }
                if !webhook {self.draw_profile_editor(ui,state,person);}
                if !webhook && !state.selected_guild_id.is_empty() {
                    ui.menu_button("Acciones del servidor", |ui| self.draw_member_moderation_menu(ui, person, state));
                }
                ui.add_space(15.);
                if let Some(d) = details {
                    if !d.guild_name.is_empty() {
                        ui.label(
                            RichText::new(format!("EN {}", d.guild_name))
                                .size(12.)
                                .color(self.theme.muted),
                        );
                        if !d.nickname.is_empty() {
                            ui.label(format!("Apodo: {}", d.nickname));
                        }
                        ui.horizontal_wrapped(|ui| {
                            for role in &d.guild_roles {
                                ui.colored_label(
                                    role_color(role.color, self.theme.ink),
                                    format!("● {}", role.name),
                                );
                            }
                        });
                    }
                    if !d.bio.is_empty() {
                        self.rich_custom_text(ui, &d.bio, state, self.prefs.chat_size);
                    }
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.small(if webhook {"IDENTIFICADOR CREADO"} else {"CUENTA CREADA"});
                            ui.label(
                                d.created_at
                                    .map(|t| profile_date(&t))
                                    .unwrap_or_else(|| "No disponible".into()),
                            );
                        });
                        if d.friends_since.is_some()
                            || state
                                .relationships
                                .iter()
                                .any(|r| r.kind == 1 && r.user.id == person.id)
                        {
                            ui.add_space(35.);
                            ui.vertical(|ui| {
                                ui.small("AMIGOS DESDE");
                                ui.label(
                                    d.friends_since
                                        .map(|t| profile_date(&t))
                                        .unwrap_or_else(|| "No disponible".into()),
                                );
                            });
                        }
                    });
                    ui.add_space(20.);
                    ui.horizontal(|ui| {
                        for (i, label) in [
                            "Actividad".to_string(),
                            format!("Amigos en común ({})", d.mutual_friends.len()),
                            format!("Servidores en común ({})", d.mutual_guilds.len()),
                        ]
                        .iter()
                        .enumerate()
                        {
                            if ui.selectable_label(self.profile_tab == i, label).clicked() {
                                self.profile_tab = i;
                            }
                        }
                    });
                    ui.separator();
                    match self.profile_tab {
                        0 => {
                            if d.activities.is_empty() {
                                ui.label("Sin actividad disponible");
                            }
                            for a in d.activities.iter().filter(|a| a.kind != 4).take(8) {
                                egui::Frame::new()
                                    .fill(self.theme.surface)
                                    .corner_radius(8)
                                    .inner_margin(egui::Margin::same(14))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if !a.large_image_url.is_empty() {
                                                self.pictures.image(
                                                    ui,
                                                    &a.large_image_url,
                                                    Vec2::splat(64.),
                                                    false,
                                                );
                                            }
                                            ui.vertical(|ui| {
                                                ui.label(activity(a));
                                                if let Some(t) = a.started_at {
                                                    ui.small(format!("Desde {}", local_time(&t)));
                                                }
                                            });
                                        });
                                    });
                            }
                        }
                        1 => {
                            if !d.mutual_friends_available {
                                ui.label("Discord no proporcionó esta información");
                            }
                            for friend in d.mutual_friends.iter().take(100) {
                                let member = ChannelMember {
                                    user: friend.clone(),
                                    ..Default::default()
                                };
                                self.member_row(ui, &member, state);
                            }
                        }
                        _ => {
                            if !d.mutual_guilds_available {
                                ui.label("Discord no proporcionó esta información");
                            }
                            for guild in d.mutual_guilds.iter().take(100) {
                                ui.horizontal(|ui| {
                                    self.pictures.image(
                                        ui,
                                        guild.icon_url.as_deref().unwrap_or(""),
                                        Vec2::splat(40.),
                                        true,
                                    );
                                    if ui.button(&guild.name).clicked() {
                                        self.send("guild", &guild.id, "", "", "");
                                    }
                                });
                            }
                        }
                    }
                } else {
                    ui.label("Cargando perfil…");
                }
                ui.add_space(15.);
                if state
                    .current_user
                    .as_ref()
                    .is_none_or(|u| u.id != person.id)
                    && !webhook
                {
                    ui.horizontal(|ui| {
                        if ui.button("Enviar mensaje").clicked() {
                            self.send("open-dm", &person.id, "", "", "");
                        }
                        let kind = state
                            .relationships
                            .iter()
                            .find(|r| r.user.id == person.id)
                            .map(|r| r.kind)
                            .unwrap_or(0);
                        match kind {
                            _ if person.is_bot => {},
                            0 => {
                                if ui.button("Añadir amigo").clicked() {
                                    self.send("add-friend-id", &person.id, "", "", "");
                                }
                            }
                            3 => {
                                if ui.button("Aceptar solicitud").clicked() {
                                    self.send("accept-friend", &person.id, "", "", "");
                                }
                            }
                            4 => {
                                ui.add_enabled(false, egui::Button::new("Solicitud enviada"));
                            }
                            _ => {}
                        }
                    });
                }
            });
    }
}
fn role_color(color: i32, fallback: Color32) -> Color32 {
    if color == 0 {
        fallback
    } else {
        Color32::from_rgb(
            ((color >> 16) & 255) as u8,
            ((color >> 8) & 255) as u8,
            (color & 255) as u8,
        )
    }
}

#[cfg(test)]
mod shell_tests {
    use super::*;
    #[test]
    fn category_title_right_edge_toggles_children_in_the_same_frame_without_actions() {
        let (mut app,mut commands)=harness(); let ctx=app.ctx.clone();
        let mut state=state(); state.selected_guild_id="50".into();
        state.channels=vec![NavItem{id:"10".into(),kind:"category".into(),name:"Category".into(),..Default::default()},
            NavItem{id:"20".into(),parent_id:"10".into(),kind:"text".into(),name:"Child".into(),..Default::default()},
            NavItem{id:"30".into(),kind:"text".into(),name:"Uncategorized".into(),..Default::default()}];
        let draw=|app:&mut NotDiscordUi,events|ctx.run(egui::RawInput{
            screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(360.,250.))),events,..Default::default()
        },|ctx|{egui::CentralPanel::default().show(ctx,|ui|{
            for item in &state.channels { if !hidden_by_category(ctx,&state,item) {app.nav_row(ui,item,&state);} }
        });});
        for _ in 0..3 {draw(&mut app,vec![]);}
        let point=egui::pos2(330.,22.);
        draw(&mut app,pointer(point,egui::PointerButton::Primary,true));
        let closed=draw(&mut app,pointer(point,egui::PointerButton::Primary,false));
        assert!(text(&closed,"Child").is_none());assert!(text(&closed,"▸ Category").is_some());
        assert!(text(&closed,"Uncategorized").is_some());
        draw(&mut app,pointer(point,egui::PointerButton::Primary,true));
        let open=draw(&mut app,pointer(point,egui::PointerButton::Primary,false));
        assert!(text(&open,"Child").is_some());assert!(text(&open,"▾ Category").is_some());
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn disabled_notification_channel_uses_the_same_muted_ink_as_placeholders_in_both_themes() {
        let (mut app,_)=harness();let ctx=app.ctx.clone();let mut state=state();state.selected_channel_id="20".into();
        let mut item=NavItem{id:"20".into(),kind:"text".into(),name:"Muted channel".into(),notifications_disabled:true,..Default::default()};
        fn color(shape:&egui::Shape)->Option<Color32>{match shape{
            egui::Shape::Text(text)if text.galley.job.text=="Muted channel"=>text.galley.job.sections.first().map(|section|section.format.color),
            egui::Shape::Vec(shapes)=>shapes.iter().find_map(color),_=>None}}
        for background in ["#313338","#F4F4F4"]{
            app.prefs.background=background.into();app.theme=Theme::from_preferences(&app.prefs);app.theme.apply(&ctx,&app.prefs);
            for disabled in [true,false]{item.notifications_disabled=disabled;
                let output=ctx.run(egui::RawInput::default(),|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.nav_row(ui,&item,&state));});
                assert_eq!(output.shapes.iter().find_map(|shape|color(&shape.shape)),Some(if disabled{app.theme.muted}else{app.theme.ink}));
                assert_eq!(ctx.style().visuals.weak_text_color(),app.theme.muted);
            }
        }
    }
    #[test]
    fn member_game_text_updates_alongside_custom_status_and_hides_when_offline_or_private() {
        let (mut app,mut commands)=harness();let ctx=app.ctx.clone();let mut state=state();
        let member=ChannelMember{user:Person{id:"person".into(),name:"Player".into(),..Default::default()},presence:"online".into(),custom_status:"Custom status".into(),
            activities:vec![PresenceActivity{kind:0,name:"First game".into(),..Default::default()}],..Default::default()};
        let draw=|app:&mut NotDiscordUi,state:&AppSnapshot|ctx.run(egui::RawInput::default(),|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.member_row(ui,&member,state));});
        let output=draw(&mut app,&state);assert!(text(&output,"Custom status").is_some());assert!(text(&output,"Jugando a First game").is_some());
        state.user_presences.insert("person".into(),UserPresence{status:"online".into(),activities:vec![PresenceActivity{kind:0,name:"Second game".into(),..Default::default()}],..Default::default()});
        let output=draw(&mut app,&state);assert!(text(&output,"Jugando a Second game").is_some());assert!(text(&output,"Jugando a First game").is_none());
        state.user_presences.get_mut("person").unwrap().status="offline".into();
        assert!(text(&draw(&mut app,&state),"Jugando a Second game").is_none());
        state.current_user=Some(member.user.clone());state.game_activity_configured=true;state.game_activity_enabled=false;
        state.user_presences.get_mut("person").unwrap().status="online".into();
        assert!(text(&draw(&mut app,&state),"Jugando a Second game").is_none());
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn game_and_voice_footer_reserve_both_sidebars_and_release_the_space_without_overlap() {
        let (mut app,mut commands)=harness();let ctx=app.ctx.clone();let mut state=state();
        state.game_activity_running=true;state.game_activity_configured=true;state.game_activity_enabled=true;
        state.self_presence="online".into();state.custom_activity_label="Offline fixture game".into();
        state.voice_connected=true;state.call_channel_name="Fixture voice".into();
        for (height,zoom) in [(700.,1.),(500.,1.5),(350.,1.)]{
            ctx.set_zoom_factor(zoom);
            let draw=|app:&mut NotDiscordUi,state:&AppSnapshot|ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1200.,height))),..Default::default()},|ctx|app.draw_shell(ctx,state));
            for _ in 0..3{draw(&mut app,&state);}
            let output=draw(&mut app,&state);assert!(text(&output,if height<420.{"Jugando a Offline fixture game"}else{"Offline fixture game"}).is_some());
            let footer=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new("left-footer-rect"))).unwrap();
            assert!(footer.top()>=0.&&footer.bottom()<=height+1.);
            if height>=420. {
                fn painted_text(shape:&egui::Shape,label:&str)->Option<egui::Rect>{match shape{
                    egui::Shape::Text(text)if text.galley.job.text==label=>Some(egui::Rect::from_min_size(text.pos,text.galley.size())),
                    egui::Shape::Vec(shapes)=>shapes.iter().find_map(|shape|painted_text(shape,label)),_=>None}}
                fn horizontal_lines(shape:&egui::Shape,footer:egui::Rect,lines:&mut Vec<f32>){match shape{
                    egui::Shape::LineSegment{points,..}if (points[0].y-points[1].y).abs()<0.1
                        && (points[1].x-points[0].x).abs()>80. && footer.contains(points[0]) && footer.contains(points[1])=>lines.push(points[0].y),
                    egui::Shape::Vec(shapes)=>for shape in shapes{horizontal_lines(shape,footer,lines)},_=>{}}}
                let detail=output.shapes.iter().find_map(|shape|painted_text(&shape.shape,"Fixture voice · 0 participante(s)")).unwrap();
                let name=output.shapes.iter().find_map(|shape|painted_text(&shape.shape,"Yo")).unwrap();
                let mut separators=Vec::new();for shape in &output.shapes{horizontal_lines(&shape.shape,footer,&mut separators);}
                assert!(name.top()>=detail.bottom()+2.,"Account name must remain below the voice detail: {name:?} vs {detail:?}");
                assert!(separators.iter().any(|y|*y>detail.bottom()&&*y<name.top()),"A separator must fit between detail and account name: {separators:?}, {detail:?}, {name:?}");
                assert!(!separators.iter().any(|y|*y>=detail.top()&&*y<=detail.bottom()),"No separator may cross the painted voice detail");
            }
            for key in ["rail-reserved-rect","navigation-reserved-rect"]{
                let reserved=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new(key))).unwrap();
                assert!(reserved.bottom()<=footer.top()+1.,"{key}: {reserved:?} vs {footer:?}");
            }
            let active_top=footer.top();state.game_activity_running=false;state.voice_connected=false;
            for _ in 0..3{draw(&mut app,&state);}
            let output=draw(&mut app,&state);assert!(text(&output,"Offline fixture game").is_none());
            let footer=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new("left-footer-rect"))).unwrap();assert!(footer.top()>active_top);
            state.game_activity_running=true;state.voice_connected=true;
        }
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn guild_notification_options_emit_only_the_clicked_minimal_patch_and_pending_blocks_more() {
        let (mut app,mut commands)=harness(); let ctx=app.ctx.clone();
        let mut state=state(); state.signed_in=true; state.selected_guild_id="original".into();
        let mut item=NavItem {id:"50".into(),kind:"guild".into(),name:"Target server".into(),..Default::default()};
        item.guild_notifications.message_notifications=Some(1);
        item.guild_notifications.suppress_everyone_known=true; item.guild_notifications.suppress_roles_known=true;
        let draw=|app:&mut NotDiscordUi,item:&NavItem,events|ctx.run(egui::RawInput {
            screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(650.,450.))),events,..Default::default()
        },|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.guild_notification_options(ui,item,&state));});
        for _ in 0..3 {draw(&mut app,&item,vec![]);} assert!(commands.try_recv().is_err(),"Rendering must not fetch or write settings");
        let output=draw(&mut app,&item,vec![]); let point=text(&output,"Nada").unwrap();
        for pressed in [true,false] {draw(&mut app,&item,pointer(point,egui::PointerButton::Primary,pressed));}
        let action=commands.try_recv().unwrap(); assert_eq!((action.kind.as_str(),action.id.as_str()),("guild-notifications","50"));
        assert_eq!(serde_json::from_str::<Value>(&action.text).unwrap(),serde_json::json!({"message_notifications":2}));
        assert!(commands.try_recv().is_err()); assert_eq!(state.selected_guild_id,"original");
        let output=draw(&mut app,&item,vec![]); let point=text(&output,"Suprimir menciones de roles").unwrap();
        for pressed in [true,false] {draw(&mut app,&item,pointer(point,egui::PointerButton::Primary,pressed));}
        let action=commands.try_recv().unwrap();assert_eq!(serde_json::from_str::<Value>(&action.text).unwrap(),serde_json::json!({"suppress_roles":true}));
        item.guild_notifications.pending=true;
        let output=draw(&mut app,&item,vec![]); let point=text(&output,"Todos los mensajes").unwrap();
        for pressed in [true,false] {draw(&mut app,&item,pointer(point,egui::PointerButton::Primary,pressed));}
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn hiding_muted_channels_includes_category_mutes_but_preserves_the_open_channel() {
        let mut state=state();state.selected_guild_id="50".into();state.selected_channel_id="21".into();
        let mut guild=NavItem {id:"50".into(),kind:"guild".into(),..Default::default()};guild.guild_notifications.hide_muted_channels=true;state.guilds=vec![guild];
        state.channels=vec![NavItem {id:"10".into(),kind:"category".into(),is_muted:true,..Default::default()},
            NavItem {id:"21".into(),kind:"text".into(),parent_id:"10".into(),..Default::default()},
            NavItem {id:"22".into(),kind:"text".into(),parent_id:"10".into(),..Default::default()},
            NavItem {id:"30".into(),kind:"text".into(),..Default::default()},NavItem {id:"31".into(),kind:"text".into(),is_muted:true,..Default::default()}];
        let visible=navigation_rows(&state).into_iter().filter_map(|row|match row{NavigationRow::Item(item)=>Some(item.id.clone()),_=>None}).collect::<Vec<_>>();
        assert_eq!(visible,["10","21","30"]);
        state.guilds[0].guild_notifications.hide_muted_channels=false;
        assert_eq!(navigation_rows(&state).len(),5);
    }
    #[test]
    fn bot_and_webhook_profiles_show_their_identity_without_impossible_friend_actions() {
        let (mut app,_,mut actions)=super::super::tests::harness();let ctx=app.ctx.clone();
        let bot=Person {id:"400".into(),name:"Bot visible".into(),username:"bot_name".into(),is_bot:true,..Default::default()};
        let mut state=AppSnapshot {view:"profile".into(),signed_in:true,profile_user:Some(bot.clone()),profile_details:Some(ProfileDetails{user:bot,..Default::default()}),..Default::default()};
        let frame=|app:&mut NotDiscordUi,state:&AppSnapshot|ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1000.,900.))),..Default::default()},|ctx| {
            egui::CentralPanel::default().show(ctx,|ui|app.draw_profile(ui,state));
        });
        for webhook in [false,true] {
            state.profile_details.as_mut().unwrap().is_webhook=webhook;
            for _ in 0..3 {frame(&mut app,&state);}
            let output=frame(&mut app,&state);
            assert!(text(&output,if webhook {"WEBHOOK"}else{"BOT / APLICACIÓN"}).is_some());
            assert!(text(&output,"Bot visible").is_some());assert!(text(&output,"Añadir amigo").is_none());
            assert_eq!(text(&output,"Enviar mensaje").is_some(),!webhook);
            assert!(actions.try_recv().is_err());
        }
    }
    #[test]
    fn guild_voice_chat_and_connect_are_distinct_and_duplicate_joins_are_disabled() {
        let (mut app,_,mut actions)=super::super::tests::harness();let ctx=app.ctx.clone();
        let mut state=AppSnapshot {signed_in:true,voice_available:true,selected_guild_id:"500".into(),..Default::default()};
        let item=NavItem {id:"200".into(),kind:"voice".into(),name:"Voz de prueba".into(),..Default::default()};
        let frame=|app:&mut NotDiscordUi,state:&AppSnapshot,events|ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(400.,250.))),events,..Default::default()},|ctx| {
            egui::CentralPanel::default().show(ctx,|ui|app.nav_row(ui,&item,state));
        });
        for _ in 0..3 {frame(&mut app,&state,vec![]);}
        let name=egui::pos2(120.,25.);
        for pressed in [true,false] {frame(&mut app,&state,pointer(name,egui::PointerButton::Primary,pressed));}
        assert_eq!(actions.try_recv().unwrap().kind,"channel");assert!(actions.try_recv().is_err(),"Opening chat alone must not join audio");
        let connect=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new(("guild-voice-join",&item.id)))).unwrap().center();
        for pressed in [true,false] {frame(&mut app,&state,pointer(connect,egui::PointerButton::Primary,pressed));}
        let sent:Vec<_>=std::iter::from_fn(||actions.try_recv().ok()).collect();
        assert_eq!(sent.iter().map(|action|action.kind.as_str()).collect::<Vec<_>>(),["channel","voice"]);assert_eq!(sent[1].id,"200");
        state.voice_connecting=true;
        state.call_channel_id=item.id.clone();
        for pressed in [true,false] {frame(&mut app,&state,pointer(connect,egui::PointerButton::Primary,pressed));}
        assert!(!std::iter::from_fn(||actions.try_recv().ok()).any(|action|action.kind=="voice"));
        state.call_channel_id="201".into();
        frame(&mut app,&state,vec![]);
        for pressed in [true,false] {frame(&mut app,&state,pointer(connect,egui::PointerButton::Primary,pressed));}
        assert!(std::iter::from_fn(||actions.try_recv().ok()).any(|action|action.kind=="voice"),"A different active call must allow an explicit switch");
    }
    #[test]
    fn guild_voice_children_expand_move_and_release_cached_sidebar_space() {
        let (mut app,mut actions)=harness();let ctx=app.ctx.clone();let mut state=state();
        state.selected_guild_id="500".into();
        state.channels=vec![NavItem{id:"200".into(),kind:"voice".into(),name:"Voice A".into(),..Default::default()},
            NavItem{id:"201".into(),kind:"voice".into(),name:"Voice B".into(),..Default::default()}];
        let draw=|app:&mut NotDiscordUi,state:&AppSnapshot|ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1200.,800.))),..Default::default()},|ctx|app.draw_shell(ctx,state));
        for _ in 0..3 {draw(&mut app,&state);}
        let base=app.nav_heights["200"].1;
        state.guild_voice_members.push(notdiscord_core::guild_settings::GuildVoiceMember{guild_id:"500".into(),channel_id:"200".into(),nickname:"Servidor".into(),user:Person{id:"2".into(),display_name:"Global".into(),username:"Account".into(),..Default::default()},..Default::default()});
        let output=draw(&mut app,&state);let first=text(&output,"Servidor").unwrap();
        assert!(app.nav_heights["200"].1>=base+31.);
        state.guild_voice_members[0].channel_id="201".into();
        let output=draw(&mut app,&state);
        assert!((app.nav_heights["200"].1-base).abs()<0.1);
        assert!(text(&output,"Servidor").unwrap().y>first.y);
        state.guild_voice_members.clear();
        let output=draw(&mut app,&state);
        assert!(text(&output,"Servidor").is_none());
        assert!((app.nav_heights["201"].1-base).abs()<0.1);
        assert!(actions.try_recv().is_err());
    }
    fn header_frame(
        app: &mut NotDiscordUi,
        state: &AppSnapshot,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 500.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.draw_header(ui, state));
            },
        )
    }
    fn last_header_icon(output: &egui::FullOutput) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if (rect.rect.width() - 36.).abs() < 0.1
                        && (rect.rect.height() - 36.).abs() < 0.1 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .max_by(|left, right| left.right().total_cmp(&right.right()))
            .expect("header members icon")
            .center()
    }
    #[test]
    fn header_sidebars_toggle_independently_and_persist_across_conversations() {
        let (mut app, _) = harness();
        let mut state = state();
        state.view = "chat".into();
        state.selected_channel_id = "200".into();
        state.selected_channel_kind = "dm".into();
        assert!(!app.prefs.show_dm_profile);
        assert!(app.prefs.show_group_members);
        assert!(app.prefs.show_server_members);
        for kind in ["dm", "group", "server"] {
            state.selected_channel_kind = if kind == "server" { "text" } else { kind }.into();
            state.selected_guild_id = if kind == "server" { "10" } else { "" }.into();
            state.channel_access = ChannelAccess::private();
            header_frame(&mut app, &state, vec![]);
            let output = header_frame(&mut app, &state, vec![]);
            let position = last_header_icon(&output);
            for pressed in [true, false] {
                header_frame(
                    &mut app,
                    &state,
                    pointer(position, egui::PointerButton::Primary, pressed),
                );
            }
        }
        assert!(app.prefs.show_dm_profile);
        assert!(!app.prefs.show_group_members);
        assert!(!app.prefs.show_server_members);
        let saved = Preferences::load(&app.data_dir);
        assert!(saved.show_dm_profile);
        assert!(!saved.show_group_members);
        assert!(!saved.show_server_members);
        state.selected_channel_id = "201".into();
        state.selected_guild_id.clear();
        state.selected_channel_kind = "dm".into();
        header_frame(&mut app, &state, vec![]);
        assert!(app.prefs.show_dm_profile);
        let _ = std::fs::remove_file(app.data_dir.join("ui-settings.json"));
        let _ = std::fs::remove_dir(&app.data_dir);
    }
    #[test]
    fn dm_sidebar_uses_real_identity_requests_once_and_preserves_the_full_profile_view() {
        let (mut app, mut commands) = harness();
        let mut state = state();
        state.view = "chat".into();
        state.selected_channel_id = "200".into();
        state.selected_channel_kind = "dm".into();
        state.direct_messages = vec![NavItem {
            id: "200".into(),
            kind: "dm".into(),
            name: "Real display".into(),
            recipient_id: "100".into(),
            ..Default::default()
        }];
        state.profile_user = Some(Person {
            id: "999".into(),
            name: "Separate full profile".into(),
            ..Default::default()
        });
        let ctx = app.ctx.clone();
        let draw = |app: &mut NotDiscordUi| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(222., 600.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.draw_dm_sidebar(ui, &state));
                },
            )
        };
        let first = draw(&mut app);
        assert!(text(&first, "Real display").is_some());
        let action = commands.try_recv().unwrap();
        assert_eq!(
            (
                action.kind.as_str(),
                action.id.as_str(),
                action.channel_id.as_str()
            ),
            ("dm-sidebar-profile", "100", "200")
        );
        draw(&mut app);
        assert!(commands.try_recv().is_err());
        app.dm_sidebar_loading = false;
        app.dm_sidebar_profile = Some(ProfileDetails {
            user: Person {
                id: "100".into(),
                name: "Real display".into(),
                username: "realuser".into(),
                ..Default::default()
            },
            bio: "Actual profile bio".into(),
            ..Default::default()
        });
        let loaded = draw(&mut app);
        assert!(text(&loaded, "@realuser").is_some());
        assert!(text(&loaded, "Actual profile bio").is_some());
        assert_eq!(state.profile_user.as_ref().unwrap().id, "999");
        assert_eq!(state.view, "chat");
        assert_eq!(app.dm_sidebar_profile.as_ref().unwrap().user.id, "100");
        assert!(commands.try_recv().is_err());
    }
    fn harness() -> (NotDiscordUi, tokio::sync::mpsc::UnboundedReceiver<UiAction>) {
        let (commands, received) = tokio::sync::mpsc::unbounded_channel();
        let (_, updates) = std::sync::mpsc::channel();
        let app = NotDiscordUi::from_context(
            &egui::Context::default(),
            std::env::temp_dir().join(format!("notdiscord-shell-test-{}", uuid::Uuid::new_v4())),
            commands,
            updates,
        );
        (app, received)
    }
    fn state() -> AppSnapshot {
        AppSnapshot {
            signed_in: true,
            current_user: Some(Person {
                id: "self".into(),
                name: "Yo".into(),
                ..Default::default()
            }),
            view: "friends".into(),
            relationships: vec![Relationship {
                user: Person {
                    id: "person".into(),
                    name: "Persona de prueba".into(),
                    username: "persona".into(),
                    ..Default::default()
                },
                kind: 1,
                ..Default::default()
            }],
            ..AppSnapshot::welcome()
        }
    }
    fn frame(
        app: &mut NotDiscordUi,
        state: &AppSnapshot,
        events: Vec<egui::Event>,
        time: f64,
        members: bool,
    ) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600., 400.),
                )),
                events,
                time: Some(time),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if members {
                        app.draw_members(ui, state)
                    } else {
                        app.draw_people(ui, state)
                    }
                });
            },
        )
    }
    fn shape_text(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() / 2.)
            }
            egui::Shape::Vec(children) => {
                children.iter().find_map(|shape| shape_text(shape, label))
            }
            _ => None,
        }
    }
    fn text(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        output
            .shapes
            .iter()
            .find_map(|shape| shape_text(&shape.shape, label))
    }
    fn pointer(
        position: egui::Pos2,
        button: egui::PointerButton,
        pressed: bool,
    ) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn friend_message_button_responds_at_the_right_edge() {
        let (mut app, mut commands) = harness();
        let state = state();
        let _ = frame(&mut app, &state, vec![], 0., false);
        let output = frame(&mut app, &state, vec![], 0.1, false);
        let position = text(&output, "Mensaje").expect("botón Mensaje visible");
        assert!(position.x > 400.);
        let _ = frame(
            &mut app,
            &state,
            pointer(position, egui::PointerButton::Primary, true),
            0.2,
            false,
        );
        let _ = frame(
            &mut app,
            &state,
            pointer(position, egui::PointerButton::Primary, false),
            0.3,
            false,
        );
        let action = commands.try_recv().expect("Mensaje debe emitir open-dm");
        assert_eq!(action.kind, "open-dm");
        assert_eq!(action.id, "person");
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn member_context_menu_opens_on_the_empty_right_side_of_the_row() {
        let (mut app, mut commands) = harness();
        let mut state = state();
        state.selected_channel_kind = "group".into();
        state.group_members = vec![ChannelMember {
            user: Person {
                id: "stranger".into(),
                name: "Miembro de prueba".into(),
                username: "miembro".into(),
                ..Default::default()
            },
            presence: "online".into(),
            ..Default::default()
        }];
        let _ = frame(&mut app, &state, vec![], 0., true);
        let output = frame(&mut app, &state, vec![], 0.1, true);
        let title = text(&output, "Miembro de prueba").expect("fila de miembro visible");
        let position = egui::pos2(560., title.y + 10.);
        let _ = frame(
            &mut app,
            &state,
            pointer(position, egui::PointerButton::Secondary, true),
            0.2,
            true,
        );
        let _ = frame(
            &mut app,
            &state,
            pointer(position, egui::PointerButton::Secondary, false),
            0.3,
            true,
        );
        let output = frame(&mut app, &state, vec![], 0.5, true);
        assert!(text(&output, "Ver perfil").is_some());
        assert!(text(&output, "Enviar mensaje").is_some());
        assert!(text(&output, "Añadir amigo").is_some());
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn private_sections_match_call_pin_and_recent_priority_without_duplicates() {
        let mut state = state();
        state.direct_messages = vec![
            NavItem {
                id: "recent".into(),
                ..Default::default()
            },
            NavItem {
                id: "pinned".into(),
                is_pinned: true,
                ..Default::default()
            },
            NavItem {
                id: "call".into(),
                is_pinned: true,
                has_active_call: true,
                ..Default::default()
            },
        ];
        let rows = navigation_rows(&state);
        let labels = rows
            .into_iter()
            .map(|row| match row {
                NavigationRow::Section(label) => label.to_string(),
                NavigationRow::Item(item) => item.id.clone(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                "LLAMADAS",
                "call",
                "FIJADOS",
                "pinned",
                "MENSAJES DIRECTOS",
                "recent"
            ]
        );
    }
    #[test]
    fn member_role_headers_are_independent_rows_and_offline_members_keep_their_group() {
        let mut state = state();
        state.selected_guild_id = "guild".into();
        state.guild_roles = vec![GuildRole {
            id: "role".into(),
            name: "Equipo".into(),
            color: 0x23a55a,
            ..Default::default()
        }];
        let members = vec![
            ChannelMember {
                user: Person {
                    id: "one".into(),
                    ..Default::default()
                },
                presence: "online".into(),
                hoist_role_id: "role".into(),
                ..Default::default()
            },
            ChannelMember {
                user: Person {
                    id: "two".into(),
                    ..Default::default()
                },
                presence: "offline".into(),
                hoist_role_id: "role".into(),
                ..Default::default()
            },
        ];
        state.guild_members = members.into();
        let projection = list_projection::members(&egui::Context::default(), &state);
        let rows = &projection.rows;
        assert_eq!(rows.len(), 4);
        assert!(matches!(&rows[0],list_projection::MemberRow::Section{label,..}if label=="EQUIPO — 1"));
        assert!(matches!(&rows[2],list_projection::MemberRow::Section{label,..}if label=="DESCONECTADOS — 1"));
    }
    #[test]
    fn online_filter_contains_online_idle_dnd_and_searches_both_names() {
        let mut state = state();
        state.relationships.clear();
        for (id, status, display, username) in [
            ("on", "online", "Pantalla Clara", "nombre.on"),
            ("idle", "idle", "Persona ausente", "nombre.idle"),
            ("dnd", "dnd", "Persona ocupada", "nombre.dnd"),
            ("off", "offline", "Pantalla Oscura", "nombre.off"),
            ("unknown", "", "Desconocida", "sin.estado"),
        ] {
            state.relationships.push(Relationship {
                kind: 1,
                user: Person {
                    id: id.into(),
                    name: display.into(),
                    display_name: display.into(),
                    username: username.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            state.user_presences.insert(
                id.into(),
                UserPresence {
                    status: status.into(),
                    ..Default::default()
                },
            );
        }
        let ids = |filter: FriendsFilter| {
            friends_visible(&state, &filter)
                .into_iter()
                .map(|relation| relation.user.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(FriendsFilter {
                online_only: true,
                ..Default::default()
            }),
            ["on", "idle", "dnd"]
        );
        assert_eq!(
            ids(FriendsFilter {
                online_only: true,
                query: "PANTALLA CLARA".into()
            }),
            ["on"]
        );
        assert_eq!(
            ids(FriendsFilter {
                online_only: true,
                query: "@nombre.idle".into()
            }),
            ["idle"]
        );
        assert!(
            ids(FriendsFilter {
                online_only: true,
                query: "nombre.off".into()
            })
            .is_empty()
        );
        assert_eq!(
            ids(FriendsFilter {
                online_only: false,
                query: "nombre.off".into()
            }),
            ["off"]
        );
        assert_eq!(ids(FriendsFilter::default()).len(), 3);
    }
    #[test]
    fn friends_filter_buttons_and_search_work_together_through_input_events() {
        let (mut app, mut commands) = harness();
        let mut state = state();
        state.relationships.push(Relationship {
            kind: 1,
            user: Person {
                id: "offline".into(),
                name: "Amigo desconectado".into(),
                display_name: "Nombre visible".into(),
                username: "usuario.off".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        state.user_presences.insert(
            "person".into(),
            UserPresence {
                status: "online".into(),
                ..Default::default()
            },
        );
        state.user_presences.insert(
            "offline".into(),
            UserPresence {
                status: "offline".into(),
                ..Default::default()
            },
        );
        let _ = frame(&mut app, &state, vec![], 0., false);
        let output = frame(&mut app, &state, vec![], 0.1, false);
        let connected = text(&output, "Conectados").unwrap();
        let _ = frame(
            &mut app,
            &state,
            pointer(connected, egui::PointerButton::Primary, true),
            0.2,
            false,
        );
        let _ = frame(
            &mut app,
            &state,
            pointer(connected, egui::PointerButton::Primary, false),
            0.3,
            false,
        );
        let output = frame(&mut app, &state, vec![], 0.4, false);
        assert!(text(&output, "Persona de prueba").is_some());
        assert!(text(&output, "Amigo desconectado").is_none());
        let filter_id = egui::Id::new(("friends-filter", "self"));
        app.ctx
            .memory_mut(|memory| memory.request_focus(filter_id.with("search")));
        let _ = frame(
            &mut app,
            &state,
            vec![egui::Event::Text("usuario.off".into())],
            0.5,
            false,
        );
        let output = frame(&mut app, &state, vec![], 0.6, false);
        assert!(text(&output, "CONECTADOS — 0").is_some());
        let all = text(&output, "Todos").unwrap();
        let _ = frame(
            &mut app,
            &state,
            pointer(all, egui::PointerButton::Primary, true),
            0.7,
            false,
        );
        let _ = frame(
            &mut app,
            &state,
            pointer(all, egui::PointerButton::Primary, false),
            0.8,
            false,
        );
        let output = frame(&mut app, &state, vec![], 0.9, false);
        assert!(text(&output, "Amigo desconectado").is_some());
        assert!(text(&output, "Persona de prueba").is_none());
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn message_search_is_absent_in_non_chat_sections() {
        let (mut app, _) = harness();
        let mut state = state();
        state.selected_channel_id = "known-chat".into();
        let ctx = app.ctx.clone();
        for view in ["friends", "requests", "profile", "welcome"] {
            state.view = view.into();
            app.search_open = true;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000., 700.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.draw_header(ui, &state));
                },
            );
            assert!(!app.search_rect.is_positive(), "{view}");
            assert!(!app.search_open);
        }
        state.view = "chat".into();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.draw_header(ui, &state));
            },
        );
        assert!(app.search_rect.is_positive());
    }
    #[test]
    fn conversation_context_menu_does_not_navigate() {
        let (mut app, mut commands) = harness();
        let mut state = state();
        state.selected_channel_id = "original".into();
        let item = NavItem {
            id: "target".into(),
            kind: "dm".into(),
            name: "Conversación destino".into(),
            recipient_id: "person".into(),
            ..Default::default()
        };
        let ctx = app.ctx.clone();
        let mut draw = |events, time| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600., 600.),
                    )),
                    events,
                    time: Some(time),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.nav_row(ui, &item, &state));
                },
            )
        };
        let _ = draw(vec![], 0.);
        let _ = draw(vec![], 0.1);
        let position = egui::pos2(560., 35.);
        let _ = draw(pointer(position, egui::PointerButton::Secondary, true), 0.2);
        let _ = draw(
            pointer(position, egui::PointerButton::Secondary, false),
            0.3,
        );
        let output = draw(vec![], 0.5);
        assert!(text(&output, "Abrir conversación").is_some());
        let pin = text(&output, "Fijar conversación").unwrap();
        assert!(commands.try_recv().is_err());
        let _ = draw(pointer(pin, egui::PointerButton::Primary, true), 0.6);
        let _ = draw(pointer(pin, egui::PointerButton::Primary, false), 0.7);
        let action = commands.try_recv().unwrap();
        assert_eq!(action.kind, "pin-dm");
        assert_eq!(action.id, "target");
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn disconnected_voice_error_remains_visible_without_overlapping_sidebars_or_account() {
        fn painted_text(shape: &egui::Shape, label: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(text) if text.galley.job.text == label =>
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size())),
                egui::Shape::Vec(children) => children.iter().find_map(|shape| painted_text(shape, label)),
                _ => None,
            }
        }
        let (mut app, mut commands) = harness();
        let ctx = app.ctx.clone();
        let mut state = state();
        state.voice_connected = false;
        state.voice_connecting = false;
        state.voice_error = "UDP_TIMEOUT · sin respuesta del transporte".into();
        for (height, zoom) in [(700., 1.), (500., 1.5), (350., 1.)] {
            ctx.set_zoom_factor(zoom);
            let draw = |app: &mut NotDiscordUi, state: &AppSnapshot, events| {
                ctx.run(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200., height))),
                    events,
                    ..Default::default()
                }, |ctx| app.draw_shell(ctx, state))
            };
            for _ in 0..3 { draw(&mut app, &state, vec![]); }
            let output = draw(&mut app, &state, vec![]);
            let bounds = |label| output.shapes.iter().find_map(|shape| painted_text(&shape.shape, label)).expect(label);
            let heading = bounds("Error de conexión de voz");
            let detail = bounds(&state.voice_error);
            let name = bounds("Yo");
            let footer = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("left-footer-rect"))).unwrap();
            assert!(footer.top() >= 0. && footer.bottom() <= height + 1., "{height}: {footer:?}");
            assert!(footer.contains(heading.center()) && footer.intersects(detail));
            assert!(name.top() >= detail.bottom() + 2., "Error must remain above the account: {detail:?} vs {name:?}");
            assert!(text(&output, "Conectando a voz…").is_none());
            assert!(text(&output, "Voz conectada").is_none());
            for key in ["rail-reserved-rect", "navigation-reserved-rect"] {
                let reserved = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new(key))).unwrap();
                assert!(reserved.bottom() <= footer.top() + 1., "{height}/{key}: {reserved:?} vs {footer:?}");
            }
            assert!(commands.try_recv().is_err(), "Rendering an error must not reconnect automatically");
            let dismiss = text(&output, "Cerrar aviso").unwrap();
            for pressed in [true, false] { draw(&mut app, &state, pointer(dismiss, egui::PointerButton::Primary, pressed)); }
            assert_eq!(commands.try_recv().unwrap().kind, "dismiss-voice-error");
            assert!(commands.try_recv().is_err());
            let unchanged = draw(&mut app, &state, vec![]);
            assert!(text(&unchanged, "Error de conexión de voz").is_some(), "The confirmed snapshot owns the dismissal");
            let previous_top = footer.top();
            state.voice_error.clear();
            for _ in 0..3 { draw(&mut app, &state, vec![]); }
            let cleared = draw(&mut app, &state, vec![]);
            assert!(text(&cleared, "Error de conexión de voz").is_none());
            let footer = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("left-footer-rect"))).unwrap();
            assert!(footer.top() > previous_top, "Dismissing the error releases reserved navigation space");
            state.voice_error = "UDP_TIMEOUT · sin respuesta del transporte".into();
        }
    }
    #[test]
    fn guild_voice_activity_changes_release_cached_row_height_and_compact_footer_keeps_destination() {
        let(mut app,mut commands)=harness();let ctx=app.ctx.clone();let mut state=state();
        state.selected_guild_id="50".into();state.guilds=vec![NavItem{id:"50".into(),name:"Fixture guild".into(),kind:"guild".into(),..Default::default()}];
        state.channels=vec![NavItem{id:"20".into(),name:"Voice".into(),kind:"voice".into(),..Default::default()},NavItem{id:"21".into(),name:"Below voice".into(),kind:"text".into(),..Default::default()}];
        let player=Person{id:"2".into(),name:"Voice player".into(),..Default::default()};
        state.guild_voice_members=vec![notdiscord_core::guild_settings::GuildVoiceMember{guild_id:"50".into(),channel_id:"20".into(),user:player.clone(),muted:true,..Default::default()}];
        state.user_presences.insert("2".into(),UserPresence{status:"online".into(),activities:vec![PresenceActivity{kind:0,name:"Fixture game".into(),..Default::default()}],..Default::default()});
        state.voice_connected=true;state.call_channel_id="20".into();state.call_guild_id="50".into();state.call_channel_name="Voice".into();state.call_status_label="Conexión cifrada".into();
        state.call_participants=vec![CallParticipant{user:player,..Default::default()}];
        let draw=|app:&mut NotDiscordUi,state:&AppSnapshot,height|ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1200.,height))),..Default::default()},|ctx|app.draw_shell(ctx,state));
        for _ in 0..3{draw(&mut app,&state,700.);}
        let output=draw(&mut app,&state,700.);assert!(text(&output,"Jugando a Fixture game").is_some());
        let expanded=app.nav_heights["20"].1;let below=text(&output,"Below voice").unwrap().y;
        state.user_presences.get_mut("2").unwrap().activities.clear();
        for _ in 0..3{draw(&mut app,&state,700.);}
        let output=draw(&mut app,&state,700.);assert!(text(&output,"Jugando a Fixture game").is_none());
        assert!(app.nav_heights["20"].1+10.<expanded);assert!(text(&output,"Below voice").unwrap().y+10.<below);
        for _ in 0..3{draw(&mut app,&state,350.);}
        let compact=draw(&mut app,&state,350.);assert!(text(&compact,"Fixture guild / Voice · 1 participante(s)").is_some());
        let footer=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new("left-footer-rect"))).unwrap();
        for key in ["rail-reserved-rect","navigation-reserved-rect"]{let reserved=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new(key))).unwrap();assert!(reserved.bottom()<=footer.top()+1.);}
        assert!(footer.top()>=0.&&footer.bottom()<=351.);
        state.voice_connected=false;state.call_participants.clear();state.guild_voice_members.clear();
        for _ in 0..3{draw(&mut app,&state,700.);}
        let closed=draw(&mut app,&state,700.);assert!(text(&closed,"Fixture guild / Voice · 0 participante(s)").is_none());
        assert!(commands.try_recv().is_err(),"Rendering live voice/activity updates must not emit call or sound actions");
    }
}
