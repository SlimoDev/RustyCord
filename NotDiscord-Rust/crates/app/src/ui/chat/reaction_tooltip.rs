//! Reaction readers are requested only after a stationary hover. This cache is
//! scoped to the signed-in session; it never persists people or hover state.
use super::*;

const DWELL: f64 = 1.2;
const TTL: f64 = 30.;
const TIMEOUT: f64 = 10.;
const CAPACITY: usize = 32;
const TRANSIT_GRACE: f64 = 0.35;
fn cache_id() -> egui::Id { egui::Id::new("reaction-users-tooltip") }
fn panel_bounds_id() -> egui::Id { egui::Id::new("reaction-users-panel-bounds") }
#[derive(Clone, Default)]
struct PanelBounds { rect: Option<egui::Rect>, frame: u64 }
pub(in super::super) fn clear_reaction_users(ctx: &egui::Context) {
    ctx.data_mut(|data| {data.remove::<Cache>(cache_id());data.remove::<PanelBounds>(panel_bounds_id());});
}

#[derive(Clone, Default, PartialEq)]
struct Scope { owner: String, generation: u64, channel: String, view: String }
#[derive(Clone, PartialEq)]
struct Key { channel: String, message: String, emoji: String, count: i32, kind: u8 }
impl Key {
    fn same_reaction(&self, other: &Self) -> bool {
        self.channel == other.channel && self.message == other.message
            && self.emoji == other.emoji
    }
}
#[derive(Clone)]
enum ResultState { Loading, Ready { users: Vec<Person>, more: bool }, Error(String) }
#[derive(Clone)]
struct Entry { key: Key, at: f64, result: ResultState }
#[derive(Clone)]
struct Pending { id: String, key: Key, since: f64 }
#[derive(Clone)]
struct Hover {
    key: Key, origin: egui::Pos2, since: f64, open: bool,
    anchor: egui::Rect, popup: egui::Rect, seen: u64, last_inside: f64,
}
#[derive(Clone, Default)]
struct Cache { scope: Scope, entries: Vec<Entry>, pending: Option<Pending>, hover: Option<Hover> }
fn store_cache(ctx: &egui::Context, cache: Cache) {
    let frame=ctx.cumulative_frame_nr();
    let rect=cache.hover.as_ref().filter(|hover|hover.open&&hover.popup.is_positive()).map(|hover|hover.popup);
    ctx.data_mut(|data| {
        data.insert_temp(panel_bounds_id(),PanelBounds {rect,frame});
        data.insert_temp(cache_id(),cache);
    });
}
pub(in super::super) fn reaction_panel_rect(ctx: &egui::Context) -> Option<egui::Rect> {
    let frame=ctx.cumulative_frame_nr();
    ctx.data(|data|data.get_temp::<PanelBounds>(panel_bounds_id()))
        .filter(|panel|frame<=panel.frame.saturating_add(1)).and_then(|panel|panel.rect)
}
pub(in super::super) fn reaction_panel_open(ctx: &egui::Context) -> bool {
    reaction_panel_rect(ctx).is_some()
}
fn over_surface(hover: &Hover, position: egui::Pos2) -> bool {
    hover.anchor.expand(3.).contains(position)||hover.open&&hover.popup.expand(3.).contains(position)
}
fn in_hover(hover: &Hover, position: egui::Pos2, now: f64) -> bool {
    // The same corridor is used before and during drawing. A small gap or
    // viewport flip must not close the panel while the pointer travels to it.
    let area=if hover.open&&hover.popup.is_positive() {hover.anchor.union(hover.popup)} else {hover.anchor};
    over_surface(hover,position)||now-hover.last_inside<=TRANSIT_GRACE&&area.expand(6.).contains(position)
}
fn read_revoked(state: &AppSnapshot, channel: &str) -> bool {
    let access = if channel == state.selected_channel_id { Some(&state.channel_access) }
        else { state.channels.iter().chain(&state.direct_messages)
            .find(|item| item.id == channel).and_then(|item| item.access.as_ref()) };
    access.is_some_and(|access| access.known && (!access.can_view || !access.can_read_history))
}

