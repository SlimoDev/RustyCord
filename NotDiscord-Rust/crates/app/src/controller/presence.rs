use super::*;
use notdiscord_core::{api::{ErrorKind, ServiceError}, settings_proto};

#[derive(Default)]
pub(super) struct Changes {
    pending: Option<u64>,
    queued: Option<UiAction>,
    visual: Option<UiAction>,
    confirmed: bool,
    reconcile: Option<u64>,
    stopped: bool,
    unknown: bool,
    gateway_unconfirmed: bool,
    discrepancy: Option<String>,
    discrepancy_revision: u64,
    intent_revision: u64,
    reconcile_intent: u64,
}

/// Presentation only: the confirmed setting and its policy stay independent.
pub(crate) fn project_status(snapshot: &mut AppSnapshot, status: &str) {
    let Some(id) = snapshot.current_user.as_ref().map(|person| person.id.clone()).filter(|id| !id.is_empty()) else { return; };
    if !settings_proto::valid_presence(status) { return; }
    let mut changed = snapshot.user_presences.get(&id).is_none_or(|presence| presence.status != status);
    if !snapshot.user_presences.contains_key(&id) && snapshot.user_presences.len() >= 2000 {
        if let Some(expired) = snapshot.user_presences.keys().next().cloned() { snapshot.user_presences.remove(&expired); }
    }
    snapshot.user_presences.entry(id.clone()).or_default().status = status.to_owned();
    // Keep SharedList identity when no member changed.
    if snapshot.guild_members.iter().any(|member| member.user.id == id && member.presence != status) {
        for member in snapshot.guild_members.iter_mut() { if member.user.id == id { member.presence = status.to_owned(); } }
        changed = true;
    }
    for member in &mut snapshot.group_members {
        if member.user.id == id && member.presence != status { member.presence = status.to_owned(); changed = true; }
    }
    for item in &mut snapshot.direct_messages {
        if item.recipient_id == id && item.presence != status { item.presence = status.to_owned(); changed = true; }
    }
    if let Some(profile) = snapshot.profile_details.as_mut().filter(|profile| profile.user.id == id && profile.presence != status) {
        profile.presence = status.to_owned(); changed = true;
    }
    if changed { snapshot.presence_version = snapshot.presence_version.wrapping_add(1); }
}

pub(super) fn project_self(snapshot: &mut AppSnapshot) {
    let status = snapshot.self_presence.clone();
    project_status(snapshot, &status);
}

fn write_failure(error: ServiceError) -> Value {
    let outcome = match error.kind {
        ErrorKind::OutcomeUnknown | ErrorKind::Network | ErrorKind::OperationPending => "unknown",
        ErrorKind::RateLimited => "rate-limited",
        ErrorKind::TransportUnavailable | ErrorKind::QueueFull => "not-sent",
        _ => "rejected",
    };
    json!({"outcome":outcome,"error":error.message,"retry_after_ms":error.retry_after.map(|wait| wait.as_millis().min(u64::MAX as u128) as u64)})
}

// PATCH confirmation survives a failed Gateway flush. Preserve typed failures.
async fn execute_change<P, PF, G, GF>(status: String, patch: P, gateway: G) -> Reply
where
    P: FnOnce(String) -> PF,
    PF: std::future::Future<Output = Result<Value, ServiceError>>,
    G: FnOnce(String) -> GF,
    GF: std::future::Future<Output = Result<(), String>>,
{
    let response = match patch(status.clone()).await {
        Ok(response) => response,
        Err(error) => return Ok(write_failure(error)),
    };
    let confirmed = response["status"].as_str().filter(|value| settings_proto::valid_presence(value)).unwrap_or(&status).to_owned();
    let error = gateway(confirmed.clone()).await.err();
    Ok(json!({"outcome":if error.is_some(){"persisted"}else{"confirmed"},"status":confirmed,"gateway_error":error}))
}

