use super::*;
use notdiscord_core::settings_proto;

#[derive(Default)]
pub(super) struct Changes {
    pending: Option<u64>,
    queued: Option<UiAction>,
    confirmed: bool,
    reconcile: Option<u64>,
    reconcile_again: bool,
}

/// The confirmed own setting is authoritative for presentation. Presence echoes
/// may arrive later than its settings response; retain activities/custom status.
pub(super) fn project_self(snapshot: &mut AppSnapshot) {
    let Some(id)=snapshot.current_user.as_ref().map(|person|person.id.clone()).filter(|id|!id.is_empty()) else{return;};
    if !settings_proto::valid_presence(&snapshot.self_presence){return;}
    let status=snapshot.self_presence.clone();
    if !snapshot.user_presences.contains_key(&id) && snapshot.user_presences.len()>=2000{
        if let Some(expired)=snapshot.user_presences.keys().next().cloned(){snapshot.user_presences.remove(&expired);}
    }
    snapshot.user_presences.entry(id.clone()).or_default().status=status.clone();
    // Preserve SharedList storage identity on unchanged snapshots.
    if snapshot.guild_members.iter().any(|member|member.user.id==id && member.presence!=status){
        for member in snapshot.guild_members.iter_mut(){if member.user.id==id{member.presence=status.clone();}}
    }
    for member in &mut snapshot.group_members{
        if member.user.id==id{member.presence=status.clone();}
    }
    for item in &mut snapshot.direct_messages{if item.recipient_id==id{item.presence=status.clone();}}
    if let Some(profile)=snapshot.profile_details.as_mut().filter(|profile|profile.user.id==id){profile.presence=status;}
}

// A successful persisted change and a failed Gateway flush are different
// outcomes. Notifications must follow the confirmed setting in either case.
async fn execute_change<P, PF, G, GF>(status: String, patch: P, gateway: G) -> Reply
where
    P: FnOnce(String) -> PF,
    PF: std::future::Future<Output = Reply>,
    G: FnOnce(String) -> GF,
    GF: std::future::Future<Output = Result<(), String>>,
{
    let response = patch(status.clone()).await?;
    let confirmed = response["status"].as_str()
        .filter(|value| settings_proto::valid_presence(value))
        .unwrap_or(&status).to_owned();
    let error = gateway(confirmed.clone()).await.err();
    Ok(json!({"status":confirmed,"gateway_error":error}))
}

impl Controller {
    pub(super) fn remote_presence(&mut self,status:&str){
        if !settings_proto::valid_presence(status){return;}
        if self.presence_changes.pending.is_some(){self.presence_changes.reconcile_again=true;return;}
        if self.presence_changes.reconcile.is_some(){self.presence_changes.reconcile_again=true;return;}
        if !self.presence_changes.confirmed || status==self.state.self_presence{self.apply_self_presence(status);return;}
        if let Err(error)=self.reconcile_presence(){self.error(error);}
    }
    fn reconcile_presence(&mut self)->Result<(),String>{
        if self.presence_changes.reconcile.is_some(){self.presence_changes.reconcile_again=true;return Ok(());}
        let api=self.api()?;let version=self.next_request("presence-read");
        self.presence_changes.reconcile=Some(version);self.presence_changes.reconcile_again=false;
        let mut action=UiAction::new("__presence-reconciled");action.extra=version.to_string();
        self.launch(action,None,async move{
            let value=api.request(Method::GET,"users/@me/settings",None).await.map_err(|error|error.to_string())?;
            let status=value["status"].as_str().filter(|status|settings_proto::valid_presence(status)).ok_or("No se confirmó el estado actual de la cuenta; se conserva el último confirmado.")?;
            Ok(json!({"status":status}))
        });Ok(())
    }
    pub(super) fn finish_presence_reconcile(&mut self,action:&UiAction,value:Option<&Value>)->Result<(),String>{
        if !self.presence_read_current(action){return Ok(());}
        self.presence_changes.reconcile=None;
        if self.presence_changes.pending.is_some(){self.presence_changes.reconcile_again=true;return Ok(());}
        if self.presence_changes.reconcile_again && value.is_some(){return self.reconcile_presence();}
        self.presence_changes.reconcile_again=false;
        if let Some(status)=value.and_then(|value|value["status"].as_str()){self.apply_self_presence(status);}
        Ok(())
    }
    pub(super) fn presence_read_current(&self,action:&UiAction)->bool{
        action.kind!="__presence-reconciled" || action.extra.parse::<u64>().ok().is_some_and(|version|Some(version)==self.presence_changes.reconcile)
    }
    pub(super) fn presence_change_pending(&self)->bool {self.presence_changes.pending.is_some()}
    pub(super) fn apply_self_presence(&mut self, status: &str) {
        if !settings_proto::valid_presence(status) { return; }
        let changed=self.state.self_presence != status;
        if status == "dnd" && self.state.self_presence != "dnd" {
            self.state.notification_epoch = self.state.notification_epoch.wrapping_add(1);
        }
        self.state.self_presence = status.to_owned();
        if changed{self.state.presence_version=self.state.presence_version.wrapping_add(1);}
        project_self(&mut self.state);
        if !changed{self.publish();return;}
        if matches!(status,"invisible"|"offline"){self.game_detector.suspend_rpc();}
        // This ordered policy barrier precedes sound/notice data even when
        // the full snapshot is still waiting for its coalescing interval.
        self.data("notification-policy", json!({"account_id":self.me(),"generation":self.generation,"presence":status,"epoch":self.state.notification_epoch}));
        self.publish();
    }