pub(super) fn begin_frame(ctx: &egui::Context, state: &AppSnapshot) {
    let scope = Scope { owner: state.current_user.as_ref().map_or(String::new(), |person| person.id.clone()),
        generation: state.notification_generation, channel: state.selected_channel_id.clone(), view: state.view.clone() };
    let (now, pointer, focused, escaped) = ctx.input(|input|
        (input.time, input.pointer.hover_pos(), input.focused, input.key_pressed(egui::Key::Escape)));
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<Cache>(cache_id());
        if cache.scope != scope || !state.signed_in || !matches!(state.view.as_str(), "chat" | "pins" | "search") {
            *cache = Cache { scope, ..Default::default() };
        }
        cache.entries.retain(|entry| now - entry.at < TTL && !read_revoked(state, &entry.key.channel)
            && !known_changed(state,&entry.key));
        if cache.pending.as_ref().is_some_and(|pending| now - pending.since >= TIMEOUT) {
            if let Some(pending) = cache.pending.take() {
                cache.entries.retain(|entry| entry.key != pending.key);
                cache.entries.push(Entry { key: pending.key, at: now,
                    result: ResultState::Error("La consulta tardó demasiado. Vuelve a intentarlo en unos segundos.".into()) });
            }
        }
        if let Some(hover)=&mut cache.hover
            && pointer.is_some_and(|position|over_surface(hover,position)) {hover.last_inside=now;}
        if cache.hover.as_ref().is_some_and(|hover| !focused || read_revoked(state, &hover.key.channel)
            || known_removed(state,&hover.key)
            || escaped
            || frame > hover.seen.saturating_add(1)
            || pointer.is_none_or(|position| !in_hover(hover,position,now))) {
            cache.hover = None;
        }
        trim(cache);
        let rect=cache.hover.as_ref().filter(|hover|hover.open&&hover.popup.is_positive()).map(|hover|hover.popup);
        data.insert_temp(panel_bounds_id(),PanelBounds {rect,frame});
    });
}
fn known_removed(state: &AppSnapshot, key: &Key) -> bool {
    state.messages.iter().chain(&state.pinned_messages).find(|message|
        message.id==key.message&&message.channel_id==key.channel).is_some_and(|message|
        message.deleted||!message.reactions.iter().any(|reaction|reaction.emoji.reaction_key()==key.emoji))
}
fn known_changed(state: &AppSnapshot, key: &Key) -> bool {
    state.messages.iter().chain(&state.pinned_messages).find(|message|
        message.id==key.message&&message.channel_id==key.channel).is_none_or(|message|
        message.deleted||!message.reactions.iter().any(|reaction|
            reaction.emoji.reaction_key()==key.emoji&&reaction.count==key.count))
}
fn trim(cache: &mut Cache) {
    while cache.entries.len() > CAPACITY {
        let oldest = cache.entries.iter().enumerate()
            .filter(|(_, entry)| cache.pending.as_ref().is_none_or(|pending| pending.key != entry.key))
            .min_by(|(_, left), (_, right)| left.at.total_cmp(&right.at)).map(|(index, _)| index);
        if let Some(index) = oldest { cache.entries.remove(index); } else { break; }
    }
}

pub(in super::super) fn reaction_users_result(ctx: &egui::Context, value: &Value) {
    let now = ctx.input(|input| input.time);
    ctx.data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<Cache>(cache_id());
        if value["event"]=="invalidate" {
            let channel=value["channelId"].as_str().unwrap_or("");let message=value["messageId"].as_str().unwrap_or("");
            let emoji=value["emoji"].as_str().unwrap_or("");let id=value["emojiId"].as_str().unwrap_or("");
            let matches=|key:&Key|key.channel==channel&&key.message==message&&(value["all"]==true||key.emoji==emoji
                ||!id.is_empty()&&key.emoji.rsplit_once(':').is_some_and(|(_,value)|value==id));
            cache.entries.retain(|entry|!matches(&entry.key));
            if cache.pending.as_ref().is_some_and(|pending|matches(&pending.key)) {cache.pending=None;}
            return;
        }
        let Some(pending) = cache.pending.as_ref() else { return; };
        if value["requestId"].as_str() != Some(pending.id.as_str())
            || value["channelId"].as_str() != Some(pending.key.channel.as_str())
            || value["messageId"].as_str() != Some(pending.key.message.as_str())
            || value["emoji"].as_str() != Some(pending.key.emoji.as_str())
            || value["kind"].as_u64() != Some(u64::from(pending.key.kind)) { return; }
        let pending = cache.pending.take().unwrap();
        cache.entries.retain(|entry| entry.key != pending.key);
        if value["invalidated"]==true {return;}
        // Leaving the reaction cancels its visible query. Its late response
        // releases the single request slot without repopulating another view.
        if cache.hover.as_ref().is_none_or(|hover| hover.key != pending.key) { return; }
        let error = value["error"].as_str().unwrap_or("");
        let result = if !error.is_empty() {
            ResultState::Error(error.chars().take(240).collect())
        } else if let Ok(mut users) = serde_json::from_value::<Vec<Person>>(value["users"].clone()) {
            let mut seen=std::collections::HashSet::new();
            users.retain(|person|notdiscord_core::parsing::valid_id(&person.id)&&seen.insert(person.id.clone()));
            users.truncate(100);
            ResultState::Ready { users, more: value["hasMore"].as_bool().unwrap_or(false) }
        } else { ResultState::Error("No se pudieron leer los usuarios de esta reacción.".into()) };
        cache.entries.push(Entry { key: pending.key, at: now, result });
        trim(cache);
    });
    ctx.request_repaint();
}