impl Controller {
    fn presence_result(&self, action: &UiAction, outcome: &str, status: &str) {
        self.data("presence-result", json!({"account_id":self.me(),"generation":self.generation,
            "revision":action.id,"outcome":outcome,"status":status}));
    }
    fn presence_action_current(&self, action: &UiAction) -> bool {
        if action.extra.is_empty() { return true; }
        serde_json::from_str::<Value>(&action.extra).ok().is_some_and(|scope|
            scope["account_id"].as_str() == Some(self.me().as_str()) && scope["generation"].as_u64() == Some(self.generation))
    }
    pub(super) fn presence_action(&mut self, action: UiAction) -> Result<(), String> {
        if !self.presence_action_current(&action) { return Ok(()); }
        if action.kind == "presence-check" {
            let source = action.clone();
            let result = self.reconcile_presence(action);
            if result.is_err() { self.presence_result(&source,"check-failed",&self.state.self_presence); }
            return result;
        }
        if !settings_proto::valid_presence(&action.text) { return Err("Estado no válido.".into()); }
        self.presence_changes.visual = Some(action.clone());
        if !self.state.signed_in || self.transport_suspended() {
            self.note_presence_intent();
            self.presence_changes.stopped = true;
            self.presence_result(&action, "not-sent", &action.text);
            return Ok(());
        }
        let result = self.api().and_then(|api| self.change_presence(action.clone(), api));
        if let Err(error) = result {
            self.presence_result(&action, if self.presence_changes.unknown {"not-sent"} else if self.presence_changes.gateway_unconfirmed {"persisted"} else {"rejected"}, &self.state.self_presence);
            self.error(error);
        }
        Ok(())
    }
    pub(super) fn remote_presence(&mut self, status: &str) {
        if !settings_proto::valid_presence(status) { return; }
        // An uncorrelated echo cannot supersede a local write or start polling.
        if self.presence_changes.pending.is_some() || self.presence_changes.reconcile.is_some() || self.presence_changes.unknown { return; }
        if !self.presence_changes.confirmed || status == self.state.self_presence {
            self.set_presence_discrepancy(None);
            self.apply_self_presence(status);
        } else { self.set_presence_discrepancy(Some(status)); }
    }
    fn set_presence_discrepancy(&mut self, status: Option<&str>) {
        if self.presence_changes.discrepancy.as_deref() == status { return; }
        self.presence_changes.discrepancy = status.map(str::to_owned);
        self.presence_changes.discrepancy_revision = self.presence_changes.discrepancy_revision.wrapping_add(1).max(1);
        self.data("presence-discrepancy",json!({"account_id":self.me(),"generation":self.generation,
            "revision":self.presence_changes.discrepancy_revision,"status":status}));
    }
    fn note_presence_intent(&mut self) {
        self.presence_changes.intent_revision = self.presence_changes.intent_revision.wrapping_add(1);
        self.set_presence_discrepancy(None);
    }
    fn reconcile_presence(&mut self, mut action: UiAction) -> Result<(), String> {
        if let Some(revision) = action.id.strip_prefix("remote:") {
            if self.presence_changes.discrepancy.is_none()
                || revision.parse::<u64>().ok() != Some(self.presence_changes.discrepancy_revision) { return Err("El estado remoto cambió. Comprueba el aviso actual.".into()); }
        } else if self.presence_changes.visual.as_ref().is_some_and(|visual|visual.id != action.id) { return Err("La elección de estado cambió. Comprueba la elección actual.".into()); }
        if self.presence_changes.reconcile.is_some() { return Ok(()); }
        if self.presence_changes.pending.is_some() { return Err("Espera a que termine el cambio de estado actual.".into()); }
        if self.transport_suspended() { return Err(connection::OFFLINE.into()); }
        let api = self.api()?;
        let version = self.next_request("presence-read");
        self.presence_changes.reconcile = Some(version);
        self.presence_changes.reconcile_intent = self.presence_changes.intent_revision;
        action.kind = "__presence-reconciled".into(); action.extra = version.to_string();
        self.launch(action, None, async move {
            let value = api.get_once("users/@me/settings").await.map_err(|error|error.to_string())?;
            let status = value["status"].as_str().filter(|status|settings_proto::valid_presence(status))
                .ok_or("No se confirmó el estado actual de la cuenta; se conserva el último confirmado.")?;
            Ok(json!({"status":status}))
        });
        Ok(())
    }
    pub(super) fn finish_presence_reconcile(&mut self, action: &UiAction, value: Option<&Value>) -> Result<(), String> {
        if !self.presence_read_current(action) { return Ok(()); }
        self.presence_changes.reconcile = None;
        if self.presence_changes.reconcile_intent != self.presence_changes.intent_revision { return Ok(()); }
        if let Some(status) = value.and_then(|value|value["status"].as_str()) {
            self.presence_changes.confirmed = true;
            self.apply_self_presence(status);
            self.set_presence_discrepancy(None);
            // A read confirms today's setting, not the earlier write's identity.
            // Keep Operations.Unknown and RateLimits untouched.
            self.presence_result(action, if self.presence_changes.unknown {"checked-unknown"} else if self.presence_changes.gateway_unconfirmed {"persisted"} else {"checked"}, status);
            if !self.presence_changes.unknown && !self.presence_changes.gateway_unconfirmed { self.presence_changes.visual = None; }
        } else { self.presence_result(action, "check-failed", &self.state.self_presence); }
        self.publish();
        Ok(())
    }
    pub(super) fn presence_read_current(&self, action: &UiAction) -> bool {
        action.kind != "__presence-reconciled" || action.extra.parse::<u64>().ok().is_some_and(|version|Some(version)==self.presence_changes.reconcile)
    }
    pub(super) fn presence_change_pending(&self) -> bool {
        self.presence_changes.pending.is_some() || self.presence_changes.unknown || self.presence_changes.gateway_unconfirmed
    }
    pub(super) fn suspend_presence_changes(&mut self) {
        if self.presence_changes.visual.is_none() { return; }
        self.presence_changes.stopped = true;
        if let Some(action) = self.presence_changes.queued.as_ref().or_else(||
            self.presence_changes.pending.is_none().then_some(self.presence_changes.visual.as_ref()).flatten()) {
            self.presence_result(action, "not-sent", &action.text);
        }
    }
    pub(super) fn apply_self_presence(&mut self, status: &str) {
        if !settings_proto::valid_presence(status) { return; }
        let changed = self.state.self_presence != status;
        if status == "dnd" && self.state.self_presence != "dnd" { self.state.notification_epoch = self.state.notification_epoch.wrapping_add(1); }
        self.state.self_presence = status.to_owned();
        if changed { self.state.presence_version = self.state.presence_version.wrapping_add(1); }
        project_self(&mut self.state);
        if !changed { self.publish(); return; }
        if matches!(status,"invisible"|"offline") { self.game_detector.suspend_rpc(); }
        self.data("notification-policy", json!({"account_id":self.me(),"generation":self.generation,"presence":status,"epoch":self.state.notification_epoch}));
        self.publish();
    }
    pub(super) fn change_presence(&mut self, action: UiAction, api: Api) -> Result<(), String> {
        if !settings_proto::valid_presence(&action.text) { return Err("Estado no válido.".into()); }
        self.note_presence_intent();
        if self.presence_changes.unknown { return Err("El cambio de estado anterior sigue sin confirmar. Comprueba el estado; no se repetirá la escritura.".into()); }
        self.presence_changes.visual = Some(action.clone());
        if self.presence_changes.pending.is_some() {
            self.presence_changes.queued = Some(action);
            return Ok(());
        }
        if self.transport_suspended() {
            self.presence_result(&action,"not-sent",&action.text);
            self.presence_changes.stopped = true;
            return Ok(());
        }
        self.presence_changes.stopped = false;
        self.presence_changes.queued = None;
        if self.presence_changes.gateway_unconfirmed && action.text == self.state.self_presence {
            self.start_presence_flush(action)
        } else { self.start_presence_change(action, api) }
    }
    fn start_presence_flush(&mut self, mut action: UiAction) -> Result<(), String> {
        // Explicit retry after a confirmed PATCH repairs only its Gateway lane.
        let gateway = self.gateway.clone().ok_or("Gateway no preparado.")?;
        let status = self.state.self_presence.clone();
        let activity = self.game_presence_for_status(&status);
        if activity.is_some() { gateway.allow_game_presence().map_err(|error|error.to_string())?; }
        let epoch = gateway.game_presence_epoch();
        action.kind = "__status".into();
        let version = self.next_request("presence-change"); action.extra = version.to_string();
        self.presence_changes.pending = Some(version);
        self.launch(action,None,async move {
            let result = if matches!(status.as_str(),"invisible"|"offline") {
                gateway.send_clear_game_presence(status.clone()).await
            } else { gateway.send_rich_presence_for_epoch(status.clone(),activity.clone(),epoch).await };
            Ok(json!({"outcome":if result.is_ok(){"confirmed"}else{"persisted"},"status":status,
                "gateway_error":result.err().map(|error|error.to_string()),"game_presence":activity}))
        });
        Ok(())
    }
    fn start_presence_change(&mut self, mut action: UiAction, api: Api) -> Result<(), String> {
        let status = action.text.clone();
        let gateway = self.gateway.clone().ok_or("Gateway no preparado.")?;
        api.safety().check().map_err(|error|error.to_string())?;
        let activity = self.game_presence_for_status(&status);
        if activity.is_some() { gateway.allow_game_presence().map_err(|error|error.to_string())?; }
        let game_epoch = gateway.game_presence_epoch();
        action.kind = "__status".into();
        let version = self.next_request("presence-change"); action.extra = version.to_string();
        self.presence_changes.pending = Some(version);
        self.launch(action, None, async move {
            let sent_activity = activity.clone();
            let mut value = execute_change(status,
                |status| async move { api.request(Method::PATCH,"users/@me/settings",Some(json!({"status":status}))).await },
                |status| async move {
                    if matches!(status.as_str(),"invisible"|"offline") {
                        return gateway.send_clear_game_presence(status).await.map_err(|error|error.to_string());
                    }
                    gateway.send_rich_presence_for_epoch(status,activity,game_epoch).await.map_err(|error|error.to_string())
                }).await?;
            value["game_presence"] = json!(if matches!(value["status"].as_str(),Some("online"|"idle"|"dnd")){sent_activity}else{None});
            Ok(value)
        });
        Ok(())
    }
    pub(super) fn finish_presence_change(&mut self, action: &UiAction, value: Option<&Value>) -> Result<(), String> {
        if !action.extra.is_empty() && action.extra.parse::<u64>().ok().is_none_or(|version|self.presence_changes.pending != Some(version)) { return Ok(()); }
        let outcome = value.and_then(|value|value["outcome"].as_str()).unwrap_or(if value.is_some_and(|value|value["gateway_error"].as_str().is_some()){"persisted"}else if value.is_some(){"confirmed"}else{"unknown"});
        let confirmed = matches!(outcome,"confirmed"|"persisted");
        let status = value.and_then(|value|value["status"].as_str()).filter(|status|settings_proto::valid_presence(status)).unwrap_or(&action.text).to_owned();
        if confirmed {
            self.presence_changes.gateway_unconfirmed = outcome == "persisted";
            if outcome == "persisted" { self.presence_changes.stopped = true; }
            self.presence_changes.confirmed = true;
            self.apply_self_presence(&status);
            if let Some(error) = value.and_then(|value|value["gateway_error"].as_str()) {
                self.error(format!("Estado guardado; no se confirmó la actualización de presencia: {error}"));
            } else {
                let activity = value.and_then(|value|value.get("game_presence")).and_then(|value|serde_json::from_value(value.clone()).ok())
                    .unwrap_or_else(||self.game_presence_for_status(&self.state.self_presence));
                self.game_detector.record_rich_presence(self.state.self_presence.clone(),activity);
            }
        } else {
            self.presence_changes.stopped = true;
            self.presence_changes.unknown |= outcome == "unknown";
            if let Some(error) = value.and_then(|value|value["error"].as_str()) { self.error(error.to_owned()); }
        }
        self.presence_result(action,outcome,if confirmed {&status}else{&self.state.self_presence});
        self.presence_changes.pending = None;
        if let Some(next) = self.presence_changes.queued.take() {
            if outcome == "confirmed" && !self.presence_changes.stopped && !self.transport_suspended() {
                if next.text == self.state.self_presence {
                    self.presence_result(&next,"confirmed",&self.state.self_presence);
                    self.presence_changes.visual = None;
                } else {
                    let result = self.api().and_then(|api|self.start_presence_change(next.clone(),api));
                    if let Err(error) = result { self.presence_result(&next,"not-sent",&next.text); self.presence_changes.stopped = true; self.error(error); }
                }
            } else {
                self.presence_changes.stopped = true;
                self.presence_result(&next,"not-sent",&next.text);
            }
        } else if matches!(outcome,"confirmed"|"rejected"|"rate-limited") {
            self.presence_changes.visual = None;
        }
        self.publish();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn typed_unknown_stops_the_queued_choice_without_starting_another_task() {
        let (mut c, updates) = Controller::test_fixture();
        c.state.signed_in=true;
        c.state.current_user=Some(Person{id:"1".into(),..Default::default()});
        c.apply_self_presence("online");
        c.presence_changes.pending=Some(1);
        let next=UiAction{kind:"status".into(),id:"2".into(),text:"idle".into(),..Default::default()};
        c.presence_changes.visual=Some(next.clone()); c.presence_changes.queued=Some(next);
        let action=UiAction{kind:"__status".into(),id:"1".into(),text:"dnd".into(),extra:"1".into(),..Default::default()};
        let failure=execute_change("dnd".into(), |_| async {Err(ServiceError::outcome_unknown())}, |_| async {panic!("a failed PATCH must not flush Gateway");#[allow(unreachable_code)] Ok(())}).await.unwrap();
        assert_eq!(failure["outcome"],"unknown");
        c.finish_presence_change(&action,Some(&failure)).unwrap();
        assert_eq!(c.state.self_presence,"online"); assert!(c.presence_changes.unknown);
        assert!(c.presence_changes.pending.is_none()); assert!(c.presence_changes.queued.is_none());
        assert!(c.request_versions.is_empty());
        let outcomes=updates.try_iter().filter_map(|update|match update{UiUpdate::Data(kind,value) if kind=="presence-result"=>Some((value["revision"].clone(),value["outcome"].clone())),_=>None}).collect::<Vec<_>>();
        assert_eq!(outcomes,vec![(json!("1"),json!("unknown")),(json!("2"),json!("not-sent"))]);
        let api=Api::new("synthetic-presence-no-network".into()).unwrap();
        assert!(c.change_presence(UiAction{kind:"status".into(),text:"online".into(),..Default::default()},api).is_err());
    }
    #[test]
    fn mismatch_echoes_never_launch_a_settings_poll_or_erase_confirmed_policy() {
        let (mut c,_) = Controller::test_fixture();
        c.apply_self_presence("online"); c.presence_changes.confirmed=true;
        for status in ["dnd","idle","dnd","idle"] { c.remote_presence(status); }
        assert_eq!(c.state.self_presence,"online"); assert!(c.request_versions.is_empty());
        assert!(c.presence_changes.reconcile.is_none());
    }
    #[test]
    fn definitive_and_rate_limited_failures_remain_distinct_from_unknown_results() {
        assert_eq!(write_failure(ServiceError::status("denied",403))["outcome"],"rejected");
        let mut limited=ServiceError::status("slow down",429); limited.retry_after=Some(Duration::from_secs(90));
        let value=write_failure(limited); assert_eq!(value["outcome"],"rate-limited"); assert_eq!(value["retry_after_ms"],90000);
        assert_eq!(write_failure(ServiceError::typed(ErrorKind::TransportUnavailable,"not sent"))["outcome"],"not-sent");
    }
    #[test]
    fn confirmed_completion_after_connection_loss_does_not_drain_latest_choice() {
        let (mut c,_) = Controller::test_fixture();
        c.state.current_user=Some(Person{id:"1".into(),..Default::default()});
        c.presence_changes.pending=Some(7);
        let next=UiAction{kind:"status".into(),id:"8".into(),text:"idle".into(),..Default::default()};
        c.presence_changes.queued=Some(next.clone()); c.presence_changes.visual=Some(next);
        c.suspend_presence_changes();
        c.finish_presence_change(&UiAction{kind:"__status".into(),id:"7".into(),extra:"7".into(),text:"online".into(),..Default::default()},Some(&json!({"outcome":"confirmed","status":"online"}))).unwrap();
        assert_eq!(c.state.self_presence,"online"); assert!(c.presence_changes.pending.is_none());
        assert!(c.presence_changes.stopped); assert!(c.request_versions.is_empty());
        assert_eq!(c.presence_changes.visual.as_ref().unwrap().text,"idle");
    }
    #[test]
    fn one_inflight_presence_write_coalesces_the_next_choice_without_spawning_more_work() {
        let (mut c, _) = Controller::test_fixture();
        c.presence_changes.pending=Some(1);
        let api=Api::new("offline-synthetic-credential-only".into()).unwrap();
        for status in ["dnd","online","idle"] {
            c.change_presence(UiAction{kind:"status".into(),text:status.into(),..Default::default()},api.clone()).unwrap();
        }
        assert_eq!(c.presence_changes.pending,Some(1));
        assert_eq!(c.presence_changes.queued.as_ref().unwrap().text,"idle");
        assert!(c.request_versions.is_empty(),"queued choices must not start a task or acquire the API");
    }

    #[tokio::test]
    async fn confirmed_exit_from_dnd_survives_gateway_failure_and_new_notices_resume() {
        let (mut c, updates) = Controller::test_fixture();
        c.state.signed_in = true;
        c.state.current_user = Some(Person {id:"1".into(),..Default::default()});
        c.apply_self_presence("dnd");
        let result = execute_change("online".into(), |status| async move {
            assert_eq!(status,"online"); Ok(json!({"status":"online"}))
        }, |_| async { Err("synthetic Gateway flush failure".into()) }).await.unwrap();
        c.complete(UiAction {kind:"__status".into(),text:"online".into(),..Default::default()},result).await.unwrap();
        assert_eq!(c.state.self_presence,"online");
        c.dispatch("MESSAGE_CREATE",json!({"id":"100","channel_id":"20","author":{"id":"2","username":"Peer"},"content":"new"})).await.unwrap();
        let emitted=updates.try_iter().filter_map(|u|match u{UiUpdate::Data(kind,value)=>Some((kind,value)),_=>None}).collect::<Vec<_>>();
        assert_eq!(emitted.iter().filter(|(kind,_)|kind=="notice").count(),1);
        assert_eq!(emitted.iter().filter(|(kind,_)|kind=="sound").count(),1);
    }

    #[tokio::test]
    async fn old_status_completion_cannot_restore_dnd_after_a_newer_choice() {
        let (mut c, _) = Controller::test_fixture();
        c.next_request("presence-change");
        let newest=c.next_request("presence-change");
        c.presence_changes.pending=Some(newest);
        c.complete(UiAction{kind:"__status".into(),text:"online".into(),extra:newest.to_string(),..Default::default()},json!({"status":"online"})).await.unwrap();
        c.complete(UiAction{kind:"__status".into(),text:"dnd".into(),extra:(newest-1).to_string(),..Default::default()},json!({"status":"dnd"})).await.unwrap();
        assert_eq!(c.state.self_presence,"online");
    }

    #[tokio::test]
    async fn confirmed_old_intent_is_kept_when_the_following_write_fails() {
        let (mut c, _) = Controller::test_fixture();
        c.apply_self_presence("dnd");
        let first=c.next_request("presence-change");
        c.presence_changes.pending=Some(first);
        c.next_request("presence-change");
        let mut action=UiAction{kind:"__status".into(),text:"online".into(),extra:first.to_string(),..Default::default()};
        c.finish_presence_change(&action,Some(&json!({"status":"online","gateway_error":"synthetic"}))).unwrap();
        c.presence_changes.pending=Some(first+1);action.extra=(first+1).to_string();action.text="idle".into();
        c.finish_presence_change(&action,None).unwrap();
        assert_eq!(c.state.self_presence,"online");
    }

    #[tokio::test]
    async fn dnd_cycle_drops_deferred_notices_preserves_mutes_and_never_replays_omissions() {
        let (mut c, updates) = Controller::test_fixture();
        c.state.signed_in = true;
        c.state.current_user = Some(Person{id:"1".into(),..Default::default()});
        c.apply_self_presence("online");
        let message = |id:&str,channel:&str| json!({"id":id,"channel_id":channel,"author":{"id":"2","username":"Peer"},"content":"new"});
        let notice = DesktopNotice{channel_id:"20".into(),message_id:"90".into(),..Default::default()};
        let mut prepared=serde_json::to_value(notice).unwrap();
        prepared["accountId"]=json!("1"); prepared["generation"]=json!(c.generation);
        prepared["notificationEpoch"]=json!(c.state.notification_epoch);
        c.apply_self_presence("dnd");
        c.dispatch("MESSAGE_CREATE",message("100","20")).await.unwrap();
        c.apply_self_presence("online");
        c.complete(UiAction{kind:"__notice-image-ready".into(),id:"90".into(),channel_id:"20".into(),..Default::default()},prepared).await.unwrap();
        c.muted.insert("21".into(),true);
        c.dispatch("MESSAGE_CREATE",message("101","21")).await.unwrap();
        c.dispatch("MESSAGE_CREATE",message("100","20")).await.unwrap();
        c.dispatch("MESSAGE_CREATE",message("102","20")).await.unwrap();
        let emitted=updates.try_iter().filter_map(|u|match u{UiUpdate::Data(kind,value)=>Some((kind,value)),_=>None}).collect::<Vec<_>>();
        for kind in ["notice","sound"] {
            let packets=emitted.iter().filter(|(k,_)|k==kind).collect::<Vec<_>>();
            assert_eq!(packets.len(),1,"{kind} must resume once, without replaying DND or muted messages");
            let id=if kind=="sound"{"message_id"}else{"MessageId"};
            assert_eq!(packets[0].1[id],"102");
        }
        assert_eq!(c.muted.get("21"),Some(&true));
    }
}

#[cfg(test)]
mod conservative_tests {
    use super::*;
    #[test]
    fn identical_non_ui_clicks_finish_locally_without_a_second_write() {
        let (mut c,updates)=Controller::test_fixture();
        c.state.current_user=Some(Person{id:"1".into(),..Default::default()});
        c.presence_changes.pending=Some(1);
        let api=Api::new("synthetic-presence-fixture".into()).unwrap();
        for id in ["2","3","4"] { c.change_presence(UiAction{kind:"status".into(),id:id.into(),text:"idle".into(),..Default::default()},api.clone()).unwrap(); }
        c.finish_presence_change(&UiAction{kind:"__status".into(),id:"1".into(),text:"idle".into(),extra:"1".into(),..Default::default()},Some(&json!({"outcome":"confirmed","status":"idle"}))).unwrap();
        assert!(c.presence_changes.pending.is_none()); assert!(c.request_versions.is_empty());
        assert!(updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,value) if kind=="presence-result" && value["revision"]=="4" && value["outcome"]=="confirmed")));
    }
    #[tokio::test]
    async fn explicit_retry_of_persisted_presence_only_flushes_gateway_and_never_repeats_patch() {
        let (mut c,_,mut replies)=Controller::test_fixture_with_replies();
        c.state.signed_in=true; c.state.current_user=Some(Person{id:"1".into(),..Default::default()});
        c.state.connection_phase=ConnectionPhase::Synchronized; c.apply_self_presence("online");
        let api=Api::new("synthetic-no-http-fixture".into()).unwrap();
        // A PATCH accidentally launched by this test would be rejected before
        // HTTP by this closed ticket. Only the offline Gateway can succeed.
        api.set_transport_available(false); c.api=Some(api.clone());
        let (gateway,mut packets,task)=notdiscord_core::gateway::GatewayHandle::offline_recorder(Default::default());
        c.gateway=Some(gateway.clone()); c.presence_changes.gateway_unconfirmed=true;
        c.change_presence(UiAction{kind:"status".into(),id:"9".into(),text:"online".into(),..Default::default()},api).unwrap();
        let packet=tokio::time::timeout(Duration::from_secs(1),packets.recv()).await.unwrap().unwrap();
        assert_eq!(packet["op"],3); assert_eq!(packet["d"]["status"],"online");
        let done=tokio::time::timeout(Duration::from_secs(1),replies.recv()).await.unwrap().unwrap();
        assert_eq!(done.result.as_ref().unwrap()["outcome"],"confirmed");
        c.complete(done.action,done.result.unwrap()).await.unwrap();
        assert!(!c.presence_changes.gateway_unconfirmed); assert!(!c.presence_change_pending());
        assert!(packets.try_recv().is_err()); gateway.stop(); task.await.unwrap();
    }
    #[test]
    fn unknown_and_unconfirmed_flush_hold_back_automatic_game_status_publication() {
        let (mut c,_)=Controller::test_fixture();
        c.presence_changes.unknown=true; assert!(c.presence_change_pending());
        c.presence_changes.unknown=false; c.presence_changes.gateway_unconfirmed=true; assert!(c.presence_change_pending());
        c.presence_changes.gateway_unconfirmed=false; assert!(!c.presence_change_pending());
    }
}

