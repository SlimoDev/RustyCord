use super::*;
use notdiscord_core::gateway::RecoveryPermit;

pub(super) struct ViewRecovery {
    key:String, owner:String, revision:u64, attempt:u64, deadline:Instant,
    started:bool, permit:Option<RecoveryPermit>,
}
pub(super) struct ShareClose {
    pub(super) key:String, pub(super) token:u64, pub(super) local_closed:bool,
    pub(super) written:bool, pub(super) acknowledged:bool, pub(super) failed:bool,
}
impl Controller {
    pub(super) fn track_screen_offer(&mut self,key:&str,identity:&str,new_publication:bool) {
        self.voice_lifecycle.sources.announce(key,identity,new_publication);
    }
    pub(super) fn project_screen_availability(&mut self) {
        let offers=self.call_people.iter().filter(|(_,p)|p.screen_sharing).map(|(owner,_)|self.stream_key_for(owner)).collect::<Vec<_>>();
        for key in offers {
            let identity=self.voice_lifecycle.stream_identities.get(&key).map(|id|id.server.clone()).unwrap_or_default();
            self.track_screen_offer(&key,&identity,false);
        }
        let channel=self.voice_channel.clone();let guild=self.voice_guild.clone();let me=self.me();
        let preparing=self.state.screen_preparing&&self.voice_lifecycle.selected_share.is_some();
        for (owner,person) in &mut self.call_people {
            let key=if guild.is_empty(){format!("call:{channel}:{owner}")}else{format!("guild:{guild}:{channel}:{owner}")};
            person.screen_view_unavailable=self.voice_lifecycle.sources.blocked(&key)&&!(owner==&me&&preparing);
        }
    }
    pub(super) async fn end_screen_publication(&mut self,key:&str,participant_left:bool)->Result<(),String> {
        let Some((channel,owner))=super::super::incoming::incoming_stream_identity(key) else{return Ok(());};
        if channel!=self.voice_channel {return Ok(());}
        if self.voice_lifecycle.sources.revision(key).is_none()
            && self.call_people.get(&owner).is_some_and(|p|p.screen_sharing) {self.track_screen_offer(key,"",false);}
        let ended=self.voice_lifecycle.sources.end(key);
        self.cancel_view_recovery_for(key);
        let view=format!("{channel}:{owner}");
        self.voice_lifecycle.pending_views.retain(|k,p|k!=&view||p.kind!="screen");
        self.voice_lifecycle.view_intents.retain(|k,p|k!=&view||p.kind!="screen");
        if let Some(p)=self.call_people.get_mut(&owner){p.screen_sharing=false;p.screen_announcement_known=true;p.screen_view_unavailable=true;}
        for call in &mut self.state.incoming_calls {
            if call.channel_id==channel {for p in &mut call.call_participants {if p.user.id==owner {p.screen_sharing=false;p.screen_announcement_known=true;p.screen_view_unavailable=true;}}}
        }
        // Publisher withdrawal already ended the subscription. No redundant
        // STREAM_DELETE or invented remote confirmation is needed.
        if let Some(watch)=self.voice_lifecycle.watches.get_mut(key){watch["requested"]=json!(false);}
        self.stop_watch(key).await?;
        self.voice_lifecycle.stream_viewers.remove(key);self.publish_stream_viewers();
        if let Some(revision)=ended && !participant_left && self.state.voice_connected {
            self.data("sound",json!({"event":"stream_ended","account_id":self.me(),"session_generation":self.state.notification_generation,"voice_attempt":self.voice_attempt,"publication":revision}));
        }
        self.refresh_call();self.flush_snapshot(true);Ok(())
    }
    fn cancel_view_recovery_for(&mut self,key:&str) {
        if self.voice_lifecycle.view_recovery.as_ref().is_some_and(|r|r.key==key)
            && let Some(mut recovery)=self.voice_lifecycle.view_recovery.take()
            && let Some(permit)=recovery.permit.take(){self.voice_lifecycle.retiring_view=Some((key.into(),permit));}
        self.sync_transport_gate();
    }
    pub(super) fn cancel_screen_recovery(&mut self) {
        if let Some(key)=self.voice_lifecycle.view_recovery.as_ref().map(|r|r.key.clone()){self.cancel_view_recovery_for(&key);}
    }
    pub(super) async fn block_screen_view(&mut self,key:&str,message:&str)->Result<(),String> {
        self.cancel_view_recovery_for(key);
        if let Some(revision)=self.voice_lifecycle.sources.revision(key){self.voice_lifecycle.sources.unavailable(key,revision);}
        if let Some((channel,owner))=super::super::incoming::incoming_stream_identity(key){
            let view=format!("{channel}:{owner}");
            self.voice_lifecycle.view_intents.retain(|k,p|k!=&view||p.kind!="screen");
            self.voice_lifecycle.pending_views.retain(|k,p|k!=&view||p.kind!="screen");
        }
        self.stop_watch(key).await?;
        self.status(&parsing::limited(message,256));self.refresh_call();Ok(())
    }
    pub(super) async fn recover_screen_view(&mut self,key:&str,message:&str)->Result<(),String> {
        if self.voice_lifecycle.watches.get(key).and_then(|w|w["publication"].as_u64()).is_some_and(|revision|self.voice_lifecycle.sources.revision(key)!=Some(revision)){
            self.cancel_view_recovery_for(key);self.stop_watch(key).await?;return Ok(());
        }
        if self.voice_lifecycle.view_recovery.as_ref().is_some_and(|r|r.key==key&&!r.started){return Ok(());}
        let Some((channel,owner))=super::super::incoming::incoming_stream_identity(key) else {return self.block_screen_view(key,message).await;};
        let selected=self.voice_lifecycle.view_intents.get(&format!("{channel}:{owner}"))
            .is_some_and(|v|v.kind=="screen"&&v.attempt==self.voice_attempt);
        if self.voice_lifecycle.sources.revision(key).is_none()&&self.call_people.get(&owner).is_some_and(|p|p.screen_sharing){self.track_screen_offer(key,"",false);}
        if !selected || channel!=self.voice_channel || self.voice_lifecycle.view_recovery.is_some(){return self.block_screen_view(key,message).await;}
        let Some(revision)=self.voice_lifecycle.sources.recover(key) else {return self.block_screen_view(key,message).await;};
        self.stop_watch(key).await?;
        self.voice_lifecycle.view_recovery=Some(ViewRecovery {key:key.into(),owner,revision,attempt:self.voice_attempt,
            deadline:Instant::now()+Duration::from_secs(45),started:false,permit:None});
        self.sync_transport_gate();
        self.status("Se interrumpió la recepción de pantalla. Esperando su cierre para recuperar la selección vigente.");
        self.refresh_call();Ok(())
    }
    pub(in crate::controller) async fn tick_screen_recovery(&mut self)->Result<(),String> {
        let stale=self.voice_lifecycle.watches.iter().filter(|(key,watch)|watch["publication"].as_u64().is_some_and(|revision|self.voice_lifecycle.sources.revision(key)!=Some(revision)))
            .map(|(key,_)|key.clone()).collect::<Vec<_>>();
        for key in stale {
            self.cancel_view_recovery_for(&key);
            if let Some((channel,owner))=super::super::incoming::incoming_stream_identity(&key){
                self.voice_lifecycle.view_intents.retain(|_,view|view.channel!=channel||view.owner!=owner||view.kind!="screen");
                self.voice_lifecycle.pending_views.retain(|_,view|view.channel!=channel||view.owner!=owner||view.kind!="screen");
            }
            self.stop_watch(&key).await?;
        }
        if self.voice_lifecycle.share_closing.is_none()&&self.voice_lifecycle.pending_share.is_some()&&self.state.voice_encryption_ready {
            self.resume_explicit_capture().await?;
        }
        let Some(r)=self.voice_lifecycle.view_recovery.as_ref() else {return Ok(());};
        let key=r.key.clone();
        let stale=r.attempt!=self.voice_attempt || self.voice_lifecycle.sources.revision(&key)!=Some(r.revision)
            || self.voice_lifecycle.sources.blocked(&key)
            || !r.started&&self.voice_lifecycle.sources.phase(&key)!=Some(stream_sources::Phase::Recovering)
            || !self.viewing_source(&r.owner,"screen");
        if stale {self.cancel_view_recovery_for(&key);self.stop_watch(&key).await?;return Ok(());}
        if Instant::now()>=r.deadline || r.permit.as_ref().is_some_and(|p|!p.is_current()) {
            return self.block_screen_view(&key,"La recepción de pantalla no se pudo recuperar dentro del intento permitido.").await;
        }
        if r.started || self.state.connection_phase.suspended() || !self.state.voice_encryption_ready
            || self.voice_lifecycle.receiver_closing.is_some()
            || self.sends.retry_in_flight()
            || self.voice_lifecycle.watch_stops.get(&key).is_some_and(|s|!s.written||s.pending>0||!s.local_closed||s.write_failed) {return Ok(());}
        let Some(permit)=self.gateway.as_ref().and_then(|g|g.try_view_recovery()) else {return Ok(());};
        let owner=r.owner.clone();let revision=r.revision;
        let r=self.voice_lifecycle.view_recovery.as_mut().unwrap();r.started=true;r.permit=Some(permit);
        self.voice_lifecycle.sources.recovered(&key,revision);
        let mut action=UiAction::id("watch-stream",&owner);action.channel_id=self.voice_channel.clone();
        if let Err(error)=Box::pin(self.media_action(&action)).await {self.block_screen_view(&key,&error).await?;}
        Ok(())
    }
    pub(super) fn screen_recovery_succeeded(&mut self,key:&str) {
        if self.voice_lifecycle.view_recovery.as_ref().is_some_and(|r|r.key==key&&r.started)
            && let Some(mut r)=self.voice_lifecycle.view_recovery.take()
            && let Some(permit)=r.permit.take(){permit.finish(true);}
        self.sync_transport_gate();
    }
    pub(super) fn close_screen_receiver(&mut self,receiver:MediaBridge) {
        self.close_screen_receiver_after(receiver,None);
    }
    pub(super) fn close_screen_receiver_after(&mut self,receiver:MediaBridge,stop:Option<(String,u64)>) {
        receiver.clear_video_frames();
        self.voice_lifecycle.receiver_close_sequence=self.voice_lifecycle.receiver_close_sequence.wrapping_add(1).max(1);
        let token=self.voice_lifecycle.receiver_close_sequence;
        self.voice_lifecycle.receiver_closing=Some(receiver.clone());
        let mut done=UiAction::id("__source-close","receiver");done.text=token.to_string();
        self.launch_owned_close(done,async move {
            if let Some((key,token))=stop {let _=receiver.command("stopWatchStream",json!({"streamKey":key,"token":token})).await;}
            let _=receiver.command("shutdown",json!({})).await;
            receiver.close();let result=receiver.await_closed().await;Ok(json!({"closed":result.is_ok()}))
        });
    }
    pub(in crate::controller) fn complete_source_close(&mut self,action:&UiAction,value:&Value)->Result<(),String> {
        if action.id=="receiver" {
            if action.text.parse::<u64>().ok()==Some(self.voice_lifecycle.receiver_close_sequence) && value["closed"]==true {
                self.voice_lifecycle.receiver_closing=None;
                if let Some((_,permit))=self.voice_lifecycle.retiring_view.take(){permit.finish(false);}
                for stop in self.voice_lifecycle.watch_stops.values_mut(){stop.local_closed=true;}
                self.voice_lifecycle.watch_stops.retain(|_,stop|stop.pending>0||!stop.written||stop.write_failed);
                self.sync_transport_gate();
            }
            return Ok(());
        }
        let Some(close)=self.voice_lifecycle.share_closing.as_mut().filter(|c|Some(c.token)==action.text.parse::<u64>().ok()) else{return Ok(());};
        close.local_closed=value["localClosed"]==true;close.written=value["written"]==true;close.failed=!close.local_closed||!close.written;
        if close.failed {self.status("La pantalla local se retiró; su cierre no quedó confirmado. Se conserva la barrera sin repetir la operación.");}
        self.finish_share_close();Ok(())
    }
    pub(super) fn acknowledge_share_close(&mut self,key:&str)->bool {
        let Some(close)=self.voice_lifecycle.share_closing.as_mut().filter(|c|c.key==key) else{return false;};
        close.acknowledged=true;self.finish_share_close();true
    }
    fn finish_share_close(&mut self) {
        if self.voice_lifecycle.share_closing.as_ref().is_some_and(|c|c.local_closed&&c.written&&c.acknowledged&&!c.failed){self.voice_lifecycle.share_closing=None;}
    }
}
