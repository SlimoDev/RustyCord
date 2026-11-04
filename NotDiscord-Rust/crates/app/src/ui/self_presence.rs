//! One ephemeral presentation intention per account/session. Confirmed policy
//! remains in AppSnapshot::self_presence and never follows this optimistic lane.
use super::*;

#[derive(Clone)]
struct Intent {
    account: String,
    generation: u64,
    revision: u64,
    status: String,
    phase: String,
}
#[derive(Clone)]
struct RemoteMismatch {
    account: String,
    generation: u64,
    revision: u64,
    status: String,
    checking: bool,
}
#[derive(Clone, Default)]
struct Visual {
    intent: Option<Intent>,
    remote: Option<RemoteMismatch>,
    remote_seen: Option<(String,u64,u64)>,
    next: u64,
    dirty: bool,
}
#[derive(Clone)]
struct MemberProjection {
    account: String,
    generation: u64,
    status: String,
    source: SharedList<ChannelMember>,
    projected: SharedList<ChannelMember>,
}
fn same_members(left: &SharedList<ChannelMember>, right: &SharedList<ChannelMember>) -> bool {
    left.len() == right.len() && left.as_ptr() == right.as_ptr()
}
fn members_key() -> egui::Id { egui::Id::new("self-presence-member-projection") }
fn key() -> egui::Id { egui::Id::new("self-presence-visual") }
fn visual(ctx: &egui::Context) -> Visual { ctx.data(|data| data.get_temp::<Visual>(key())).unwrap_or_default() }
fn current(intent: &Intent, state: &AppSnapshot) -> bool {
    state.signed_in && intent.generation == state.notification_generation
        && state.current_user.as_ref().is_some_and(|user| user.id == intent.account)
}
fn store(ctx: &egui::Context, value: Visual) { ctx.data_mut(|data| data.insert_temp(key(), value)); }

pub(super) fn project_snapshot(ctx: &egui::Context, state: &mut AppSnapshot) {
    let mut value = visual(ctx);
    if value.intent.as_ref().is_some_and(|intent| !current(intent,state)) { value.intent = None; }
    if value.remote.as_ref().is_some_and(|remote|!state.signed_in || remote.generation != state.notification_generation
        || state.current_user.as_ref().is_none_or(|user|user.id != remote.account)) { value.remote = None; }
    if value.remote_seen.as_ref().is_some_and(|(account,generation,_)|!state.signed_in || *generation != state.notification_generation
        || state.current_user.as_ref().is_none_or(|user|&user.id != account)) { value.remote_seen = None; }
    if value.intent.as_ref().is_some_and(|intent| intent.phase == "confirmed" && intent.status == state.self_presence) { value.intent = None; }
    let status = value.intent.as_ref().map(|intent| intent.status.clone()).unwrap_or_else(|| state.self_presence.clone());
    let source = state.guild_members.clone();
    let account = state.current_user.as_ref().map(|user|user.id.clone()).unwrap_or_default();
    let cache = ctx.data(|data|data.get_temp::<MemberProjection>(members_key()));
    if let Some(cache) = cache.as_ref().filter(|cache|cache.account == account
        && cache.generation == state.notification_generation && cache.status == status
        && (same_members(&source,&cache.source) || same_members(&source,&cache.projected))) {
        state.guild_members = cache.projected.clone();
    }
    crate::controller::presence::project_status(state,&status);
    if value.intent.is_some() {
        let projected = state.guild_members.clone();
        let source = cache.as_ref().filter(|cache|same_members(&source,&cache.projected))
            .map(|cache|cache.source.clone()).unwrap_or(source);
        let cache = MemberProjection { account,generation:state.notification_generation,status,source,projected };
        ctx.data_mut(|data|data.insert_temp(members_key(),cache));
    } else { ctx.data_mut(|data|data.remove::<MemberProjection>(members_key())); }
    value.dirty = false;
    store(ctx,value);
}