impl NotDiscordUi {
    pub(super) fn reaction_tooltip(&mut self, ui: &mut egui::Ui, response: &egui::Response,
        message: &ChatMessage, reaction: &MessageReaction, state: &AppSnapshot) {
        if !state.signed_in || message.deleted || read_revoked(state, &message.channel_id) { return; }
        let now = ui.input(|input| input.time);
        let pointer = ui.input(|input| input.pointer.hover_pos());
        let on_button = response.enabled() && response.contains_pointer() && response.rect.intersects(ui.clip_rect());
        let active = ui.ctx().data_mut(|data| data.get_temp_mut_or_default::<Cache>(cache_id())
            .hover.as_ref().is_some_and(|hover| hover.key.channel == message.channel_id
                && hover.key.message == message.id
                && hover.key.emoji == reaction.emoji.reaction_key()));
        if !on_button && !active { return; }
        // The cache can retain 3,200 people. Move it only for the active
        // reaction; cloning it for every visible button would multiply that
        // work on each frame even when the pointer is elsewhere.
        let mut cache = ui.ctx().data_mut(|data| data.remove_temp::<Cache>(cache_id())).unwrap_or_default();
        if !response.rect.intersects(ui.clip_rect()) {
            cache.hover = None;
            store_cache(ui.ctx(),cache);
            return;
        }
        let key = Key { channel: message.channel_id.clone(), message: message.id.clone(),
            emoji: reaction.emoji.reaction_key(), count: reaction.count, kind: 0 };
        let same = cache.hover.as_ref().is_some_and(|hover| hover.key.same_reaction(&key));
        if on_button && !same {
            if let Some(position) = pointer {
                cache.hover = Some(Hover { key: key.clone(), origin: position, since: now, open: false,
                    anchor: response.rect, popup: egui::Rect::NOTHING, seen: ui.ctx().cumulative_frame_nr(), last_inside: now });
            }
        }
        let Some(hover) = cache.hover.as_mut() else {
            store_cache(ui.ctx(),cache);
            return;
        };
        if !hover.key.same_reaction(&key) {
            store_cache(ui.ctx(),cache);
            return;
        }
        let inside_popup = hover.open && pointer.is_some_and(|position|in_hover(hover,position,now));
        if !on_button && !inside_popup {
            cache.hover = None;
            store_cache(ui.ctx(),cache);
            return;
        }
        hover.anchor = response.rect;
        hover.seen = ui.ctx().cumulative_frame_nr();
        if pointer.is_some_and(|position|over_surface(hover,position)) {hover.last_inside=now;}
        if hover.key.count!=reaction.count {
            hover.key.count=reaction.count;
            cache.entries.retain(|entry|!entry.key.same_reaction(&key));
            if cache.pending.as_ref().is_some_and(|pending|pending.key.same_reaction(&key)) {cache.pending=None;}
        }
        if !hover.open {
            if pointer.is_some_and(|position| position.distance(hover.origin) > 3.)
                || ui.input(|input| input.pointer.any_down()) {
                hover.origin = pointer.unwrap_or(hover.origin); hover.since = now;
            }
            let remaining = (DWELL - (now - hover.since)).max(0.);
            if remaining > 0. {
                ui.ctx().request_repaint_after(Duration::from_secs_f64(remaining));
                store_cache(ui.ctx(),cache);
                return;
            }
            hover.open = true;
        }
        let key = hover.key.clone();
        cache.entries.retain(|entry| !matches!(&entry.result,ResultState::Loading)
            || cache.pending.as_ref().is_some_and(|pending|pending.key==entry.key));
        let entry = cache.entries.iter().find(|entry| entry.key == key && now - entry.at < TTL);
        let query_available=cache.pending.as_ref().is_none_or(|pending|
            !pending.key.same_reaction(&key)||pending.key.count!=key.count);
        if entry.is_none() && query_available {
            let id = uuid::Uuid::new_v4().to_string();
            let action = UiAction { kind: "reaction-users".into(), id: key.message.clone(),
                channel_id: key.channel.clone(), text: key.emoji.clone(),
                extra: serde_json::json!({"requestId":id,"kind":key.kind}).to_string() };
            let result = if self.command.send(action).is_ok() {
                cache.pending = Some(Pending { id, key: key.clone(), since: now });
                ResultState::Loading
            } else { ResultState::Error("El servicio de la aplicación se cerró.".into()) };
            cache.entries.push(Entry { key: key.clone(), at: now, result });
            trim(&mut cache);
        }
        let result = cache.entries.iter().find(|entry| entry.key == key).map(|entry| &entry.result);
        let mut selected_kind = key.kind;
        let mut retry=false;let mut scrollable=false;
        let bounds=ui.ctx().content_rect().shrink(4.);
        let width=290_f32.min(bounds.width().max(1.));
        let rows=match result {Some(ResultState::Ready{users,..})=>users.len().min(7),_=>0};
        let list_height=(bounds.height()-90.).clamp(1.,240.);
        let error_height=(bounds.height()-125.).clamp(1.,84.);
        let height=match result {
            Some(ResultState::Error(_))=>125.+error_height,
            _ if rows>0=>80.+(rows as f32*36.).min(list_height),
            _=>126.,
        }.min(bounds.height().max(1.));
        let anchor=cache.hover.as_ref().unwrap().anchor;
        let top=if anchor.bottom()+6.+height<=bounds.bottom(){anchor.bottom()+6.}else{anchor.top()-6.-height};
        let position=egui::pos2(anchor.left().clamp(bounds.left(),(bounds.right()-width).max(bounds.left())),
            top.clamp(bounds.top(),(bounds.bottom()-height).max(bounds.top())));
        let panel_id=egui::Id::new(("reaction-people-panel",&cache.scope.owner,cache.scope.generation,
            &cache.scope.channel,&cache.scope.view,&key.channel,&key.message,&key.emoji));
        let panel=egui::Area::new(panel_id).order(egui::Order::Tooltip).fixed_pos(position)
            .movable(false).interactable(true).sense(egui::Sense::hover()).fade_in(false).constrain_to(bounds)
            .show(ui.ctx(),|ui| {
                egui::Frame::popup(ui.style()).show(ui,|ui| {
                    ui.set_width((width-16.).max(1.));
                    self.emoji_section(ui,("reaction-panel-emojis",panel_id),|this,ui| {
                        ui.horizontal(|ui| {
                            this.emoji(ui,&reaction.emoji,24.);
                            let title=if reaction.emoji.id.is_empty(){"Reacciones"}else{&reaction.emoji.name};
                            ui.add(egui::Label::new(RichText::new(title).strong()).selectable(false).truncate());
                        });
                        ui.horizontal_wrapped(|ui| {
                            for (kind,label) in [(0,"Normales"),(1,"Superreacciones")] {
                                if ui.selectable_label(key.kind==kind,label).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                    selected_kind=kind;ui.ctx().request_repaint();
                                }
                            }
                        });
                        ui.separator();
                        match result {
                            None|Some(ResultState::Loading)=>{ui.horizontal(|ui|{ui.spinner();ui.label("Cargando usuarios…");});}
                            Some(ResultState::Error(error))=>{
                                let scrolled=theme::vertical_scroll(ui).id_salt(("reaction-error",panel_id))
                                    .max_height(error_height).show(ui,|ui| {ui.label(error);});
                                scrollable=scrolled.content_size.y>scrolled.inner_rect.height()+0.5;
                                retry=ui.button("Reintentar").clicked();
                            }
                            Some(ResultState::Ready{users,more})=>{
                                if users.is_empty(){ui.label("Sin usuarios disponibles para este tipo de reacción.");}
                                let scrolled=theme::vertical_scroll(ui).id_salt(("reaction-people",&key.channel,&key.message,&key.emoji,key.kind))
                                    .max_height(list_height).show(ui,|ui| {
                                        for person in users {
                                            let name=if message.channel_id==state.selected_channel_id {state.guild_display_name(person)}
                                                else {super::super::mentions::person_name(person)};
                                            let (rect,row)=ui.allocate_exact_size(egui::vec2(ui.available_width(),34.),egui::Sense::click());
                                            let row=row.on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if row.hovered(){ui.painter().rect_filled(rect,4.,this.theme.surface);}
                                            let mut avatar_clicked=false;
                                            ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(3.,2.)))
                                                .layout(egui::Layout::left_to_right(egui::Align::Center)),|ui| {
                                                avatar_clicked=this.avatar(ui,person,"",28.).clicked();
                                                ui.add(egui::Label::new(name).selectable(false).sense(egui::Sense::hover()).truncate());
                                                theme::highlighted_control_feedback(&row);
                                            });
                                            if row.clicked()||avatar_clicked {this.send("profile",&person.id,"","",&message.channel_id);}
                                        }
                                        if *more {ui.weak("Se muestran hasta 100 usuarios.");}
                                    });
                                scrollable=scrolled.content_size.y>scrolled.inner_rect.height()+0.5;
                            }
                        }
                    });
                });
            });
        if let Some(hover)=&mut cache.hover {hover.popup=panel.response.rect;hover.key.kind=selected_kind;}
        if retry {cache.entries.retain(|entry|entry.key!=key);ui.ctx().request_repaint();}
        if scrollable&&pointer.is_some_and(|position|panel.response.rect.contains(position)) {
            // Consume any remainder at the list's boundaries too. The anchor
            // itself is outside this rect, so its wheel still scrolls history.
            ui.ctx().input_mut(|input|{input.smooth_scroll_delta=egui::Vec2::ZERO;input.raw_scroll_delta=egui::Vec2::ZERO;});
        }
        let remaining = cache.pending.as_ref().map(|pending| (TIMEOUT - (now - pending.since)).max(0.));
        let transit_remaining=cache.hover.as_ref().filter(|hover|hover.open
            &&pointer.is_some_and(|position|!over_surface(hover,position)))
            .map(|hover|(TRANSIT_GRACE-(now-hover.last_inside)).max(0.));
        store_cache(ui.ctx(),cache);
        if let Some(remaining) = remaining {
            ui.ctx().request_repaint_after(Duration::from_secs_f64(remaining));
        }
        if let Some(remaining)=transit_remaining {ui.ctx().request_repaint_after(Duration::from_secs_f64(remaining));}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reaction_people_cache_bounds_entries_and_keeps_the_single_active_request() {
        let mut cache=Cache::default();
        for index in 0..80 {
            let key=Key {channel:"20".into(),message:index.to_string(),emoji:"♥".into(),count:1,kind:0};
            if index==0 {cache.pending=Some(Pending {id:"pending".into(),key:key.clone(),since:0.});}
            cache.entries.push(Entry {key,at:f64::from(index),result:ResultState::Loading});
            trim(&mut cache);
        }
        assert_eq!(cache.entries.len(),CAPACITY);
        assert!(cache.entries.iter().any(|entry|entry.key.message=="0"));
        assert!(cache.entries.iter().any(|entry|entry.key.message=="79"));
        assert!(!cache.entries.iter().any(|entry|entry.key.message=="1"));
        let ctx=egui::Context::default();
        ctx.data_mut(|data|data.insert_temp(cache_id(),cache));
        clear_reaction_users(&ctx);
        assert!(ctx.data(|data|data.get_temp::<Cache>(cache_id())).is_none());
    }
    #[test]
    fn reaction_people_require_confirmed_revocations_not_missing_cross_channel_metadata() {
        let mut state=AppSnapshot {selected_channel_id:"20".into(),channel_access:ChannelAccess::private(),..Default::default()};
        assert!(!read_revoked(&state,"20"));
        assert!(!read_revoked(&state,"21"));
        state.channels=vec![NavItem {id:"21".into(),access:Some(ChannelAccess::default()),..Default::default()}];
        assert!(!read_revoked(&state,"21"));
        state.channels[0].access=Some(ChannelAccess {known:true,..Default::default()});
        assert!(read_revoked(&state,"21"));
        state.channel_access.can_read_history=false;
        assert!(read_revoked(&state,"20"));
    }
}
