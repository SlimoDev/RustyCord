//! Bounded observations of private calls, separate from invitations and media.
use super::*;

const RECORD_LIMIT: usize = 64;
const UNANSWERED_LIMIT: usize = 4;
const REQUEST_HISTORY_LIMIT: usize = 4096;
const RESPONSE_WINDOW: Duration = Duration::from_secs(10);
fn newer_id(candidate: &str, previous: &str) -> bool {
    candidate.parse::<u64>().ok().zip(previous.parse::<u64>().ok()).is_some_and(|(candidate, previous)|candidate > previous)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn visible_group(c: &mut Controller) {
        c.state.signed_in = true;
        c.state.current_user = Some(Person {id:"1".into(),..Default::default()});
        c.state.view = "chat".into();c.state.selected_channel_id = "20".into();
        c.state.selected_channel_kind = "group".into();
        c.state.channel_access = ChannelAccess::private();
        c.state.direct_messages.push(NavItem {id:"20".into(),kind:"group".into(),..Default::default()});
    }
    #[tokio::test]
    async fn visible_demand_waits_for_sync_queries_once_and_never_acquires_media_or_rings() {
        let (mut c,_,mut replies) = Controller::test_fixture_with_replies();visible_group(&mut c);
        let (gateway,mut wire,task) = GatewayHandle::offline_recorder(SafetyGate::default());
        c.gateway = Some(gateway.clone());c.state.connection_phase = ConnectionPhase::Connecting;
        c.demand_private_call("20",false).unwrap();c.poll_call_discovery();
        assert!(c.observed_calls.flight.is_none() && wire.try_recv().is_err());
        c.state.connection_phase = ConnectionPhase::Synchronized;
        for _ in 0..20 {c.poll_call_discovery();c.demand_private_call("20",false).unwrap();}
        assert_eq!(wire.recv().await.unwrap(),json!({"op":13,"d":{"channel_id":"20"}}));
        let done = replies.recv().await.unwrap();c.complete(done.action,done.result.unwrap()).await.unwrap();
        c.dispatch("CALL_CREATE",json!({"channel_id":"20","message_id":"100","ringing":[],
            "voice_states":[{"user_id":"2","channel_id":"20"},{"user_id":"3","channel_id":"20"}]})).await.unwrap();
        assert_eq!(c.state.incoming_calls[0].participants.len(),2);
        assert!(c.state.incoming_calls[0].is_active() && c.state.direct_messages[0].has_active_call);
        for _ in 0..20 {c.demand_private_call("20",false).unwrap();c.poll_call_discovery();}
        assert!(wire.try_recv().is_err() && c.media.is_none() && c.voice_channel.is_empty());
        assert!(!c.state.voice_connected && !c.state.voice_connecting && !c.state.incoming_calls[0].ringing);
        gateway.stop();task.await.unwrap();
    }
    #[tokio::test]
    async fn deleted_call_tombstone_rejects_old_query_response_and_new_instance_clears_media_flags() {
        let (mut c,_) = Controller::test_fixture();visible_group(&mut c);
        let call = json!({"channel_id":"20","message_id":"100","ringing":[],
            "voice_states":[{"user_id":"2","channel_id":"20","self_video":true,"self_stream":true}]});
        c.dispatch("CALL_CREATE",call.clone()).await.unwrap();
        c.dispatch("CALL_DELETE",json!({"channel_id":"20"})).await.unwrap();
        for kind in ["CALL_CREATE","CALL_UPDATE"] {c.dispatch(kind,call.clone()).await.unwrap();}
        assert!(c.state.incoming_calls.is_empty() && !c.state.direct_messages[0].has_active_call);
        c.dispatch("CALL_CREATE",json!({"channel_id":"20","message_id":"101","ringing":[],
            "voice_states":[{"user_id":"2","channel_id":"20"}]})).await.unwrap();
        let peer = &c.state.incoming_calls[0].call_participants[0];
        assert!(!peer.self_video && !peer.screen_sharing && !peer.camera_announcement_known);
    }
    #[tokio::test]
    async fn query_snapshot_cannot_overwrite_a_newer_call_roster_or_voice_departure() {
        for delta in ["CALL_UPDATE","VOICE_STATE_UPDATE"] {
            let (mut c,_,mut replies)=Controller::test_fixture_with_replies();visible_group(&mut c);
            c.state.connection_phase=ConnectionPhase::Synchronized;
            let old=json!({"channel_id":"20","message_id":"100","ringing":[],"voice_states":[{"user_id":"2","channel_id":"20"}]});
            c.dispatch("CALL_CREATE",old.clone()).await.unwrap();
            let (gateway,mut wire,task)=GatewayHandle::offline_recorder(SafetyGate::default());c.gateway=Some(gateway.clone());
            c.demand_private_call("20",false).unwrap();assert_eq!(wire.recv().await.unwrap()["op"],13);
            let done=replies.recv().await.unwrap();c.complete(done.action,done.result.unwrap()).await.unwrap();
            let value=if delta=="CALL_UPDATE" {json!({"channel_id":"20","message_id":"100","ringing":[],"voice_states":[]})}
                else {json!({"user_id":"2","channel_id":null})};
            c.dispatch(delta,value).await.unwrap();
            assert!(c.observed_calls.flight.is_some(),"a delta does not acknowledge the query response");
            c.dispatch("CALL_CREATE",old).await.unwrap();
            assert!(c.state.incoming_calls[0].participants.is_empty());
            assert!(!c.state.direct_messages[0].has_active_call&&c.observed_calls.flight.is_none());
            gateway.stop();task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn complete_empty_roster_ends_activity_but_omitted_and_truncated_rosters_are_unknown() {
        let (mut c,_) = Controller::test_fixture();visible_group(&mut c);
        c.dispatch("CALL_CREATE",json!({"channel_id":"20","ringing":[],"voice_states":[{"user_id":"2"}]})).await.unwrap();
        c.dispatch("CALL_UPDATE",json!({"channel_id":"20","voice_states":null})).await.unwrap();
        assert_eq!(c.state.incoming_calls[0].participants.len(),1);
        let states:Vec<_> = (3..30).map(|id|json!({"user_id":id.to_string()})).collect();
        c.dispatch("CALL_UPDATE",json!({"channel_id":"20","voice_states":states})).await.unwrap();
        assert!(!c.state.incoming_calls[0].roster_complete && c.state.incoming_calls[0].participants.len()<=25);
        c.dispatch("CALL_UPDATE",json!({"channel_id":"20","ringing":[],"voice_states":[]})).await.unwrap();
        assert!(!c.state.incoming_calls[0].is_active() && !c.state.direct_messages[0].has_active_call);
    }
    async fn emitted_query(c:&mut Controller, replies:&mut mpsc::UnboundedReceiver<Completed>, wire:&mut mpsc::Receiver<Value>, explicit:bool) {
        let channel=c.state.selected_channel_id.clone();c.demand_private_call(&channel,explicit).unwrap();
        assert_eq!(wire.recv().await.unwrap(),json!({"op":13,"d":{"channel_id":channel}}));
        let done=replies.recv().await.unwrap();c.complete(done.action,done.result.unwrap()).await.unwrap();
    }
    fn expire_query(c:&mut Controller) {
        c.observed_calls.flight.as_mut().unwrap().deadline=Instant::now()-Duration::from_millis(1);
        c.poll_call_discovery();assert!(c.observed_calls.flight.is_none());
    }
    #[tokio::test]
    async fn timed_out_requery_keeps_older_authority_in_both_snapshot_orders_and_after_extra_creates() {
        for newer_first in [false,true] {
            let (mut c,_,mut replies)=Controller::test_fixture_with_replies();visible_group(&mut c);
            c.state.connection_phase=ConnectionPhase::Synchronized;
            let old=json!({"channel_id":"20","message_id":"100","ringing":[],"voice_states":[{"user_id":"2","channel_id":"20"}]});
            c.dispatch("CALL_CREATE",old.clone()).await.unwrap();
            let (gateway,mut wire,task)=GatewayHandle::offline_recorder(SafetyGate::default());c.gateway=Some(gateway.clone());
            emitted_query(&mut c,&mut replies,&mut wire,false).await;
            c.dispatch("CALL_UPDATE",json!({"channel_id":"20","message_id":"100","ringing":[],"voice_states":[]})).await.unwrap();
            expire_query(&mut c);
            emitted_query(&mut c,&mut replies,&mut wire,true).await;
            let empty=json!({"channel_id":"20","message_id":"100","ringing":[],"voice_states":[]});
            for snapshot in if newer_first {[empty,old.clone()]}else{[old.clone(),empty]} {
                c.dispatch("CALL_CREATE",snapshot).await.unwrap();
                assert!(c.state.incoming_calls[0].participants.is_empty());
            }
            assert!(c.observed_calls.flight.is_none()&&c.observed_calls.records["20"].queries.is_empty());
            // No nonce proves that every CREATE belonged to a query. Keep the
            // instance's ambiguity guard after the coarse response count ends.
            c.dispatch("CALL_CREATE",old).await.unwrap();
            assert!(c.state.incoming_calls[0].participants.is_empty()&&!c.state.direct_messages[0].has_active_call);
            assert!(c.demand_private_call("20",true).unwrap_err().contains("no identifican"));
            assert!(wire.try_recv().is_err(),"a spent ambiguous cohort cannot be rearmed by another click");
            c.dispatch("CALL_UPDATE",json!({"channel_id":"20","message_id":"100","ringing":[],"voice_states":[{"user_id":"3"}]})).await.unwrap();
            assert_eq!(c.state.incoming_calls[0].participants[0].id,"3");
            c.fresh_call_discovery_transport();
            c.demand_private_call("20",false).unwrap();assert!(wire.try_recv().is_err());
            emitted_query(&mut c,&mut replies,&mut wire,true).await;
            c.dispatch("CALL_CREATE",json!({"channel_id":"20","message_id":"100","ringing":[],"voice_states":[{"user_id":"3"}]})).await.unwrap();
            assert_eq!(c.state.incoming_calls[0].participants[0].id,"3");
            assert!(!c.state.incoming_calls[0].observation_stale);
            gateway.stop();task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn unknown_flush_retains_old_snapshot_guard_but_a_definite_unemitted_failure_releases_it() {
        let (mut c,_,mut replies)=Controller::test_fixture_with_replies();visible_group(&mut c);
        c.state.connection_phase=ConnectionPhase::Synchronized;
        let (gateway,mut wire,task)=GatewayHandle::offline_recorder(SafetyGate::default());c.gateway=Some(gateway.clone());
        c.demand_private_call("20",false).unwrap();wire.recv().await.unwrap();
        let done=replies.recv().await.unwrap();c.complete(done.action,json!({"error":"Synthetic unknown flush","notSent":false})).await.unwrap();
        assert_eq!(c.observed_calls.records["20"].queries.len(),1);
        c.demand_private_call("20",true).unwrap();wire.recv().await.unwrap();
        let done=replies.recv().await.unwrap();c.complete(done.action,json!({"error":"Synthetic known admission rejection","notSent":true})).await.unwrap();
        assert_eq!(c.observed_calls.records["20"].queries.len(),1);
        assert!(c.observed_calls.records["20"].ambiguous_floor.is_some());
        gateway.stop();task.await.unwrap();
    }
    #[tokio::test]
    async fn unanswered_limit_is_per_channel_and_silent_no_call_does_not_block_another_group() {
        let (mut c,_,mut replies)=Controller::test_fixture_with_replies();visible_group(&mut c);
        c.state.connection_phase=ConnectionPhase::Synchronized;
        let (gateway,mut wire,task)=GatewayHandle::offline_recorder(SafetyGate::default());c.gateway=Some(gateway.clone());
        for index in 0..UNANSWERED_LIMIT {emitted_query(&mut c,&mut replies,&mut wire,index>0).await;expire_query(&mut c);}
        assert!(c.demand_private_call("20",true).unwrap_err().contains("sin respuesta"));
        assert!(wire.try_recv().is_err());
        c.state.direct_messages.push(NavItem{id:"21".into(),kind:"group".into(),..Default::default()});c.state.selected_channel_id="21".into();
        emitted_query(&mut c,&mut replies,&mut wire,false).await;
        c.dispatch("CALL_DELETE",json!({"channel_id":"21"})).await.unwrap();
        assert!(c.observed_calls.flight.is_none());
        gateway.stop();task.await.unwrap();
    }
    #[tokio::test]
    async fn quiescent_no_call_slots_recycle_without_rearming_automatic_reads() {
        let (mut c,_,mut replies)=Controller::test_fixture_with_replies();visible_group(&mut c);
        c.state.connection_phase=ConnectionPhase::Synchronized;
        let (gateway,mut wire,task)=GatewayHandle::offline_recorder(SafetyGate::default());c.gateway=Some(gateway.clone());
        for number in 20..90 {
            let channel=number.to_string();
            if !c.state.direct_messages.iter().any(|item|item.id==channel) {c.state.direct_messages.push(NavItem{id:channel.clone(),kind:"group".into(),..Default::default()});}
            c.state.selected_channel_id=channel.clone();emitted_query(&mut c,&mut replies,&mut wire,true).await;
            c.dispatch("CALL_DELETE",json!({"channel_id":channel})).await.unwrap();
        }
        assert_eq!(c.observed_calls.records.len(),RECORD_LIMIT);
        let evicted=(20..90).map(|number|number.to_string()).find(|channel|!c.observed_calls.records.contains_key(channel)).unwrap();
        c.state.selected_channel_id=evicted.clone();c.demand_private_call(&evicted,false).unwrap();
        assert!(wire.try_recv().is_err()&&c.observed_calls.flight.is_none());
        emitted_query(&mut c,&mut replies,&mut wire,true).await;
        c.dispatch("CALL_DELETE",json!({"channel_id":evicted})).await.unwrap();
        c.dispatch("CALL_CREATE",json!({"channel_id":"999","message_id":"1000","voice_states":[{"user_id":"2"}],"ringing":[]})).await.unwrap();
        assert!(c.state.incoming_calls.iter().any(|call|call.channel_id=="999"));
        assert_eq!(c.observed_calls.records.len(),RECORD_LIMIT);
        gateway.stop();task.await.unwrap();
    }
    #[tokio::test]
    async fn bounded_authority_slots_never_evict_call_tombstones_to_admit_more_traffic() {
        let (mut c,_)=Controller::test_fixture();visible_group(&mut c);
        for number in 100..100+RECORD_LIMIT {
            let channel=number.to_string();let message=(number+1000).to_string();
            c.dispatch("CALL_CREATE",json!({"channel_id":channel,"message_id":message,"voice_states":[],"ringing":[]})).await.unwrap();
            c.dispatch("CALL_DELETE",json!({"channel_id":channel})).await.unwrap();
        }
        c.state.direct_messages.push(NavItem{id:"999".into(),kind:"group".into(),..Default::default()});c.state.selected_channel_id="999".into();
        assert!(c.demand_private_call("999",true).is_err());
        assert_eq!(c.observed_calls.records.len(),RECORD_LIMIT);assert!(c.observed_calls.flight.is_none());
        c.dispatch("CALL_CREATE",json!({"channel_id":"100","message_id":"1100","voice_states":[{"user_id":"2"}]})).await.unwrap();
        assert!(c.state.incoming_calls.is_empty());
    }
    #[tokio::test]
    async fn exact_admission_history_has_no_silent_first_read_suppression_and_manual_refresh_survives_capacity() {
        let (mut c,_,mut replies)=Controller::test_fixture_with_replies();visible_group(&mut c);
        c.state.connection_phase=ConnectionPhase::Synchronized;
        for number in 1000..1000+REQUEST_HISTORY_LIMIT {c.observed_calls.requested.insert(number.to_string());}
        assert!(!c.observed_calls.requested.contains("20"));
        assert!(c.demand_private_call("20",false).unwrap_err().contains("Actualizar llamada"));
        let (gateway,mut wire,task)=GatewayHandle::offline_recorder(SafetyGate::default());c.gateway=Some(gateway.clone());
        emitted_query(&mut c,&mut replies,&mut wire,true).await;
        assert_eq!(c.observed_calls.requested.len(),REQUEST_HISTORY_LIMIT);
        c.dispatch("CALL_DELETE",json!({"channel_id":"20"})).await.unwrap();
        c.observed_calls.records.remove("20");
        assert!(c.demand_private_call("20",false).is_err());assert!(wire.try_recv().is_err());
        gateway.stop();task.await.unwrap();
    }
    #[tokio::test]
    async fn discovery_capacity_never_aborts_normal_offline_chat_navigation_or_hides_cached_messages() {
        for authority_full in [false,true] {
            let (mut c,_,replies)=Controller::test_fixture_with_replies();visible_group(&mut c);
            if authority_full {
                for number in 100..100+RECORD_LIMIT {
                    let channel=number.to_string();let message=(number+1000).to_string();
                    c.dispatch("CALL_CREATE",json!({"channel_id":channel,"message_id":message,"voice_states":[],"ringing":[]})).await.unwrap();
                    c.dispatch("CALL_DELETE",json!({"channel_id":channel})).await.unwrap();
                }
            } else {
                for number in 1000..1000+REQUEST_HISTORY_LIMIT {c.observed_calls.requested.insert(number.to_string());}
            }
            c.cache.replace("20",vec![ChatMessage{id:"100".into(),channel_id:"20".into(),content:"Synthetic cached conversation".into(),..Default::default()}]);
            c.state.connection_phase=ConnectionPhase::Cooldown;
            c.select_channel("20").unwrap();
            assert_eq!(c.state.selected_channel_id,"20");assert_eq!(c.state.view,"chat");
            assert_eq!(c.state.messages[0].content,"Synthetic cached conversation");
            assert!(c.state.status.contains("límite"));
            assert!(c.api.is_none()&&c.media.is_none()&&c.observed_calls.flight.is_none()&&replies.is_empty());
        }
    }
    #[tokio::test]
    async fn remote_departure_publishes_without_local_voice_and_local_departure_cannot_be_resurrected() {
        let (mut c,_) = Controller::test_fixture();visible_group(&mut c);
        c.dispatch("CALL_CREATE",json!({"channel_id":"20","message_id":"100","ringing":[],
            "voice_states":[{"user_id":"1","channel_id":"20"},{"user_id":"2","channel_id":"20"}]})).await.unwrap();
        c.local_call_departure("20");
        c.dispatch("CALL_UPDATE",json!({"channel_id":"20","message_id":"100",
            "voice_states":[{"user_id":"1"},{"user_id":"2"}]})).await.unwrap();
        assert_eq!(c.state.incoming_calls[0].participants.iter().map(|p|p.id.as_str()).collect::<Vec<_>>(),["2"]);
        c.flush_snapshot(true);
        c.dispatch("VOICE_STATE_UPDATE",json!({"user_id":"2","channel_id":null})).await.unwrap();
        let snapshot=c.latest_snapshot.lock().unwrap().clone().unwrap();
        assert!(snapshot.incoming_calls[0].participants.is_empty() && !snapshot.direct_messages[0].has_active_call);
    }
    fn navigation(c: &Controller, kind: &str) -> UiAction {
        UiAction {kind:kind.into(),channel_id:c.state.selected_channel_id.clone(),extra:json!({"navigationContext":{
            "account":c.me(),"generation":c.generation,"guild":c.state.selected_guild_id,"view":c.state.view}}).to_string(),..Default::default()}
    }
    #[tokio::test]
    async fn ram_log_roundtrip_restores_live_cache_and_ignores_old_return_and_lost_permission() {
        let (mut c,_) = Controller::test_fixture();visible_group(&mut c);
        c.cache.replace("20",vec![ChatMessage {id:"100".into(),channel_id:"20".into(),content:"Synthetic live".into(),..Default::default()}]);
        c.refresh_messages();
        c.act(navigation(&c,"history")).await.unwrap();
        assert_eq!(c.state.view,"history");assert_eq!(c.state.selected_channel_id,"20");
        let old=navigation(&c,"return-chat");c.act(old.clone()).await.unwrap();
        assert_eq!(c.state.view,"chat");assert_eq!(c.state.messages[0].content,"Synthetic live");
        c.state.selected_channel_id="21".into();c.act(old).await.unwrap();assert_eq!(c.state.selected_channel_id,"21");
        c.state.selected_channel_id="20".into();c.act(navigation(&c,"history")).await.unwrap();
        c.state.channel_access.can_read_history=false;
        assert!(c.act(navigation(&c,"return-chat")).await.is_err());assert_eq!(c.state.view,"history");
    }
    #[tokio::test]
    async fn opening_ram_log_during_initial_load_retains_fresh_offline_demand() {
        let (mut c,_,replies) = Controller::test_fixture_with_replies();visible_group(&mut c);
        c.state.connection_phase=ConnectionPhase::Cooldown;c.loading_channel="20".into();
        c.act(navigation(&c,"history")).await.unwrap();
        assert!(c.loading_channel.is_empty());
        c.act(navigation(&c,"return-chat")).await.unwrap();
        assert_eq!(c.state.view,"chat");assert_eq!(c.state.selected_channel_id,"20");
        assert!(replies.is_empty() && c.ram_history_reload.is_none());
        c.reload_deferred_view();assert!(replies.is_empty(),"offline return never admits HTTP");
    }
    #[tokio::test]
    async fn valid_empty_cache_returns_locally_but_an_evicted_page_retains_ordinary_offline_demand() {
        let (mut c,_,replies)=Controller::test_fixture_with_replies();visible_group(&mut c);
        c.cache.replace("20",vec![]);
        c.act(navigation(&c,"history")).await.unwrap();
        c.act(navigation(&c,"return-chat")).await.unwrap();
        assert_eq!(c.state.view,"chat");assert!(replies.is_empty());
        c.act(navigation(&c,"history")).await.unwrap();
        for channel in ["21","22","23"] {c.cache.replace(channel,vec![]);}
        c.state.connection_phase=ConnectionPhase::Cooldown;
        c.act(navigation(&c,"return-chat")).await.unwrap();
        assert_eq!(c.state.selected_channel_id,"20");assert_eq!(c.state.view,"chat");
        assert!(replies.is_empty() && !c.cache.contains_channel("20"));
    }
}
#[derive(Default)]
struct Record {
    requested: bool,
    message: String,
    deleted: bool,
    revision: u64,
    departed_locally: bool,
    // A later explicit read must not forget an earlier unanswered snapshot.
    // The floor is retained until the entire overlapping cohort has replied.
    query_floor: Option<u64>,
    // No nonce identifies replies of overlapping reads. Never promote that
    // ambiguity into freshness for the same instance after a newer delta.
    ambiguous_floor: Option<u64>,
    live_revision: u64,
    queries: VecDeque<u64>,
    touched: u64,
}
struct Flight {
    channel: String,
    token: u64,
    event_seen: bool,
    written: bool,
    deadline: Instant,
}
#[derive(Default)]
pub(super) struct Observations {
    records: HashMap<String, Record>,
    demand: Option<(String,bool)>,
    flight: Option<Flight>,
    sequence: u64,
    book_clock: u64,
    // Exact session-only identities, bounded independently of the authority
    // book. At capacity, new channels require a visible explicit refresh.
    requested: HashSet<String>,
    cancel: tokio_util::sync::CancellationToken,
}
impl Controller {
    fn ensure_call_record(&mut self,channel:&str)->bool {
        if !self.observed_calls.records.contains_key(channel) && self.observed_calls.records.len()>=RECORD_LIMIT {
            // Only admission-only/no-call entries are dispensable. Real call
            // identities, departures and unanswered reads retain their guard.
            let disposable=self.observed_calls.records.iter().filter(|(known,record)|
                record.message.is_empty()&&!record.departed_locally&&record.queries.is_empty()
                    && !self.state.incoming_calls.iter().any(|call|call.channel_id==known.as_str())
                    && self.observed_calls.flight.as_ref().is_none_or(|flight|flight.channel.as_str()!=known.as_str())
                    && self.observed_calls.demand.as_ref().map(|(channel,_)|channel.as_str())!=Some(known.as_str()))
                .min_by_key(|(_,record)|record.touched).map(|(known,_)|known.clone());
            let Some(disposable)=disposable else {return false;};
            self.observed_calls.records.remove(&disposable);
        }
        self.observed_calls.book_clock=self.observed_calls.book_clock.wrapping_add(1);
        self.observed_calls.records.entry(channel.into()).or_default().touched=self.observed_calls.book_clock;
        true
    }
    fn call_discovery_status(&self, channel: &str, pending: bool, uncertain: bool, error: &str) {
        self.data("call-discovery", json!({"account":self.me(),"generation":self.generation,
            "channelId":channel,"pending":pending,"uncertain":uncertain,"error":error}));
    }
    pub(super) fn demand_private_call(&mut self, channel: &str, explicit: bool) -> Result<(), String> {
        if !self.state.signed_in || !self.state.direct_messages.iter().any(|item|
            item.id == channel && matches!(item.kind.as_str(), "dm" | "group")) { return Ok(()); }
        Self::valid(channel)?;
        if self.observed_calls.flight.as_ref().is_some_and(|flight| flight.channel == channel) {
            return Ok(());
        }
        if !explicit && self.observed_calls.requested.contains(channel) {return Ok(());}
        if !explicit && self.observed_calls.requested.len()>=REQUEST_HISTORY_LIMIT {
            return Err("Se alcanzó el límite de consultas automáticas de esta sesión. Usa Actualizar llamada para una consulta explícita.".into());
        }
        if !self.ensure_call_record(channel) {
            return Err("Se alcanzó el límite de llamadas o consultas aún sin confirmar de esta sesión.".into());
        }
        let record = self.observed_calls.records.entry(channel.into()).or_default();
        if record.requested && !explicit { return Ok(()); }
        if explicit && record.queries.is_empty() && record.ambiguous_floor.is_some_and(|floor|floor<record.revision) {
            return Err("Las respuestas de esta instancia no identifican su consulta. Se conservan los eventos más recientes; espera nuevos eventos de llamada o una sesión Gateway nueva, sin repetir la consulta.".into());
        }
        if record.queries.len()>=UNANSWERED_LIMIT {
            return Err("Las consultas anteriores de esta llamada siguen sin respuesta. No se enviarán más hasta recibir sus eventos o abrir una sesión nueva.".into());
        }
        if explicit { record.requested = false; }
        // Retain visible demand before cloning any connection/admitting work.
        self.observed_calls.demand = Some((channel.into(),explicit));
        self.poll_call_discovery();
        Ok(())
    }
    pub(super) fn suspend_call_discovery(&mut self) {
        self.observed_calls.cancel.cancel();
        self.observed_calls.cancel = tokio_util::sync::CancellationToken::new();
        for call in &mut self.state.incoming_calls { call.observation_stale = true; }
        // Initial visible demand survives Connecting. An admitted operation is
        // never rearmed by recovery; its record remains requested.
    }
    pub(super) fn fresh_call_discovery_transport(&mut self) {
        // READY_CATALOG is emitted only for a fresh READY, never RESUMED.
        // Old socket responses cannot belong to this new Gateway session.
        self.observed_calls.cancel.cancel();
        self.observed_calls.cancel=tokio_util::sync::CancellationToken::new();
        self.observed_calls.flight=None;
        for record in self.observed_calls.records.values_mut() {
            record.queries.clear();record.query_floor=None;record.ambiguous_floor=None;
        }
        for call in &mut self.state.incoming_calls {call.observation_stale=true;}
        // Keep seen/requested admission history and legitimate visible demand;
        // a fresh transport does not drain or replay the old query backlog.
    }
    pub(super) fn poll_call_discovery(&mut self) {
        if self.observed_calls.flight.as_ref().is_some_and(|flight| flight.written && Instant::now() >= flight.deadline) {
            let flight = self.observed_calls.flight.take().unwrap();
            self.call_discovery_status(&flight.channel, false, true,
                "Discord no confirmó la llamada. Puedes actualizarla explícitamente.");
        }
        if self.observed_calls.flight.is_some() || self.state.connection_phase != ConnectionPhase::Synchronized {
            return;
        }
        let Some((channel,explicit)) = self.observed_calls.demand.clone() else { return; };
        if channel != self.state.selected_channel_id || !self.state.selected_guild_id.is_empty() {
            self.observed_calls.demand = None;
            return;
        }
        let Some(gateway) = self.gateway.clone().filter(|gateway| gateway.is_ready()) else { return; };
        if self.observed_calls.records.get(&channel).is_some_and(|record| record.requested) {
            self.observed_calls.demand = None;
            return;
        }
        self.observed_calls.demand = None;
        if !explicit && self.observed_calls.requested.len()>=REQUEST_HISTORY_LIMIT {
            self.call_discovery_status(&channel,false,true,"Se alcanzó el límite de consultas automáticas. Usa Actualizar llamada para una consulta explícita.");
            return;
        }
        self.observed_calls.sequence = self.observed_calls.sequence.wrapping_add(1).max(1);
        let token = self.observed_calls.sequence;
        let record=self.observed_calls.records.entry(channel.clone()).or_default();
        if record.queries.len()>=UNANSWERED_LIMIT {
            self.call_discovery_status(&channel,false,true,"Las consultas anteriores siguen sin respuesta; no se repetirá la consulta.");
            return;
        }
        record.requested = true;
        if record.queries.is_empty() {record.query_floor=Some(record.revision);}
        else {
            let floor=record.query_floor.unwrap_or(record.revision);
            record.ambiguous_floor=Some(record.ambiguous_floor.map_or(floor,|old|old.min(floor)));
        }
        record.queries.push_back(token);
        if self.observed_calls.requested.len()<REQUEST_HISTORY_LIMIT {self.observed_calls.requested.insert(channel.clone());}
        self.observed_calls.flight = Some(Flight { channel: channel.clone(), token, event_seen: false,
            written: false, deadline: Instant::now() + RESPONSE_WINDOW });
        let cancel = self.observed_calls.cancel.child_token();
        let mut done = UiAction::id("__call-discovered", &channel);
        done.extra = token.to_string();
        self.call_discovery_status(&channel, true, false, "");
        self.launch(done, None, async move {
            if cancel.is_cancelled() {return Ok(json!({"notSent":true,"error":"La sesión cambió antes de admitir la consulta."}));}
            let result = tokio::select! { biased;
                _=cancel.cancelled()=>Err((false,"La sesión cambió; actualiza la llamada cuando se sincronice.".to_owned())),
                result=gateway.send(GatewayCommand::CallConnect { channel_id: channel })=>result.map_err(|error|
                    (matches!(error.kind,notdiscord_core::api::ErrorKind::QueueFull),error.to_string())),
            };
            Ok(match result { Ok(())=>json!({"written":true}), Err((not_sent,error))=>json!({"notSent":not_sent,"error":error}) })
        });
    }
    pub(super) fn complete_call_discovery(&mut self, action: &UiAction, value: &Value) {
        let Some(flight) = self.observed_calls.flight.as_mut().filter(|flight|
            flight.channel == action.id && Some(flight.token) == action.extra.parse().ok()) else { return; };
        let seen = flight.event_seen;
        if seen || value["written"] != true {
            if !seen && value["notSent"]==true {
                if let Some(record)=self.observed_calls.records.get_mut(&action.id) {
                    record.queries.retain(|token|Some(*token)!=action.extra.parse().ok());
                    if record.queries.is_empty() {record.query_floor=None;}
                }
            }
            self.observed_calls.flight = None;
            self.call_discovery_status(&action.id, false, !seen, if seen { "" } else { value["error"].as_str().unwrap_or("Consulta sin confirmar.") });
        } else {
            flight.written = true;
            flight.deadline = Instant::now() + RESPONSE_WINDOW;
        }
    }
    /// Gateway events carry no query nonce. Retain service message identity so
    /// a delayed query response cannot resurrect a deleted call instance.
    pub(super) fn admit_call_event(&mut self, kind: &str, value: &Value) -> bool {
        let channel = s(value, "channel_id");
        if !parsing::valid_id(&channel) { return false; }
        if !self.ensure_call_record(&channel) {return false;}
        let record = self.observed_calls.records.entry(channel.clone()).or_default();
        let message = s(value, "message_id");
        if kind=="CALL_CREATE"&&!message.is_empty()&&!record.message.is_empty()&&newer_id(&message,&record.message)&&record.queries.is_empty() {
            record.ambiguous_floor=None;
        }
        // CALL_CONNECT has no nonce. A CREATE answering the query cannot
        // overwrite later deltas of that same service call. A genuinely new
        // message identity still starts a new instance.
        let floor=record.query_floor.into_iter().chain(record.ambiguous_floor).min();
        let superseded = kind == "CALL_CREATE" && floor.is_some_and(|revision|revision < record.revision)
            && (message.is_empty() || message == record.message || !record.message.is_empty() && !newer_id(&message,&record.message));
        if superseded {
            let uncertain=record.ambiguous_floor.is_some()&&floor.is_some_and(|floor|record.live_revision<=floor);
            let ambiguous=record.ambiguous_floor.is_some();
            let mut stale_changed=false;
            if uncertain {for call in self.state.incoming_calls.iter_mut().filter(|call|call.channel_id==channel&&!call.observation_stale) {
                call.observation_stale=true;stale_changed=true;
            }}
            self.finish_call_query_event(&channel,false);
            if stale_changed {self.publish();self.flush_snapshot(true);}
            if ambiguous {
                let pending=self.observed_calls.flight.as_ref().is_some_and(|flight|flight.channel==channel);
                self.call_discovery_status(&channel,pending,true,"La respuesta no identifica su consulta. Se conservan los eventos más recientes hasta otra instancia o una sesión Gateway nueva.");
            } else if self.state.incoming_calls.iter().any(|call|call.channel_id==channel&&(!call.roster_complete||call.observation_stale)) {
                self.call_discovery_status(&channel,false,true,"Llegó información más reciente. Actualiza la llamada para completar sus participantes.");
            }
            return false;
        }
        if kind != "CALL_DELETE" && record.deleted {
            if kind != "CALL_CREATE" || !parsing::valid_id(&message)
                || !record.message.is_empty() && !newer_id(&message, &record.message) { return false; }
            record.deleted = false;
            record.departed_locally = false;
        }
        if !message.is_empty() && !record.message.is_empty() && message != record.message
            && !newer_id(&message, &record.message) { return false; }
        if !message.is_empty() {
            if message != record.message { record.departed_locally = false; }
            record.message = message;
        }
        record.revision = record.revision.wrapping_add(1);
        if kind!="CALL_CREATE" {record.live_revision=record.revision;}
        if kind == "CALL_DELETE" { record.deleted = true; }
        if matches!(kind,"CALL_CREATE"|"CALL_DELETE") {self.finish_call_query_event(&channel,kind=="CALL_DELETE");}
        true
    }
    fn finish_call_query_event(&mut self, channel:&str, ended:bool) {
        let token=self.observed_calls.records.get_mut(channel).and_then(|record| {
            let token=record.queries.pop_front();
            if ended {record.queries.clear();}
            if record.queries.is_empty() {record.query_floor=None;}
            token
        });
        if let Some(flight) = self.observed_calls.flight.as_mut().filter(|flight|flight.channel == channel&&(ended||Some(flight.token)==token)) {
            flight.event_seen = true;
            if flight.written { self.observed_calls.flight = None; }
            self.call_discovery_status(channel, false, false, "");
        }
    }
    pub(super) fn observed_call_roster_delta(&mut self, channel:&str) {
        if let Some(record)=self.observed_calls.records.get_mut(channel) {
            record.revision=record.revision.wrapping_add(1);record.live_revision=record.revision;
        }
    }
    pub(super) fn local_call_departure(&mut self, channel: &str) {
        if channel.is_empty() { return; }
        if let Some(record) = self.observed_calls.records.get_mut(channel) { record.departed_locally = true; }
        let me = self.me();
        for call in self.state.incoming_calls.iter_mut().filter(|call|call.channel_id == channel) {
            call.participants.retain(|person|person.id != me);
            call.call_participants.retain(|person|person.user.id != me);
            call.ringing = false;
        }
    }
    pub(super) fn local_call_join(&mut self, channel: &str) {
        if let Some(record) = self.observed_calls.records.get_mut(channel) { record.departed_locally = false; }
    }
    pub(super) fn locally_left_call(&self, channel: &str) -> bool {
        self.observed_calls.records.get(channel).is_some_and(|record|record.departed_locally)
    }
    pub(super) fn current_navigation_action(&self, action: &UiAction) -> bool {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        let Some(context) = extra.get("navigationContext") else { return true; };
        s(context,"account") == self.me() && context["generation"].as_u64() == Some(self.generation)
            && s(context,"guild") == self.state.selected_guild_id && s(context,"view") == self.state.view
            && action.channel_id == self.state.selected_channel_id
    }
}
