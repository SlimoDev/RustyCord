//! Native state is retained locally; only the application coordinator admits
//! a replacement transport. No retries or Identify fallback live in the host.
use super::*;
use voice_socket::RecoveryClass;

impl Host {
    pub(super) fn wait_for_socket_close(&mut self, request: Value, result: Value) -> Result<Option<Value>> {
        let result = if self.close_failures.is_empty() { Ok(result) } else {
            Err(MediaError::new("NATIVE_RELEASE_UNKNOWN", "El motor no confirmó la liberación de los recursos multimedia."))
        };
        if self.close_barriers.is_empty() { return result.map(Some); }
        if self.close_waiters.len() >= 32 { return Err(MediaError::new("CLOSE_BARRIER_BUSY", "La liberación anterior aún no terminó.")); }
        self.close_waiters.push((request, result, self.close_barriers.clone()));
        Ok(None)
    }
    pub(super) fn socket_joined(&mut self, id: u64) {
        self.close_barriers.remove(&id);
        let waiters = std::mem::take(&mut self.close_waiters);
        for (request, result, mut waiting) in waiters {
            waiting.remove(&id);
            if waiting.is_empty() {
                if self.pending.contains_key(&request.to_string()) { self.reply(request, result); }
            } else { self.close_waiters.push((request, result, waiting)); }
        }
        if self.close_barriers.is_empty() {
            for s in self.sessions.values().filter(|s| !s.closed) {
                if self.recoveries.borrow().get(&s.id).is_some_and(|recovery| recovery.interrupted && recovery.idle())
                    && !self.sockets.contains_key(&s.id) { self.emit_interrupted(s); }
            }
        }
    }
    fn socket_current(&self, id: u64, generation: u64) -> bool {
        self.recoveries.borrow().get(&id).is_some_and(|recovery| recovery.protocol.accepts(generation))
            && self.sockets.get(&id).is_some_and(|socket| socket.generation() == generation)
    }
    pub(super) fn socket_open(&mut self, id: u64, generation: u64) {
        if !self.socket_current(id, generation) { return; }
        let result = (|| {
            let s = self.sessions.get(&id).ok_or_else(MediaError::internal)?;
            self.stage(s, "websocket-open");
            let resuming = self.recoveries.borrow().get(&id).is_some_and(|recovery| recovery.protocol.is_resuming());
            if resuming {
                let packet = self.recoveries.borrow().get(&id).ok_or_else(MediaError::internal)?.protocol.resume_payload(&s.credentials)?;
                self.sockets.get(&id).ok_or_else(MediaError::internal)?.send(Message::Text(packet.to_string().into()))?;
                self.recoveries.borrow_mut().get_mut(&id).ok_or_else(MediaError::internal)?.protocol.resume_sent()?;
            } else {
                let p = &s.credentials;
                self.send(id,0,json!({"server_id":p.server_id,"channel_id":p.channel_id,"user_id":p.user_id,
                    "session_id":p.session_id,"token":p.voice_token,"max_dave_protocol_version":1,
                    "video":true,"streams":[{"type":"video","rid":"100","quality":100,"active":false}]}));
            }
            Ok(())
        })();
        if let Err(error) = result && let Some(mut s) = self.sessions.remove(&id) { self.fail(&mut s, error); }
    }
    pub(super) fn socket_frame(&mut self, id: u64, generation: u64, frame: Frame) {
        if !self.socket_current(id, generation) { return; }
        let Some(mut s) = self.sessions.remove(&id) else { return; };
        let ticket = self.recoveries.borrow_mut().get_mut(&id).ok_or_else(MediaError::internal)
            .and_then(|recovery| recovery.begin(id, generation, &frame));
        let result = ticket.and_then(|ticket| {
            self.frame_context = Some(ticket);
            let result = self.frame(&mut s, frame);
            self.frame_context = None;
            result?;
            let resumed = self.recoveries.borrow_mut().get_mut(&id).ok_or_else(MediaError::internal)?.complete(ticket)?;
            self.refresh_applied(&mut s, resumed)
        });
        if let Err(error) = result { self.fail(&mut s, error); }
        if !s.closed { self.sessions.insert(id, s); }
    }
    pub(super) fn refresh_applied(&mut self, s: &mut Session, resumed: bool) -> Result<()> {
        if let Some(recovery) = self.recoveries.borrow().get(&s.id) { s.seq = recovery.protocol.seq_ack(); }
        if resumed {
            if let Some(recovery) = self.recoveries.borrow_mut().get_mut(&s.id) {
                recovery.interrupted = false; recovery.reason = None; recovery.close_code = None;
            }
            self.check_connected(s)?;
        } else if !self.sockets.contains_key(&s.id) && self.recoveries.borrow().get(&s.id)
            .is_some_and(|recovery| recovery.interrupted && recovery.idle() && !recovery.protocol.is_resuming()) {
            self.emit_interrupted(s);
        }
        Ok(())
    }
    fn emit_interrupted(&self, s: &Session) {
        let recoveries = self.recoveries.borrow();
        let Some(recovery) = recoveries.get(&s.id) else { return; };
        let mut event = s.state_event("interrupted", recovery.reason.clone());
        event["resumeEligible"] = json!(recovery.idle() && !recovery.quiescing && !recovery.protocol.is_resuming() && !self.sockets.contains_key(&s.id)
            && self.close_barriers.is_empty() && self.close_failures.is_empty() && s.session_description && s.dave_ready
            && self.connections.contains_key(&s.id) && !s.closed);
        event["retainedNative"] = json!(true);
        event["transportGeneration"] = json!(recovery.protocol.generation());
        event["closeCode"] = json!(recovery.close_code);
        self.emit(event);
    }
    fn suspend_session(&mut self, s: &mut Session, reason: MediaError, close_code: Option<u16>) -> Result<()> {
        if let Some(socket) = self.sockets.remove(&s.id) { self.stopped_sockets.insert(s.id, socket.stop()); }
        if let Some(recovery) = self.recoveries.borrow_mut().get_mut(&s.id) {
            recovery.protocol.suspend(); recovery.interrupted = true; recovery.quiescing = true;
            recovery.reason = Some(reason); recovery.close_code = close_code;
        }
        s.connected = false; s.ptt_active = false;
        s.connect_deadline = None; s.udp_deadline = None; s.dave_deadline = None;
        s.heartbeat_pending = None; s.heartbeat_interval = None; s.next_heartbeat = None;
        self.transmit_gate(s, false)?;
        self.conn_call(s.id, "setSelfMute", vec![json!(true)])?;
        s.reset_input_activity();
        if let Some(record) = s.input_capture.cancel(stats::voice::EndReason::ConnectionChanged) { self.emit_voice_record(record); }
        if let Some(event) = s.speaking_event(&s.credentials.user_id.clone(), false) { self.emit(event); }
        // The actor must retain the user's selections before ordinary local
        // camera/source cleanup can look like an explicit stop.
        self.emit_interrupted(s);
        self.video_suspend(s.id);
        self.stop_camera(s)?;
        if s.stream() && !s.receive() { s.cancel_share(); self.clear_share(s)?; }
        if !s.stream() && let Some(share_id) = self.share {
            if let Some(mut share) = self.sessions.remove(&share_id) {
                if let Some(recovery) = self.recoveries.borrow_mut().get_mut(&share_id) { recovery.interrupted = true; }
                // The main voice episode owns rearming the explicit selection;
                // there is no second GoLive Resume or independent retry here.
                self.close(&mut share, false);
            }
        }
        if let Some(recovery) = self.recoveries.borrow_mut().get_mut(&s.id) { recovery.quiescing = false; }
        // Publication closure may now own an asynchronous socket barrier.
        self.emit_interrupted(s);
        Ok(())
    }
    pub(super) fn socket_error(&mut self, id: u64, generation: u64, failure: voice_socket::Failure) {
        if !self.socket_current(id, generation) { return; }
        let Some(mut s) = self.sessions.remove(&id) else { return; };
        let class = failure.disposition();
        let close = failure.close_code.map(|code| code.value());
        self.emit(json!({"event":"transportError","mediaSession":id,"phase":failure.phase,
            "code":failure.error.code,"closeCode":close,"transportGeneration":generation}));
        let resumable = class == RecoveryClass::ResumeCandidate && s.session_description
            && self.connections.contains_key(&id);
        if resumable {
            if let Err(error) = self.suspend_session(&mut s, failure.error, close) { self.fail(&mut s, error); }
            if !s.closed { self.sessions.insert(id, s); }
        } else {
            let mut event = s.state_event("error", Some(failure.error));
            event["closeCode"] = json!(close);
            event["recoveryClass"] = json!(match class { RecoveryClass::ResumeCandidate => "fresh-candidate",
                RecoveryClass::SessionInvalid => "session-invalid", RecoveryClass::Terminal => "terminal" });
            self.emit(event);
            self.close(&mut s, false);
        }
    }
    pub(super) fn recovery_command(&mut self, name: &str, p: &Value) -> Result<Value> {
        let id = p["mediaSession"].as_u64().filter(|id| *id > 0).ok_or_else(||
            MediaError::new("VOICE_CONTEXT_REQUIRED", "La recuperación necesita el intento de voz vigente."))?;
        let mut s = self.sessions.remove(&id).filter(|s| !s.closed).ok_or_else(||
            MediaError::new("VOICE_CONTEXT_CHANGED", "El intento de voz ya terminó."))?;
        let result = (|| {
            if !self.connections.contains_key(&id) || !s.session_description {
                return Err(MediaError::new("VOICE_RESUME_UNAVAILABLE", "No hay una conexión nativa conservada que se pueda reanudar."));
            }
            if name == "suspendVoice" {
                let interrupted = self.recoveries.borrow().get(&id).is_some_and(|recovery| recovery.interrupted);
                if !interrupted { self.suspend_session(&mut s,
                    MediaError::new("VOICE_SUSPENDED", "La llamada espera la recuperación coordinada de conexión."), None)?; }
                return Ok(json!({"suspended":true,"mediaSession":id}));
            }
            let generation = {
                let mut recoveries = self.recoveries.borrow_mut();
                let recovery = recoveries.get_mut(&id).ok_or_else(MediaError::internal)?;
                if !recovery.interrupted || recovery.quiescing || !recovery.idle() || !s.dave_ready || self.sockets.contains_key(&id)
                    || !self.close_barriers.is_empty() || !self.close_failures.is_empty() {
                    return Err(MediaError::new("VOICE_RESUME_BUSY", "La conexión conservada aún no está lista para reanudarse."));
                }
                let generation = recovery.protocol.generation().checked_add(1).ok_or_else(MediaError::internal)?;
                recovery.protocol.begin_resume(generation)?;
                generation
            };
            let previous = self.stopped_sockets.remove(&id);
            let socket = voice_socket::spawn_after(self.runtime.handle(), s.credentials.endpoint.clone(), id,
                generation, self.sender.clone(), previous);
            self.sockets.insert(id, socket);
            s.connect_deadline = Some(Instant::now() + Duration::from_secs(30));
            self.emit(s.state_event("resuming", None));
            Ok(json!({"admitted":true,"mediaSession":id,"generation":generation}))
        })();
        if !s.closed { self.sessions.insert(id, s); }
        result
    }
}
