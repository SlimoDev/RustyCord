//! Private-call invitations are service-confirmed identities, not voice peers.
use super::*;

const RESPONSE_WINDOW: Duration = Duration::from_secs(10);
const SOLO_WINDOW: Duration = Duration::from_secs(120);
fn invitation_request(channel:&str,recipients:&[String],ring:bool)->(String,Value) {
    (format!("channels/{channel}/call/{}",if ring{"ring"}else{"stop-ringing"}),json!({"recipients":recipients}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call()->Controller {
        let(mut controller,_)=Controller::test_fixture();
        controller.state.signed_in=true;
        let me=Person{id:"1".into(),name:"Synthetic local".into(),..Default::default()};
        controller.state.current_user=Some(me.clone());controller.voice_channel="20".into();controller.voice_attempt=7;
        controller.state.voice_connected=true;
        controller.call_people.insert("1".into(),CallParticipant{user:me,is_self:true,..Default::default()});
        controller.prepare_private_call(&json!([{"id":"2","username":"Synthetic second"},{"id":"3","username":"Synthetic third"}]));
        controller.refresh_call();controller
    }
    fn flight(controller:&mut Controller,ids:&[&str],token:u64)->UiAction {
        let scope=&mut controller.voice_lifecycle.private_calls.outgoing;
        for id in ids {scope.finished.remove(*id);scope.flights.insert((*id).into(),Flight{token,revision:scope.ringing_revision});}
        scope.idle_since=None;
        UiAction{kind:"__private-ring".into(),id:"20".into(),extra:json!({"voiceAttempt":7,"token":token,"recipients":ids}).to_string(),..Default::default()}
    }
    #[test]
    fn service_request_bodies_target_only_selected_recipients_and_never_use_null_broadcast() {
        let recipients=vec!["2".into(),"3".into()];
        assert_eq!(invitation_request("20",&recipients,true),("channels/20/call/ring".into(),json!({"recipients":["2","3"]})));
        assert_eq!(invitation_request("20",&["3".into()],false),("channels/20/call/stop-ringing".into(),json!({"recipients":["3"]})));
    }
    #[tokio::test]
    async fn confirmed_invitations_survive_local_connect_and_first_peer_stops_only_the_sound() {
        let mut controller=call();let ack=flight(&mut controller,&["2","3"],1);
        controller.refresh_call();assert!(controller.state.call_pending_invitees.is_empty());assert!(!controller.state.outgoing_call_ringing);
        controller.complete_private_ring(&ack).unwrap();assert_eq!(controller.state.call_pending_invitees.len(),2);assert!(controller.state.outgoing_call_ringing);
        controller.media_event(json!({"event":"voiceState","state":"waiting-peer","transportReady":true,"encryptionReady":false})).await.unwrap();
        assert!(controller.state.voice_connected&&controller.state.outgoing_call_ringing);assert_eq!(controller.state.call_status_label,"Conectado · Esperando participantes");
        controller.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"2","channel_id":"20"})).await.unwrap();
        assert_eq!(controller.state.call_pending_invitees.iter().map(|person|person.id.as_str()).collect::<Vec<_>>(),vec!["3"]);
        assert!(!controller.state.outgoing_call_ringing);assert!(controller.call_people.contains_key("2"));
        controller.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"2","channel_id":null})).await.unwrap();
        assert!(!controller.state.outgoing_call_ringing,"a departed first peer must not restart the old ring");
        let future=controller.voice_lifecycle.private_calls.outgoing.pending["3"].deadline+Duration::from_millis(1);
        controller.expire_private_calls_at(future).await;
        assert!(controller.state.call_pending_invitees.is_empty());assert_eq!(controller.state.call_status_label,"Conectado");
        assert!(!controller.state.call_status_label.contains("micrófono"));assert_eq!(controller.voice_channel,"20");
        assert!(controller.api.is_none()&&controller.gateway.is_none()&&controller.media.is_none());
    }
    #[tokio::test]
    async fn accepting_or_service_cancel_before_http_ack_never_resurrects_a_pending_tile() {
        let mut controller=call();let ack=flight(&mut controller,&["2","3"],9);
        controller.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"2","channel_id":"20"})).await.unwrap();
        controller.observe_private_call_event("CALL_UPDATE",&json!({"channel_id":"20","ringing":["3"]}));
        controller.observe_private_call_event("CALL_UPDATE",&json!({"channel_id":"20","ringing":[]}));
        controller.complete_private_ring(&ack).unwrap();
        assert!(controller.state.call_pending_invitees.is_empty());assert!(!controller.state.outgoing_call_ringing);
        assert!(controller.voice_lifecycle.private_calls.outgoing.finished.contains("2"));
        assert!(controller.voice_lifecycle.private_calls.outgoing.finished.contains("3"));
        assert!(!controller.voice_lifecycle.private_calls.outcomes.iter().any(|entry|entry.3==EndReason::ExplicitDecline));
    }
    #[tokio::test]
    async fn ten_second_deadlines_are_per_recipient_and_stale_completions_cannot_end_a_reinvite() {
        let mut controller=call();let now=Instant::now();
        let first=flight(&mut controller,&["2"],1);controller.complete_private_ring(&first).unwrap();
        let second=flight(&mut controller,&["3"],2);controller.complete_private_ring(&second).unwrap();
        controller.voice_lifecycle.private_calls.outgoing.pending.get_mut("2").unwrap().deadline=now+Duration::from_secs(10);
        controller.voice_lifecycle.private_calls.outgoing.pending.get_mut("3").unwrap().deadline=now+Duration::from_secs(13);
        controller.expire_private_calls_at(now+Duration::from_millis(10001)).await;
        assert!(!controller.voice_lifecycle.private_calls.outgoing.pending.contains_key("2"));assert!(controller.voice_lifecycle.private_calls.outgoing.pending.contains_key("3"));
        assert_eq!(controller.voice_lifecycle.private_calls.outcomes.back().unwrap().3,EndReason::NoAnswer);
        controller.voice_lifecycle.private_calls.outgoing.closing.insert("2".into(),Closing{token:1});
        controller.observe_private_call_event("CALL_UPDATE",&json!({"channel_id":"20","ringing":[]}));
        assert!(controller.voice_lifecycle.private_calls.outgoing.closing.contains_key("2"),"Gateway removal does not ACK an older queued HTTP stop");
        let attempt=UiAction{kind:"call-invite".into(),id:"2".into(),channel_id:"20".into(),extra:json!({"voiceAttempt":7}).to_string(),..Default::default()};
        assert!(controller.invite_private_call(&attempt).unwrap_err().contains("cancelación"));
        let cancel_ack=UiAction{kind:"__private-ring-stop".into(),id:"20".into(),extra:json!({"voiceAttempt":7,"token":1,"recipient":"2"}).to_string(),..Default::default()};
        controller.complete_private_ring_stop(&cancel_ack);assert!(!controller.voice_lifecycle.private_calls.outgoing.closing.contains_key("2"));
        let reinvite=flight(&mut controller,&["2"],3);controller.complete_private_ring(&reinvite).unwrap();
        controller.voice_lifecycle.private_calls.outgoing.pending.get_mut("2").unwrap().deadline=now+Duration::from_secs(24);
        controller.voice_lifecycle.private_calls.outgoing.closing.insert("2".into(),Closing{token:4});
        controller.complete_private_ring_stop(&cancel_ack);
        assert_eq!(controller.voice_lifecycle.private_calls.outgoing.closing["2"].token,4);
        controller.complete_private_ring(&first).unwrap();controller.expire_private_calls_at(now+Duration::from_secs(12)).await;
        assert_eq!(controller.voice_lifecycle.private_calls.outgoing.pending["2"].token,3);
    }
    #[tokio::test]
    async fn incoming_expiry_is_ten_seconds_no_answer_is_not_decline_and_new_instances_are_independent() {
        let(mut controller,_)=Controller::test_fixture();controller.state.current_user=Some(Person{id:"1".into(),..Default::default()});
        controller.dispatch("CALL_CREATE",json!({"channel_id":"30","ringing":["1"],"voice_states":[{"user_id":"2","channel_id":"30"}]})).await.unwrap();
        let first=controller.state.incoming_calls[0].instance_id;let deadline=controller.voice_lifecycle.private_calls.incoming["30"].deadline.unwrap();
        controller.expire_private_calls_at(deadline-Duration::from_millis(1)).await;assert!(controller.state.incoming_calls[0].ringing);
        controller.expire_private_calls_at(deadline).await;assert!(!controller.state.incoming_calls[0].ringing);
        assert_eq!(controller.voice_lifecycle.private_calls.outcomes.back().unwrap().3,EndReason::NoAnswer);
        controller.dispatch("CALL_UPDATE",json!({"channel_id":"30","ringing":["1"]})).await.unwrap();
        assert_eq!(controller.state.incoming_calls[0].instance_id,first);assert!(!controller.state.incoming_calls[0].ringing);
        controller.dispatch("CALL_CREATE",json!({"channel_id":"30","ringing":["1"]})).await.unwrap();
        let second=controller.state.incoming_calls[0].instance_id;assert!(second>first&&controller.state.incoming_calls[0].ringing);
        controller.end_incoming_invitation("30",first,true);assert!(controller.state.incoming_calls[0].ringing,"old accept cannot dismiss a new invitation");
        controller.end_incoming_invitation("30",second,true);
        controller.dispatch("CALL_UPDATE",json!({"channel_id":"30","ringing":["1"]})).await.unwrap();
        assert_eq!(controller.state.incoming_calls[0].instance_id,second);assert!(!controller.state.incoming_calls[0].ringing);
        controller.expire_private_calls_at(deadline+Duration::from_secs(20)).await;assert_eq!(controller.state.incoming_calls[0].instance_id,second);
        assert!(controller.api.is_none()&&controller.gateway.is_none()&&controller.media.is_none());
    }
    #[tokio::test]
    async fn solo_idle_wait_starts_after_pending_work_and_never_applies_to_guild_voice() {
        let mut controller=call();let now=Instant::now();controller.voice_lifecycle.private_calls.outgoing.idle_since=Some(now);
        controller.expire_private_calls_at(now+Duration::from_secs(119)).await;assert_eq!(controller.voice_channel,"20");
        let ack=flight(&mut controller,&["2"],1);controller.complete_private_ring(&ack).unwrap();
        assert!(controller.voice_lifecycle.private_calls.outgoing.idle_since.is_none());
        controller.observe_private_call_event("CALL_UPDATE",&json!({"channel_id":"20","ringing":[]}));controller.refresh_call();
        let idle=controller.voice_lifecycle.private_calls.outgoing.idle_since.unwrap();
        controller.expire_private_calls_at(idle+Duration::from_secs(120)).await;assert!(controller.voice_channel.is_empty());
        let mut guild=call();guild.voice_guild="500".into();guild.refresh_call();
        guild.expire_private_calls_at(now+Duration::from_secs(900)).await;assert_eq!(guild.voice_channel,"20");assert!(guild.state.voice_connected);
    }
    #[tokio::test]
    async fn remote_audio_flags_remain_distinct_from_local_gain_and_preserve_partial_updates() {
        let mut controller=call();
        controller.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"2","channel_id":"20","self_mute":true,"self_deaf":false,"mute":true})).await.unwrap();
        controller.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"2","self_mute":false,"deaf":true})).await.unwrap();
        let remote=&controller.call_people["2"];assert!(!remote.self_mute&&remote.server_mute&&remote.server_deaf&&!remote.self_deaf);
        controller.media_preferences.user_volumes.insert("2".into(),0.);controller.refresh_call();
        assert_eq!(controller.call_people["2"].volume_percent,0.);assert!(controller.call_people["2"].server_mute);
        controller.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"2","mute":false,"deaf":false})).await.unwrap();
        assert!(!controller.call_people["2"].server_mute&&!controller.call_people["2"].server_deaf);
        assert!(!controller.state.self_mute&&!controller.state.self_deaf);
    }
    #[tokio::test]
    async fn explicit_call_rosters_reconcile_departures_but_omitted_and_null_metadata_preserve_peers() {
        let mut controller=call();
        controller.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"2","channel_id":"20"})).await.unwrap();
        let ack=flight(&mut controller,&["3"],1);controller.complete_private_ring(&ack).unwrap();
        for metadata in [json!({"channel_id":"20"}),json!({"channel_id":"20","voice_states":null})] {
            controller.dispatch("CALL_UPDATE",metadata).await.unwrap();assert!(controller.call_people.contains_key("2"));
        }
        controller.dispatch("CALL_UPDATE",json!({"channel_id":"20","voice_states":[]})).await.unwrap();
        assert!(!controller.call_people.contains_key("2"));assert!(controller.call_people.contains_key("1"));
        assert_eq!(controller.state.call_pending_invitees.iter().map(|person|person.id.as_str()).collect::<Vec<_>>(),vec!["3"]);
        assert!(controller.voice_lifecycle.private_calls.outgoing.idle_since.is_none());
        controller.dispatch("CALL_UPDATE",json!({"channel_id":"20","voice_states":[{"user_id":"3","channel_id":"20"}]})).await.unwrap();
        assert!(controller.call_people.contains_key("3")&&controller.state.call_pending_invitees.is_empty());
        controller.dispatch("CALL_UPDATE",json!({"channel_id":"20","voice_states":[]})).await.unwrap();
        assert!(controller.voice_lifecycle.private_calls.outgoing.idle_since.is_some());assert!(controller.state.voice_connected);
    }
    #[test]
    fn scope_replacement_and_session_cleanup_cancel_queued_work_and_drop_synthetic_people() {
        let mut controller=call();let old=flight(&mut controller,&["2"],1);let cancel=controller.voice_lifecycle.private_calls.outgoing.cancel.clone();
        controller.clear_private_outgoing();assert!(cancel.is_cancelled());
        controller.voice_attempt=8;controller.prepare_private_call(&json!([{"id":"2","username":"New attempt"}]));
        controller.complete_private_ring(&old).unwrap();assert!(controller.state.call_pending_invitees.is_empty());
        controller.track_incoming_invitation("30",11,true,true);let inbound=controller.voice_lifecycle.private_calls.incoming["30"].cancel.clone();
        controller.reset_session_view();assert!(inbound.is_cancelled());
        assert!(controller.voice_lifecycle.private_calls.incoming.is_empty()&&controller.voice_lifecycle.private_calls.outgoing.candidates.is_empty());
        assert!(controller.state.call_pending_invitees.is_empty()&&controller.state.call_invite_candidates.is_empty());
    }
    #[tokio::test]
    async fn group_member_added_while_viewing_another_chat_becomes_eligible_without_being_invited() {
        let mut controller=call();controller.state.selected_channel_id="99".into();
        controller.dispatch("CHANNEL_RECIPIENT_ADD",json!({"channel_id":"20","user":{"id":"4","username":"New synthetic member"}})).await.unwrap();
        assert!(controller.state.call_invite_candidates.iter().any(|person|person.id=="4"&&person.name=="New synthetic member"));
        assert!(controller.state.call_pending_invitees.is_empty()&&controller.state.call_invite_busy_ids.is_empty());
        assert!(!controller.state.outgoing_call_ringing);assert_eq!(controller.state.selected_channel_id,"99");
        controller.dispatch("CHANNEL_RECIPIENT_ADD",json!({"channel_id":"98","user":{"id":"5","username":"Unrelated synthetic member"}})).await.unwrap();
        assert!(!controller.state.call_invite_candidates.iter().any(|person|person.id=="5"));
        assert!(controller.api.is_none()&&controller.gateway.is_none()&&controller.media.is_none());
    }
    #[tokio::test]
    async fn group_member_removal_ends_only_its_pending_or_inflight_invitation_and_stale_ack_cannot_restore_it() {
        let mut controller=call();controller.state.selected_channel_id="99".into();
        let ack=flight(&mut controller,&["2","3"],10);
        controller.confirm_private_recipient("2",10,Instant::now());controller.refresh_call();
        assert_eq!(controller.state.call_pending_invitees.len(),1);
        for id in ["2","3"] {
            controller.dispatch("CHANNEL_RECIPIENT_REMOVE",json!({"channel_id":"20","user":{"id":id}})).await.unwrap();
        }
        controller.complete_private_ring(&ack).unwrap();
        assert!(controller.state.call_pending_invitees.is_empty()&&controller.state.call_invite_busy_ids.is_empty());
        assert!(!controller.state.call_invite_candidates.iter().any(|person|matches!(person.id.as_str(),"2"|"3")));
        controller.dispatch("CHANNEL_RECIPIENT_ADD",json!({"channel_id":"20","user":{"id":"2","username":"Rejoined synthetic member"}})).await.unwrap();
        controller.complete_private_ring(&ack).unwrap();
        assert!(controller.state.call_invite_candidates.iter().any(|person|person.id=="2"));assert!(controller.state.call_pending_invitees.is_empty());
        assert!(controller.state.voice_connected);assert_eq!(controller.voice_channel,"20");
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndReason { Accepted, ExplicitDecline, NoAnswer, EndedByService, Cancelled }
struct Pending { person: Person, token: u64, deadline: Instant }
struct Flight { token: u64, revision: u64 }
struct Closing { token: u64 }
struct Incoming { instance: u64, service_ringing: bool, deadline: Option<Instant>, ended: Option<EndReason>, cancel:tokio_util::sync::CancellationToken }
#[derive(Default)]
struct Outgoing {
    channel: String,
    attempt: u64,
    candidates: Vec<Person>,
    pending: HashMap<String,Pending>,
    flights: HashMap<String,Flight>,
    closing: HashMap<String,Closing>,
    finished: HashSet<String>,
    ringing_revision: u64,
    service_ringing: HashSet<String>,
    first_peer_joined: bool,
    idle_since: Option<Instant>,
    cancel:tokio_util::sync::CancellationToken,
}
#[derive(Default)]
pub(super) struct Calls {
    sequence: u64,
    outgoing: Outgoing,
    incoming: HashMap<String,Incoming>,
    outcomes: VecDeque<(String,String,u64,EndReason)>,
}
impl Calls {
    fn outcome(&mut self,channel:&str,user:&str,token:u64,reason:EndReason) {
        if self.outcomes.len()>=64 {self.outcomes.pop_front();}
        self.outcomes.push_back((channel.into(),user.into(),token,reason));
    }
    fn end_recipient(&mut self,id:&str,reason:EndReason) {
        let token=self.outgoing.pending.remove(id).map(|pending|pending.token)
            .or_else(||self.outgoing.flights.remove(id).map(|flight|flight.token));
        self.outgoing.flights.remove(id);
        self.outgoing.finished.insert(id.into());
        if let Some(token)=token {let channel=self.outgoing.channel.clone();self.outcome(&channel,id,token,reason);}
    }
}
impl Controller {
    pub(in crate::controller) fn clear_private_call_state(&mut self) {
        self.voice_lifecycle.private_calls.outgoing.cancel.cancel();
        for incoming in self.voice_lifecycle.private_calls.incoming.values(){incoming.cancel.cancel();}
        self.voice_lifecycle.private_calls=Default::default();
        self.state.call_pending_invitees.clear();self.state.call_invite_candidates.clear();self.state.call_invite_busy_ids.clear();
    }
    pub(super) fn clear_private_outgoing(&mut self) {
        let recipients=self.voice_lifecycle.private_calls.outgoing.pending.keys().chain(self.voice_lifecycle.private_calls.outgoing.flights.keys()).cloned().collect::<HashSet<_>>();
        for id in recipients {self.voice_lifecycle.private_calls.end_recipient(&id,EndReason::Cancelled);}
        self.voice_lifecycle.private_calls.outgoing.cancel.cancel();
        self.voice_lifecycle.private_calls.outgoing=Default::default();
        self.state.call_pending_invitees.clear();self.state.call_invite_candidates.clear();self.state.call_invite_busy_ids.clear();
    }
    pub(super) fn prepare_private_call(&mut self,recipients:&Value) {
        self.clear_private_outgoing();
        if !self.voice_guild.is_empty()||self.voice_channel.is_empty() {return;}
        let me=self.me();
        let mut candidates=items(recipients).iter().take(25).map(parsing::person)
            .filter(|person|parsing::valid_id(&person.id)&&person.id!=me&&!person.is_bot).collect::<Vec<_>>();
        if candidates.is_empty() {
            if self.state.selected_channel_id==self.voice_channel {candidates.extend(self.state.group_members.iter().map(|member|member.user.clone()));}
            if let Some(dm)=self.state.direct_messages.iter().find(|dm|dm.id==self.voice_channel)
                && let Some(person)=self.people.get(&dm.recipient_id) {candidates.push(person.clone());}
        }
        let mut seen=HashSet::new();candidates.retain(|person|parsing::valid_id(&person.id)&&person.id!=me&&!person.is_bot&&seen.insert(person.id.clone()));candidates.truncate(25);
        for person in &candidates {self.remember(person.clone());}
        let outgoing=&mut self.voice_lifecycle.private_calls.outgoing;
        outgoing.channel=self.voice_channel.clone();outgoing.attempt=self.voice_attempt;outgoing.candidates=candidates;
    }
    fn private_context_current(&self)->bool {
        let outgoing=&self.voice_lifecycle.private_calls.outgoing;
        self.voice_guild.is_empty()&&!self.voice_channel.is_empty()&&outgoing.channel==self.voice_channel&&outgoing.attempt==self.voice_attempt
    }
    pub(super) fn sync_private_call_snapshot(&mut self,now:Instant) {
        if !self.private_context_current() {
            self.state.call_pending_invitees.clear();self.state.call_invite_candidates.clear();self.state.call_invite_busy_ids.clear();self.state.outgoing_call_ringing=false;return;
        }
        let me=self.me();let peers=self.call_people.keys().filter(|id|*id!=&me).cloned().collect::<Vec<_>>();
        for peer in &peers {self.voice_lifecycle.private_calls.end_recipient(peer,EndReason::Accepted);}
        let outgoing=&mut self.voice_lifecycle.private_calls.outgoing;
        if !peers.is_empty(){outgoing.first_peer_joined=true;}
        self.state.call_pending_invitees=outgoing.pending.values().filter(|pending|pending.deadline>now).map(|pending|pending.person.clone()).collect();
        self.state.call_pending_invitees.sort_by(|a,b|a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        self.state.call_invite_candidates=outgoing.candidates.clone();
        self.state.call_invite_busy_ids=outgoing.flights.keys().chain(outgoing.closing.keys()).cloned().collect();
        self.state.call_invite_busy_ids.sort();self.state.call_invite_busy_ids.dedup();
        self.state.outgoing_call_ringing=!self.state.call_pending_invitees.is_empty()&&!outgoing.first_peer_joined;
        if self.state.voice_connected&&peers.is_empty()&&outgoing.pending.is_empty()&&outgoing.flights.is_empty() {
            outgoing.idle_since.get_or_insert(now);
        }else{outgoing.idle_since=None;}
    }
    pub(super) fn observe_private_call_event(&mut self,kind:&str,value:&Value) {
        if !self.private_context_current()||s(value,"channel_id")!=self.voice_channel{return;}
        if kind=="CALL_DELETE" {self.clear_private_outgoing();return;}
        if let Some(ringing)=value["ringing"].as_array() {
            let me=self.me();let ids=ringing.iter().filter_map(Value::as_str).filter(|id|parsing::valid_id(id)&&*id!=me).map(str::to_owned).collect::<HashSet<_>>();
            let ended=self.voice_lifecycle.private_calls.outgoing.pending.keys().filter(|id|!ids.contains(*id)).cloned().collect::<Vec<_>>();
            for id in ended {self.voice_lifecycle.private_calls.end_recipient(&id,EndReason::EndedByService);}
            let scope=&mut self.voice_lifecycle.private_calls.outgoing;
            scope.ringing_revision=scope.ringing_revision.wrapping_add(1);scope.service_ringing=ids;
            // A queued stop can still arrive after a Gateway removal. Keep
            // reinvites blocked until that exact HTTP cancellation is ACKed.
            let confirmed=scope.flights.iter().filter(|(id,_)|scope.service_ringing.contains(*id)).map(|(id,flight)|(id.clone(),flight.token)).collect::<Vec<_>>();
            for (id,token) in confirmed {self.confirm_private_recipient(&id,token,Instant::now());}
        }
    }
    pub(in crate::controller) fn observe_private_recipient(&mut self,kind:&str,value:&Value) {
        if !self.private_context_current()||s(value,"channel_id")!=self.voice_channel{return;}
        let mut id=s(&value["user"],"id");if id.is_empty(){id=s(value,"user_id");}
        if !parsing::valid_id(&id)||id==self.me(){return;}
        if kind=="CHANNEL_RECIPIENT_REMOVE" {
            self.voice_lifecycle.private_calls.end_recipient(&id,EndReason::EndedByService);
            self.voice_lifecycle.private_calls.outgoing.candidates.retain(|person|person.id!=id);
        }else if kind=="CHANNEL_RECIPIENT_ADD" {
            let person=if value["user"].is_object(){Some(parsing::person(&value["user"]))}else{self.people.get(&id).cloned()};
            if let Some(person)=person.filter(|person|!person.is_bot&&person.id==id) {
                self.remember(person.clone());
                let candidates=&mut self.voice_lifecycle.private_calls.outgoing.candidates;
                if candidates.len()<25&&!candidates.iter().any(|known|known.id==id){candidates.push(person);}
            }
        }
        // A membership delta never sends an invitation by itself. A pending
        // HTTP batch may complete later, but removed IDs no longer have flights
        // or candidate identities and cannot recreate a provisional tile.
        self.refresh_call();
    }
    fn confirm_private_recipient(&mut self,id:&str,token:u64,now:Instant) {
        if self.call_people.contains_key(id)||!self.private_context_current(){return;}
        let scope=&mut self.voice_lifecycle.private_calls.outgoing;
        if scope.finished.contains(id)||scope.pending.contains_key(id)||scope.closing.contains_key(id){return;}
        if let Some(person)=scope.candidates.iter().find(|person|person.id==id).cloned() {
            scope.pending.insert(id.into(),Pending{person,token,deadline:now+RESPONSE_WINDOW});scope.idle_since=None;
        }
    }
    pub(in crate::controller) fn invite_private_call(&mut self,action:&UiAction)->Result<(),String> {
        let context:Value=serde_json::from_str(&action.extra).unwrap_or_default();
        if !self.private_context_current()||action.channel_id!=self.voice_channel||context["voiceAttempt"].as_u64()!=Some(self.voice_attempt)||!self.state.voice_connected {
            return Err("Selecciona la llamada privada actual para añadir a una persona.".into());
        }
        self.send_private_invites(Some(&action.id),true)
    }
    pub(super) fn send_private_invites(&mut self,target:Option<&str>,explicit:bool)->Result<(),String> {
        if !self.private_context_current(){return Err("No hay una llamada privada disponible.".into());}
        let me=self.me();let scope=&self.voice_lifecycle.private_calls.outgoing;
        let recipients=scope.candidates.iter().filter(|person|target.is_none_or(|id|person.id==id))
            .filter(|person|person.id!=me&&!person.is_bot&&!self.call_people.contains_key(&person.id)&&!scope.pending.contains_key(&person.id)&&!scope.flights.contains_key(&person.id)&&!scope.closing.contains_key(&person.id))
            .map(|person|person.id.clone()).collect::<Vec<_>>();
        if recipients.is_empty(){return Err("La persona ya está conectada, tiene una invitación pendiente o aún se está confirmando su cancelación.".into());}
        let api=self.api()?;let channel=self.voice_channel.clone();let attempt=self.voice_attempt;
        let calls=&mut self.voice_lifecycle.private_calls;calls.sequence=calls.sequence.wrapping_add(1).max(1);let token=calls.sequence;
        let scope=&mut calls.outgoing;
        if explicit {scope.first_peer_joined=self.call_people.keys().any(|id|id!=&me);}
        scope.idle_since=None;
        for id in &recipients {scope.finished.remove(id);scope.flights.insert(id.clone(),Flight{token,revision:scope.ringing_revision});}
        let cancel=scope.cancel.clone();
        let mut done=UiAction::id("__private-ring",&channel);done.extra=json!({"voiceAttempt":attempt,"token":token,"recipients":recipients}).to_string();
        let (path,body)=invitation_request(&channel,&recipients,true);
        self.launch(done,None,async move {tokio::select!{biased;_=cancel.cancelled()=>Ok(Value::Null),result=api.request(Method::POST,&path,Some(body))=>result.map_err(|error|error.to_string())}});
        self.refresh_call();Ok(())
    }
    pub(in crate::controller) fn complete_private_ring(&mut self,action:&UiAction)->Result<(),String> {
        let meta:Value=serde_json::from_str(&action.extra).unwrap_or_default();let token=meta["token"].as_u64().unwrap_or(0);
        if !self.private_context_current()||action.id!=self.voice_channel||meta["voiceAttempt"].as_u64()!=Some(self.voice_attempt){return Ok(());}
        for id in items(&meta["recipients"]).iter().filter_map(Value::as_str) {
            let Some(flight)=self.voice_lifecycle.private_calls.outgoing.flights.remove(id) else {continue;};
            if flight.token!=token {self.voice_lifecycle.private_calls.outgoing.flights.insert(id.into(),flight);continue;}
            let scope=&self.voice_lifecycle.private_calls.outgoing;
            if scope.ringing_revision>flight.revision&&!scope.service_ringing.contains(id) {
                self.voice_lifecycle.private_calls.end_recipient(id,EndReason::EndedByService);
            }else{self.confirm_private_recipient(id,token,Instant::now());}
        }
        self.refresh_call();Ok(())
    }
    pub(in crate::controller) fn private_ring_failed(&mut self,action:&UiAction) {
        let meta:Value=serde_json::from_str(&action.extra).unwrap_or_default();
        if action.id!=self.voice_channel||meta["voiceAttempt"].as_u64()!=Some(self.voice_attempt){return;}
        let token=meta["token"].as_u64().unwrap_or(0);
        self.voice_lifecycle.private_calls.outgoing.flights.retain(|_,flight|flight.token!=token);
        self.refresh_call();
    }
    fn launch_recipient_cancel(&mut self,id:&str,token:u64) {
        let Ok(api)=self.api() else {return;};let channel=self.voice_channel.clone();let attempt=self.voice_attempt;let user=id.to_owned();
        self.voice_lifecycle.private_calls.outgoing.closing.insert(user.clone(),Closing{token});
        let cancel=self.voice_lifecycle.private_calls.outgoing.cancel.clone();
        let mut done=UiAction::id("__private-ring-stop",&channel);done.extra=json!({"voiceAttempt":attempt,"token":token,"recipient":user}).to_string();
        let (path,body)=invitation_request(&channel,&[user],false);
        self.launch(done,None,async move {tokio::select!{biased;_=cancel.cancelled()=>Ok(Value::Null),result=api.request(Method::POST,&path,Some(body))=>result.map_err(|error|error.to_string())}});
    }
    pub(in crate::controller) fn complete_private_ring_stop(&mut self,action:&UiAction) {
        let meta:Value=serde_json::from_str(&action.extra).unwrap_or_default();let user=s(&meta,"recipient");
        if action.id==self.voice_channel&&meta["voiceAttempt"].as_u64()==Some(self.voice_attempt)
            && self.voice_lifecycle.private_calls.outgoing.closing.get(&user).is_some_and(|closing|Some(closing.token)==meta["token"].as_u64()) {
            self.voice_lifecycle.private_calls.outgoing.closing.remove(&user);self.refresh_call();
        }
    }
    pub(in crate::controller) fn incoming_service_ring(&self,channel:&str)->Option<bool> {
        self.voice_lifecycle.private_calls.incoming.get(channel).map(|incoming|incoming.service_ringing)
    }
    pub(in crate::controller) fn track_incoming_invitation(&mut self,channel:&str,instance:u64,service_ringing:bool,fresh:bool)->bool {
        let calls=&mut self.voice_lifecycle.private_calls;
        if fresh||!calls.incoming.contains_key(channel) {
            if let Some(previous)=calls.incoming.get(channel){previous.cancel.cancel();}
            if calls.incoming.len()>=32 {calls.incoming.retain(|_,incoming|{if incoming.deadline.is_none(){incoming.cancel.cancel();false}else{true}});}
            if calls.incoming.len()>=32 {return false;}
            calls.incoming.insert(channel.into(),Incoming{instance,service_ringing,deadline:service_ringing.then(||Instant::now()+RESPONSE_WINDOW),ended:None,cancel:tokio_util::sync::CancellationToken::new()});
        }
        let incoming=calls.incoming.get_mut(channel).unwrap();
        incoming.service_ringing=service_ringing;
        if !service_ringing {incoming.deadline=None;}
        service_ringing&&incoming.ended.is_none()
    }
    pub(in crate::controller) fn end_incoming_invitation(&mut self,channel:&str,instance:u64,accepted:bool) {
        let me=self.me();let reason=if accepted{EndReason::Accepted}else{EndReason::ExplicitDecline};
        if let Some(incoming)=self.voice_lifecycle.private_calls.incoming.get_mut(channel)
            && (instance==0||incoming.instance==instance) {
            incoming.deadline=None;incoming.ended=Some(reason);
            incoming.cancel.cancel();
            let actual=incoming.instance;self.voice_lifecycle.private_calls.outcome(channel,&me,actual,reason);
            for call in self.state.incoming_calls.iter_mut().filter(|call|call.channel_id==channel&&call.instance_id==actual) {call.ringing=false;call.unread=false;}
        }
    }
    pub(in crate::controller) fn remove_incoming_invitation(&mut self,channel:&str) {if let Some(incoming)=self.voice_lifecycle.private_calls.incoming.remove(channel){incoming.cancel.cancel();}}
    pub(in crate::controller) async fn expire_private_calls_at(&mut self,now:Instant) {
        let expired=self.voice_lifecycle.private_calls.incoming.iter().filter(|(_,incoming)|incoming.deadline.is_some_and(|deadline|deadline<=now))
            .map(|(channel,incoming)|(channel.clone(),incoming.instance)).collect::<Vec<_>>();
        for (channel,instance) in expired {
            let current=self.state.incoming_calls.iter().any(|call|call.channel_id==channel&&call.instance_id==instance&&call.ringing);
            if let Some(incoming)=self.voice_lifecycle.private_calls.incoming.get_mut(&channel) {incoming.deadline=None;incoming.ended=Some(EndReason::NoAnswer);}
            if !current {continue;}
            let me=self.me();self.voice_lifecycle.private_calls.outcome(&channel,&me,instance,EndReason::NoAnswer);
            for call in self.state.incoming_calls.iter_mut().filter(|call|call.channel_id==channel&&call.instance_id==instance) {call.ringing=false;call.unread=false;}
            if let Ok(api)=self.api() {
                let cancel=self.voice_lifecycle.private_calls.incoming[&channel].cancel.clone();
                let (path,body)=invitation_request(&channel,&[me],false);
                let mut done=UiAction::id("__private-incoming-expired",&channel);done.extra=instance.to_string();
                self.launch(done,None,async move {tokio::select!{biased;_=cancel.cancelled()=>Ok(Value::Null),result=api.request(Method::POST,&path,Some(body))=>result.map_err(|error|error.to_string())}});
            }
            self.decorate_dms();self.publish();
        }
        if !self.private_context_current(){return;}
        let expired=self.voice_lifecycle.private_calls.outgoing.pending.iter().filter(|(_,pending)|pending.deadline<=now).map(|(id,pending)|(id.clone(),pending.token)).collect::<Vec<_>>();
        let changed=!expired.is_empty();
        for (id,token) in expired {self.voice_lifecycle.private_calls.end_recipient(&id,EndReason::NoAnswer);self.launch_recipient_cancel(&id,token);}
        if changed{self.sync_private_call_snapshot(now);}
        if self.voice_lifecycle.private_calls.outgoing.idle_since.is_some_and(|since|now.saturating_duration_since(since)>=SOLO_WINDOW) {
            self.stop_voice().await;self.status("La llamada privada se cerró tras dos minutos sin participantes ni invitaciones pendientes.");
        }else if changed{self.refresh_call();}
    }
}