#[cfg(test)]
mod discrepancy_tests {
    use super::*;
    #[test]
    fn divergent_remote_echo_offers_an_explicit_check_without_launching_optional_work() {
        let (mut c,updates)=Controller::test_fixture();
        c.state.signed_in=true; c.state.current_user=Some(Person{id:"1".into(),..Default::default()});
        c.apply_self_presence("online"); c.presence_changes.confirmed=true;
        for _ in 0..5 { c.remote_presence("dnd"); }
        assert_eq!(c.state.self_presence,"online"); assert!(c.request_versions.is_empty());
        assert!(c.presence_changes.reconcile.is_none()); assert_eq!(c.presence_changes.discrepancy.as_deref(),Some("dnd"));
        let events=updates.try_iter().filter_map(|update|match update{UiUpdate::Data(kind,value) if kind=="presence-discrepancy"=>Some(value),_=>None}).collect::<Vec<_>>();
        assert_eq!(events.len(),1); assert_eq!(events[0]["status"],"dnd"); assert_eq!(events[0]["account_id"],"1");
        assert_eq!(events[0]["generation"],c.generation); assert_eq!(events[0]["revision"],1);
    }
    #[test]
    fn a_read_started_before_a_new_choice_cannot_restore_old_policy_or_visual_intention() {
        let (mut c,_)=Controller::test_fixture();
        c.apply_self_presence("online"); c.presence_changes.confirmed=true;
        c.presence_changes.reconcile=Some(7); c.presence_changes.reconcile_intent=c.presence_changes.intent_revision;
        // Keep the real old read owned until completion; a subsequent write is
        // separately coalesced. No second read or network task is created.
        c.presence_changes.pending=Some(9);
        c.change_presence(UiAction{kind:"status".into(),id:"new-choice".into(),text:"idle".into(),..Default::default()},Api::new("synthetic-presence-fixture".into()).unwrap()).unwrap();
        assert_eq!(c.presence_changes.reconcile,Some(7));
        let action=UiAction{kind:"__presence-reconciled".into(),id:"remote:1".into(),extra:"7".into(),..Default::default()};
        c.finish_presence_reconcile(&action,Some(&json!({"status":"dnd"}))).unwrap();
        assert_eq!(c.state.self_presence,"online"); assert_eq!(c.presence_changes.visual.as_ref().unwrap().text,"idle");
        assert_eq!(c.presence_changes.queued.as_ref().unwrap().id,"new-choice");
        assert!(c.presence_changes.reconcile.is_none()); assert!(c.request_versions.is_empty());
    }
}

#[cfg(test)]
mod discrepancy_admission_tests {
    use super::*;
    #[test]
    fn failed_check_admission_releases_the_correlated_ui_action_without_optional_work() {
        for phase in [ConnectionPhase::Cooldown,ConnectionPhase::Synchronized] {
            let (mut c,updates)=Controller::test_fixture();
            c.state.signed_in=true; c.state.current_user=Some(Person{id:"1".into(),..Default::default()});
            c.state.connection_phase=phase; c.presence_changes.confirmed=true; c.state.self_presence="online".into();
            c.remote_presence("dnd");
            let action=UiAction{kind:"presence-check".into(),id:"remote:1".into(),..Default::default()};
            // Cooldown fails at the transport gate; synchronized fails because
            // this synthetic fixture deliberately has no Api / account session.
            assert!(c.presence_action(action).is_err());
            assert!(c.presence_changes.reconcile.is_none()); assert!(c.request_versions.is_empty());
            assert!(updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,value) if kind=="presence-result" && value["revision"]=="remote:1" && value["outcome"]=="check-failed")));
        }
    }
}