    pub(super) fn change_presence(&mut self, action: UiAction, api: Api) -> Result<(), String> {
        if !settings_proto::valid_presence(&action.text) { return Err("Estado no válido.".into()); }
        if self.presence_changes.pending.is_some() {
            self.presence_changes.queued = Some(action);
            return Ok(());
        }
        self.start_presence_change(action, api)
    }
    fn start_presence_change(&mut self, mut action: UiAction, api: Api) -> Result<(), String> {
        // A new explicit choice supersedes an older read reconciliation.
        self.presence_changes.reconcile=None;
        let status = action.text.clone();
        let gateway = self.gateway.clone().ok_or("Gateway no preparado.")?;
        let activity = self.game_presence_for_status(&status);
        if activity.is_some(){gateway.allow_game_presence().map_err(|error|error.to_string())?;}
        let game_epoch=gateway.game_presence_epoch();
        action.kind = "__status".into();
        let version = self.next_request("presence-change");
        action.extra = version.to_string();
        self.presence_changes.pending = Some(version);
        self.launch(action, None, async move {
            let sent_activity=activity.clone();
            let mut value=execute_change(status,
                |status| async move {
                    api.request(Method::PATCH, "users/@me/settings", Some(json!({"status":status})))
                        .await.map_err(|error|error.to_string())
                },
                |status| async move {
                    if matches!(status.as_str(),"invisible"|"offline") {
                        gateway.enqueue_clear_game_presence(&status).map_err(|error|error.to_string())?;
                        let epoch=gateway.game_presence_epoch();
                        return gateway.send_rich_presence_for_epoch(status,None,epoch).await.map_err(|error|error.to_string());
                    }
                    gateway.send_rich_presence_for_epoch(status,activity,game_epoch)
                        .await.map_err(|error|error.to_string())
                }).await?;
            value["game_presence"]=json!(if matches!(value["status"].as_str(),Some("online"|"idle"|"dnd")){sent_activity}else{None});
            Ok(value)
        });
        Ok(())
    }
    pub(super) fn finish_presence_change(&mut self, action: &UiAction, value: Option<&Value>) -> Result<(), String> {
        if !action.extra.is_empty() && action.extra.parse::<u64>().ok().is_none_or(|version|self.presence_changes.pending != Some(version)) {
            return Ok(());
        }
        if let Some(value) = value {
            self.presence_changes.confirmed=true;
            let status = value["status"].as_str().unwrap_or(&action.text);
            self.apply_self_presence(status);
            if let Some(error) = value["gateway_error"].as_str() {
                self.error(format!("Estado guardado; no se confirmó la actualización de presencia: {error}"));
            } else {
                let activity=value.get("game_presence").and_then(|value|serde_json::from_value(value.clone()).ok())
                    .unwrap_or_else(||self.game_presence_for_status(&self.state.self_presence));
                self.game_detector.record_rich_presence(self.state.self_presence.clone(),activity);
            }
        }
        self.presence_changes.pending = None;
        if let Some(next) = self.presence_changes.queued.take() {
            self.start_presence_change(next, self.api()?)?;
        }else if self.presence_changes.reconcile_again{
            self.reconcile_presence()?;
        }
        self.publish();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