impl NotDiscordUi {
    pub(super) fn visual_presence(&self, state: &AppSnapshot) -> String {
        visual(&self.ctx).intent.filter(|intent| current(intent,state)).map(|intent| intent.status).unwrap_or_else(|| state.self_presence.clone())
    }
    pub(super) fn choose_presence(&mut self, state: &AppSnapshot, status: &str) {
        if !state.signed_in || !notdiscord_core::settings_proto::valid_presence(status) { return; }
        let Some(account) = state.current_user.as_ref().map(|user| user.id.clone()).filter(|id| !id.is_empty()) else { return; };
        let mut value = visual(&self.ctx);
        if value.intent.as_ref().is_some_and(|intent| current(intent,state) && intent.status == status && intent.phase == "pending") { return; }
        value.next = value.next.wrapping_add(1).max(1);
        let action = UiAction { kind:"status".into(),id:value.next.to_string(),text:status.into(),
            extra:serde_json::json!({"account_id":account,"generation":state.notification_generation}).to_string(),..Default::default() };
        if self.command.send(action).is_err() { self.error = "El servicio de la aplicación se cerró.".into(); return; }
        // Set the intention only after local command admission succeeds.
        value.remote = None;
        value.intent = Some(Intent { account,generation:state.notification_generation,revision:value.next,status:status.into(),phase:"pending".into() });
        value.dirty = true;
        store(&self.ctx,value);
        self.ctx.request_repaint();
    }
    pub(super) fn project_visual_presence(&mut self) {
        if visual(&self.ctx).dirty { project_snapshot(&self.ctx,&mut self.state); }
    }
    pub(super) fn presence_result(&mut self, result: &Value) {
        if result["revision"].as_str().is_some_and(|revision|revision.starts_with("remote:")) { self.remote_presence_result(result); return; }
        let mut value = visual(&self.ctx);
        let Some(intent) = value.intent.as_mut() else { return; };
        if !current(intent,&self.state) || result["account_id"].as_str() != Some(intent.account.as_str())
            || result["generation"].as_u64() != Some(intent.generation)
            || result["revision"].as_str().and_then(|revision|revision.parse::<u64>().ok()) != Some(intent.revision) { return; }
        match result["outcome"].as_str().unwrap_or("") {
            outcome @ ("confirmed" | "checked" | "persisted") => {
                intent.phase = if outcome == "persisted" {"persisted"}else{"confirmed"}.into();
                if let Some(status) = result["status"].as_str().filter(|status|notdiscord_core::settings_proto::valid_presence(status)) { intent.status = status.into(); }
            }
            "rejected" | "rate-limited" => { value.intent = None; }
            phase @ ("unknown" | "not-sent" | "checked-unknown" | "check-failed") => { intent.phase = phase.into(); }
            _ => return,
        }
        value.dirty = true;
        store(&self.ctx,value);
        self.ctx.request_repaint();
    }
    pub(super) fn presence_discrepancy(&mut self, event: &Value) {
        let account = s(event,"account_id");
        let Some(generation) = event["generation"].as_u64() else { return; };
        let Some(revision) = event["revision"].as_u64() else { return; };
        if !self.state.signed_in || generation != self.state.notification_generation
            || self.state.current_user.as_ref().is_none_or(|user|user.id != account) { return; }
        let mut value = visual(&self.ctx);
        if value.remote_seen.as_ref().is_some_and(|(seen_account,seen_generation,seen_revision)|seen_account == account && *seen_generation == generation && *seen_revision >= revision) { return; }
        value.remote = match event["status"].as_str().filter(|status|notdiscord_core::settings_proto::valid_presence(status)) {
            Some(status) => Some(RemoteMismatch{account:account.into(),generation,revision,status:status.into(),checking:false}),
            None if event["status"].is_null() => None,
            _ => return,
        };
        value.remote_seen = Some((account.into(),generation,revision));
        store(&self.ctx,value); self.ctx.request_repaint();
    }
    fn remote_presence_result(&mut self, result: &Value) {
        let mut value = visual(&self.ctx);
        if value.intent.is_some() { return; }
        let Some(remote) = value.remote.as_mut() else { return; };
        let revision = result["revision"].as_str().and_then(|revision|revision.strip_prefix("remote:")).and_then(|revision|revision.parse::<u64>().ok());
        if !self.state.signed_in || self.state.notification_generation != remote.generation
            || self.state.current_user.as_ref().is_none_or(|user|user.id != remote.account)
            || result["account_id"].as_str() != Some(remote.account.as_str())
            || result["generation"].as_u64() != Some(remote.generation) || revision != Some(remote.revision) { return; }
        match result["outcome"].as_str() {
            Some("checked") => value.remote = None,
            Some("check-failed") => remote.checking = false,
            _ => return,
        }
        store(&self.ctx,value); self.ctx.request_repaint();
    }
    fn check_remote_presence(&mut self, state: &AppSnapshot) {
        let mut value = visual(&self.ctx);
        if value.intent.is_some() { return; }
        let Some(remote) = value.remote.as_mut().filter(|remote|!remote.checking && state.signed_in
            && remote.generation == state.notification_generation && state.current_user.as_ref().is_some_and(|user|user.id == remote.account)) else { return; };
        let action = UiAction{kind:"presence-check".into(),id:format!("remote:{}",remote.revision),
            extra:serde_json::json!({"account_id":remote.account,"generation":remote.generation}).to_string(),..Default::default()};
        if self.command.send(action).is_err() { self.error = "El servicio de la aplicación se cerró.".into(); return; }
        remote.checking = true; store(&self.ctx,value); self.ctx.request_repaint();
    }
    fn remote_presence_note(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        let value = visual(&self.ctx);
        if value.intent.is_some() { return; }
        let Some(remote) = value.remote.filter(|remote|state.signed_in
            && remote.generation == state.notification_generation && state.current_user.as_ref().is_some_and(|user|user.id == remote.account)) else { return; };
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::Label::new(RichText::new("Estado remoto diferente, sin confirmar").small().color(self.theme.muted)))
                .on_hover_text(format!("Se recibió «{}». Puedes comprobar el estado guardado sin volver a enviarlo.",presence_label(&remote.status)));
            if ui.add_enabled(!remote.checking,egui::Button::new(if remote.checking {"Comprobando…"}else{"Comprobar"}).small()).clicked() {
                self.check_remote_presence(state);
            }
        });
    }
    pub(super) fn presence_note(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        let Some(intent) = visual(&self.ctx).intent.filter(|intent|current(intent,state) && intent.phase != "confirmed") else {
            self.remote_presence_note(ui,state); return;
        };
        let label = match intent.phase.as_str() {
            "pending" => "Cambiando estado…",
            "not-sent" => "Estado pendiente de envío",
            "persisted" => "Estado guardado; presencia sin confirmar",
            "checked-unknown" => "Estado comprobado; envío anterior sin confirmar",
            "check-failed" => "No se pudo comprobar el estado",
            _ => "Estado sin confirmar",
        };
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::Label::new(RichText::new(label).small().color(self.theme.muted)));
            if intent.phase == "persisted" && ui.small_button("Reintentar presencia").on_hover_text("Reintenta sólo la emisión de presencia; no repite el ajuste guardado.").clicked() {
                self.choose_presence(state,&intent.status);
            }
            if intent.phase != "pending" && ui.small_button("Comprobar").on_hover_text("Consulta una vez el estado guardado; no vuelve a enviarlo.").clicked() {
                let extra = serde_json::json!({"account_id":intent.account,"generation":intent.generation}).to_string();
                self.send("presence-check",intent.revision.to_string(),"",extra,"");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{harness,signed_in};

    #[test]
    fn visual_choice_projects_all_own_rows_without_changing_confirmed_policy() {
        let (mut app,_,mut actions) = harness();
        let mut state = signed_in("42"); state.notification_generation=7; state.self_presence="online".into();
        state.guild_members=vec![ChannelMember{user:Person{id:"42".into(),..Default::default()},presence:"online".into(),..Default::default()}].into();
        state.group_members=state.guild_members.to_vec();
        app.state=state.clone();
        app.choose_presence(&state,"dnd");
        let action=actions.try_recv().unwrap(); assert_eq!(action.kind,"status");
        app.project_visual_presence();
        assert_eq!(app.state.self_presence,"online"); assert_eq!(app.state.notification_epoch,state.notification_epoch);
        assert_eq!(app.visual_presence(&app.state),"dnd");
        assert_eq!(app.state.guild_members[0].presence,"dnd"); assert_eq!(app.state.group_members[0].presence,"dnd");
        assert_eq!(app.state.user_presences["42"].status,"dnd");
        let mut another=state.clone();
        project_snapshot(&app.ctx,&mut another);
        assert_eq!(app.state.guild_members.as_ptr(),another.guild_members.as_ptr(),"unchanged source reuses the projected member storage");
        // An older completion, policy barrier and snapshot cannot erase idle.
        app.choose_presence(&state,"idle"); let newest=actions.try_recv().unwrap();
        app.presence_result(&serde_json::json!({"account_id":"42","generation":7,"revision":action.id,"outcome":"confirmed","status":"dnd"}));
        project_snapshot(&app.ctx,&mut state);
        assert_eq!(state.guild_members[0].presence,"idle"); assert_eq!(state.self_presence,"online");
        app.presence_result(&serde_json::json!({"account_id":"42","generation":7,"revision":newest.id,"outcome":"rejected","status":"online"}));
        app.project_visual_presence(); assert_eq!(app.state.guild_members[0].presence,"online");
    }
    #[test]
    fn closed_command_lane_creates_no_visual_intent_and_account_change_clears_it() {
        let (mut app,_,actions)=harness(); let state=signed_in("42");
        drop(actions); app.choose_presence(&state,"idle"); assert!(visual(&app.ctx).intent.is_none());
        let (mut app,_,_actions)=harness(); app.state=state.clone(); app.choose_presence(&state,"idle");
        let mut other=signed_in("43"); project_snapshot(&app.ctx,&mut other);
        assert!(visual(&app.ctx).intent.is_none());
    }
    #[test]
    fn repeated_pending_choice_is_coalesced_and_uncertainty_survives_old_snapshots() {
        let (mut app,_,mut actions)=harness(); let state=signed_in("42"); app.state=state.clone();
        app.choose_presence(&state,"idle"); let action=actions.try_recv().unwrap();
        app.choose_presence(&state,"idle"); assert!(actions.try_recv().is_err());
        app.presence_result(&serde_json::json!({"account_id":"42","generation":state.notification_generation,"revision":action.id,"outcome":"unknown"}));
        let mut old=state.clone(); project_snapshot(&app.ctx,&mut old);
        assert_eq!(app.visual_presence(&old),"idle"); assert_eq!(visual(&app.ctx).intent.unwrap().phase,"unknown");
    }
}
#[cfg(test)]
mod discrepancy_tests {
    use super::*;
    use super::super::tests::{harness,signed_in};
    #[test]
    fn a_remote_mismatch_has_a_check_action_without_creating_a_visual_choice_or_changing_policy() {
        let (mut app,_,mut commands)=harness(); let mut state=signed_in("42");
        state.self_presence="online".into(); state.notification_generation=3; app.state=state.clone();
        app.presence_discrepancy(&serde_json::json!({"account_id":"42","generation":3,"revision":8,"status":"dnd"}));
        assert!(commands.try_recv().is_err()); assert!(visual(&app.ctx).intent.is_none());
        assert_eq!(visual(&app.ctx).remote.as_ref().unwrap().status,"dnd"); assert_eq!(app.state.self_presence,"online");
        app.check_remote_presence(&state); let action=commands.try_recv().unwrap();
        assert_eq!((action.kind.as_str(),action.id.as_str()),("presence-check","remote:8"));
        app.check_remote_presence(&state); assert!(commands.try_recv().is_err());
        app.presence_result(&serde_json::json!({"account_id":"42","generation":3,"revision":"remote:8","outcome":"check-failed"}));
        assert!(!visual(&app.ctx).remote.as_ref().unwrap().checking);
        let mut other=signed_in("43"); project_snapshot(&app.ctx,&mut other); assert!(visual(&app.ctx).remote.is_none());
    }
    #[test]
    fn stale_remote_results_and_events_cannot_replace_a_more_recent_local_choice() {
        let (mut app,_,mut commands)=harness(); let mut state=signed_in("42");
        state.self_presence="online".into(); state.notification_generation=3; app.state=state.clone();
        let discrepancy=serde_json::json!({"account_id":"42","generation":3,"revision":8,"status":"dnd"});
        app.presence_discrepancy(&discrepancy); app.check_remote_presence(&state); commands.try_recv().unwrap();
        app.choose_presence(&state,"idle"); assert_eq!(commands.try_recv().unwrap().text,"idle");
        app.presence_result(&serde_json::json!({"account_id":"42","generation":3,"revision":"remote:8","outcome":"checked","status":"dnd"}));
        app.presence_discrepancy(&discrepancy);
        app.project_visual_presence(); assert_eq!(app.visual_presence(&app.state),"idle"); assert_eq!(app.state.self_presence,"online");
        assert!(visual(&app.ctx).remote.is_none()); assert_eq!(visual(&app.ctx).intent.unwrap().phase,"pending");
    }
}

#[cfg(test)]
mod discrepancy_order_tests {
    use super::*;
    use super::super::tests::{harness,signed_in};
    #[test]
    fn a_mismatch_between_confirmation_and_snapshot_waits_without_being_lost_or_clobbered_by_old_clears() {
        let (mut app,_,mut commands)=harness(); let mut state=signed_in("42");
        state.self_presence="online".into(); state.notification_generation=3; app.state=state.clone();
        app.choose_presence(&state,"idle"); let action=commands.try_recv().unwrap();
        app.presence_result(&serde_json::json!({"account_id":"42","generation":3,"revision":action.id,"outcome":"confirmed","status":"idle"}));
        app.presence_discrepancy(&serde_json::json!({"account_id":"42","generation":3,"revision":12,"status":"dnd"}));
        assert_eq!(visual(&app.ctx).remote.as_ref().unwrap().status,"dnd");
        app.check_remote_presence(&state); assert!(commands.try_recv().is_err(),"a presentation intention still owns this frame");
        state.self_presence="idle".into(); project_snapshot(&app.ctx,&mut state); app.state=state.clone();
        assert!(visual(&app.ctx).intent.is_none()); assert!(visual(&app.ctx).remote.is_some());
        app.presence_discrepancy(&serde_json::json!({"account_id":"42","generation":3,"revision":11,"status":null}));
        assert!(visual(&app.ctx).remote.is_some(),"a stale clear cannot erase the newer discrepancy");
        app.presence_discrepancy(&serde_json::json!({"account_id":"42","generation":3,"revision":13,"status":null}));
        app.presence_discrepancy(&serde_json::json!({"account_id":"42","generation":3,"revision":12,"status":"dnd"}));
        assert!(visual(&app.ctx).remote.is_none(),"an old mismatch cannot return after its clear");
    }
}
