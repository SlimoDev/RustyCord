//! Bounded local catalog; no process memory, injection, remote catalog or paths persisted.
use super::*;
mod detection;
mod rpc;
#[cfg(not(test))]
mod windows;
use detection::ScanResult;
use notdiscord_core::game_presence::GamePresence;
const SCAN_PERIOD: Duration = Duration::from_secs(5);
const AUTOMATIC_PRESENCE_PERIOD:Duration=Duration::from_secs(15);
#[cfg(test)]
fn identify(executable: &str) -> Option<&'static str> {
    match detection::identify(executable){Some(detection::Program::Game(name,_))=>Some(name),_=>None}
}
#[cfg(not(test))]
fn scan_games(previous:Option<String>) -> Result<ScanResult, String> {
    windows::scan(previous.as_deref())
}
#[cfg(test)]
fn scan_games(_previous:Option<String>) -> Result<ScanResult, String> {
    Ok(ScanResult::default())
}
#[cfg(feature="offline-probes")]
pub(super) fn scan_once_for_probe()->Result<Value,String>{
    let result=scan_games(None)?;Ok(json!({"activity":result.activity,"rpcs3_open":result.rpcs3_open}))
}
pub(super) struct Detector {
    enabled: bool,
    next_scan: Instant,
    pending: Option<tokio::task::JoinHandle<Result<ScanResult, String>>>,
    detected: Option<String>,
    rpcs3_open: bool,
    rpc: rpc::Receiver,
    manual: Option<String>,
    last_sent: Option<(String, Option<GamePresence>)>,
    last_automatic_attempt:Option<Instant>,
    manual_dirty:bool,
    privacy_revoked:bool,
    last_privacy_attempt:Option<Instant>,
    scan_error: Option<String>,
}
impl Detector {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            next_scan: Instant::now(),
            pending: None,
            detected: None,
            rpcs3_open: false,
            rpc:rpc::Receiver::new(),
            manual: None,
            last_sent: None,
            last_automatic_attempt:None,
            manual_dirty:false,
            privacy_revoked:false,
            last_privacy_attempt:None,
            scan_error: None,
        }
    }
    pub(super) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.suspend_scan();
            self.rpc.close();
        } else {
            self.next_scan = Instant::now();
        }
    }
    fn suspend_scan(&mut self) {
        if let Some(task) = self.pending.take() {
            task.abort();
        }
        self.detected = None;
        self.rpcs3_open = false;
        self.scan_error = None;
        self.next_scan = Instant::now();
    }
    pub(super) fn reset(&mut self) {
        self.suspend_scan();
        self.rpc.reset();
        self.manual = None;
        self.last_sent = None;
        self.last_automatic_attempt=None;
        self.manual_dirty=false;
        self.privacy_revoked=false;self.last_privacy_attempt=None;
    }
    pub(super) fn suspend_rpc(&mut self){self.rpc.close();}
    pub(super) async fn shutdown_rpc(&mut self){self.rpc.shutdown().await;}
    #[allow(dead_code)]
    pub(super) fn record_presence(&mut self, status: String, activity: Option<String>) {
        self.record_rich_presence(status,activity.and_then(|name|GamePresence::from_name(&name).ok()));
    }
    pub(super) fn record_rich_presence(&mut self,status:String,activity:Option<GamePresence>){
        self.last_sent=Some((status,activity));self.last_automatic_attempt=Some(Instant::now());
    }
    fn rich_activity(&self,visible:bool)->Option<GamePresence>{
        if !visible{return None;}
        if let Some(manual)=&self.manual{return GamePresence::from_name(manual).ok();}
        if !self.enabled{return None;}
        let process=self.detected.as_ref().and_then(|name|GamePresence::from_name(name).ok());
        if let Some(rpc)=&self.rpc.activity {
            // RPCS3's verified default RPC says Idle while its launcher is open.
            // An actual recognized game has precedence over that idle emulator.
            if rpc::idle_emulator(rpc)&&process.as_ref().is_some_and(|process|process.name!="RPCS3"){return process;}
            return Some(rpc.clone());
        }
        process
    }
    #[cfg(test)]
    fn activity(&self, visible: bool) -> Option<String> {
        self.rich_activity(visible).map(|activity|activity.summary())
    }
    fn set_manual(&mut self, label: String) {
        self.manual = (!label.is_empty()).then_some(label);
        self.manual_dirty=true;
        self.next_scan = Instant::now();
    }
    fn may_send(&self,desired:&(String,Option<GamePresence>),now:Instant)->bool{
        if self.last_sent.as_ref()==Some(desired){return false;}
        let automatic=self.manual.is_none()&&!self.manual_dirty;
        let status_changed=self.last_sent.as_ref().is_some_and(|last|last.0!=desired.0);
        !automatic||desired.1.is_none()||status_changed||self.last_automatic_attempt.is_none_or(|last|now.saturating_duration_since(last)>=AUTOMATIC_PRESENCE_PERIOD)
    }
}
fn visible(state: &AppSnapshot) -> bool {
    state.signed_in
        && !state.demo
        && state.game_activity_configured
        && state.game_activity_enabled
        && matches!(state.self_presence.as_str(), "online" | "idle" | "dnd")
}
fn rpc_allowed(state:&AppSnapshot,enabled:bool,ready:bool)->bool {
    enabled&&ready&&visible(state)&&state.network_safety==SafetyState::Allowed
}
impl Controller {
    pub(super) fn game_presence_for_status(&self,status:&str)->Option<GamePresence>{
        let visible=self.state.signed_in&&!self.state.demo&&self.state.game_activity_configured&&self.state.game_activity_enabled
            &&matches!(status,"online"|"idle"|"dnd")&&self.safety_enforced==SafetyState::Allowed;
        if !self.gateway.as_ref().is_some_and(|gateway|gateway.is_ready())&&self.game_detector.manual.is_none(){return None;}
        self.game_detector.rich_activity(visible)
    }
    #[allow(dead_code)]
    pub(super) fn game_activity_for_status(&self, status: &str) -> Option<String> {
        self.game_presence_for_status(status).map(|activity|activity.summary())
    }
    pub(super) async fn set_manual_game_activity(&mut self, label: String) {
        self.game_detector.set_manual(label);
        self.tick_game_activity().await;
        self.publish();
    }
    pub(super) async fn tick_game_activity(&mut self) {
        let ready = self
            .gateway
            .as_ref()
            .is_some_and(|gateway| gateway.is_ready())
            && self.safety_enforced == SafetyState::Allowed;
        let publishing = visible(&self.state) && ready;
        let identity=self.state.current_user.as_ref().filter(|user|parsing::valid_id(&user.id)).map(|user|{
            let username=if user.username.is_empty(){"Usuario"}else{&user.username};
            (crate::game_rpc::Identity{id:user.id.clone(),username:username.chars().filter(|c|!c.is_control()).take(80).collect()},self.generation)
        });
        self.game_detector.rpc.poll(rpc_allowed(&self.state,self.game_detector.enabled,ready),identity).await;
        let scanning =
            publishing && self.game_detector.enabled && self.game_detector.manual.is_none();
        if !scanning {
            self.game_detector.suspend_scan();
        } else {
            if self
                .game_detector
                .pending
                .as_ref()
                .is_some_and(|task| task.is_finished())
            {
                if let Some(task) = self.game_detector.pending.take() {
                    self.game_detector.detected = match task.await {
                        Ok(Ok(game)) => {
                            self.game_detector.scan_error = None;
                            self.game_detector.rpcs3_open=game.rpcs3_open;
                            game.activity
                        }
                        Ok(Err(error)) => {
                            self.game_detector.rpcs3_open=false;
                            self.game_detector.scan_error = Some(error);
                            None
                        }
                        Err(_) => {
                            self.game_detector.rpcs3_open=false;
                            self.game_detector.scan_error =
                                Some("No se pudo completar la detección local.".into());
                            None
                        }
                    };
                }
            }
            if self.game_detector.pending.is_none()
                && Instant::now() >= self.game_detector.next_scan
            {
                self.game_detector.next_scan = Instant::now() + SCAN_PERIOD;
                let previous=self.game_detector.detected.clone();
                self.game_detector.pending = Some(tokio::task::spawn_blocking(move||scan_games(previous)));
            }
        }
        let activity = self.game_detector.rich_activity(publishing);
        let note = if !self.state.signed_in {
            "Inicia sesión para publicar actividad.".to_owned()
        } else if !self.state.game_activity_configured {
            "Discord aún no ha confirmado la visibilidad de actividad; la detección permanece pausada.".to_owned()
        } else if !self.state.game_activity_enabled {
            "La actividad de juegos está oculta en Discord.".to_owned()
        } else if !matches!(self.state.self_presence.as_str(), "online" | "idle" | "dnd") {
            "La actividad está pausada por el estado Invisible o sin conexión.".to_owned()
        } else if !ready {
            "Esperando una conexión Gateway preparada para publicar actividad.".to_owned()
        } else if self.game_detector.manual.is_some() {
            "La actividad personalizada tiene prioridad sobre la detección automática.".to_owned()
        } else if !self.game_detector.enabled {
            "La detección de juegos está desactivada en este equipo.".to_owned()
        } else if self.game_detector.rpc.activity.as_ref().is_some_and(|rpc|activity.as_ref()==Some(rpc)) {
            if activity.as_ref().is_some_and(rpc::idle_emulator){"RPCS3 está abierto; el emulador informa que está inactivo.".into()}
            else{format!("Actividad recibida de {}.",activity.as_ref().unwrap().name)}
        } else if let Some(error) = &self.game_detector.scan_error {
            error.clone()
        } else if self.game_detector.rpcs3_open&&self.game_detector.detected.as_deref()==Some("RPCS3") {
            "RPCS3 está abierto. No se ha confirmado qué juego está ejecutando.".to_owned()
        } else if let Some(game) = &self.game_detector.detected {
            format!("Detectado: {game}.")
        } else if let Some(error)=&self.game_detector.rpc.error {
            error.clone()
        } else {
            "Buscando juegos compatibles cada 5 segundos; incluye RPCS3 cuando se reconoce un juego en ejecución.".to_owned()
        };
        let note_changed = self.state.game_detection_note != note;
        self.state.game_detection_note = note;
        let label = activity.as_ref().map(GamePresence::summary).unwrap_or_default();
        if self.state.custom_activity_label != label
            || self.state.game_activity_running != activity.is_some()
            || note_changed
        {
            self.state.custom_activity_label = label;
            self.state.game_activity_running = activity.is_some();
            self.publish();
        }
        // Revoke queued/cached rich data before a privacy change can race reconnect.
        // This local clear is also accepted while the Gateway is reconnecting.
        let permission=visible(&self.state)&&(self.game_detector.enabled||self.game_detector.manual.is_some());
        if !permission {
            let now=Instant::now();
            if !self.game_detector.privacy_revoked&&self.game_detector.last_privacy_attempt.is_none_or(|last|now.saturating_duration_since(last)>=Duration::from_secs(1)) {
            if let Some(gateway)=&self.gateway {
                let status=if notdiscord_core::settings_proto::valid_presence(&self.state.self_presence){Some(self.state.self_presence.clone())}
                    else{self.game_detector.last_sent.as_ref().map(|(status,_)|status.clone()).filter(|status|notdiscord_core::settings_proto::valid_presence(status))};
                if let Some(status)=status {self.game_detector.last_privacy_attempt=Some(now);if gateway.enqueue_clear_game_presence(&status).is_ok(){self.game_detector.privacy_revoked=true;self.game_detector.last_sent=Some((status,None));}}
            }}
            return;
        }
        self.game_detector.privacy_revoked=false;self.game_detector.last_privacy_attempt=None;
        if !ready || !notdiscord_core::settings_proto::valid_presence(&self.state.self_presence) {
            return;
        }
        // A user status write owns the Gateway status until its canonical result arrives.
        // In particular, do not re-enable a rich epoch while an Invisible change is in flight.
        if self.presence_change_pending(){return;}
        let desired = (self.state.self_presence.clone(), activity);
        if self.game_detector.last_sent.as_ref() == Some(&desired) {
            return;
        }
        // Identify already publishes this initial status without an activity.
        if self.game_detector.last_sent.is_none() && desired.1.is_none() {
            self.game_detector.last_sent = Some(desired);
            return;
        }
        let now=Instant::now();
        if !self.game_detector.may_send(&desired,now){return;}
        if let Some(gateway) = &self.gateway {
            if desired.1.is_some()&&gateway.allow_game_presence().is_err(){return;}
            self.game_detector.last_automatic_attempt=Some(now);
            if gateway
                .send(GatewayCommand::RichPresence {
                    status: desired.0.clone(),
                    activity: desired.1.clone(),
                })
                .await
                .is_ok()
            {
                self.game_detector.last_sent = Some(desired);
                self.game_detector.manual_dirty=false;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outlast_matches_exact_basename_and_ignores_launchers() {
        assert_eq!(
            identify(
                r"E:\Games\The Outlast Trials\TOTClient\Binaries\Win64\TOTClient-Win64-Shipping.exe"
            ),
            Some("The Outlast Trials")
        );
        assert_eq!(identify("TOTCLIENT.EXE"), Some("The Outlast Trials"));
        assert_eq!(
            identify(r"E:\Games\Overwatch\_retail_\Overwatch.exe"),
            Some("Overwatch")
        );
        assert_eq!(identify("OVERWATCH.EXE"), Some("Overwatch"));
        assert_eq!(identify("Overwatch Launcher.exe"), None);
        assert_eq!(identify("Overwatch.exe.backup"), None);
        for name in [
            "TOTClient-Win64-Shipping.exe.backup",
            "EasyAntiCheat_EOS.exe",
            "steam.exe",
        ] {
            assert_eq!(identify(name), None);
        }
    }
    #[test]
    fn clear_manual_priority_and_privacy_are_deterministic_offline() {
        let mut detector = Detector::new(true);
        detector.detected = Some("The Outlast Trials".into());
        assert_eq!(
            detector.activity(true).as_deref(),
            Some("The Outlast Trials")
        );
        assert!(detector.activity(false).is_none());
        detector.set_manual("Mi actividad".into());
        assert_eq!(detector.activity(true).as_deref(), Some("Mi actividad"));
        detector.set_manual(String::new());
        detector.detected = None;
        assert!(detector.activity(true).is_none());
        detector.detected = Some("The Outlast Trials".into());
        detector.set_enabled(false);
        assert!(detector.activity(true).is_none());
        let mut state = AppSnapshot::welcome();
        state.signed_in = true;
        state.game_activity_configured = true;
        state.game_activity_enabled = true;
        for status in ["invisible", "offline", "unknown", ""] {
            state.self_presence = status.into();
            assert!(!visible(&state));
        }
        for status in ["online", "idle", "dnd"] {
            state.self_presence = status.into();
            assert!(visible(&state));
        }
        state.game_activity_enabled = false;
        assert!(!visible(&state));
        detector.reset();
        assert!(detector.manual.is_none() && detector.last_sent.is_none());
    }
    #[test]
    fn automatic_rich_updates_coalesce_metadata_changes_for_fifteen_seconds_and_clear_immediately(){
        let now=Instant::now();let mut detector=Detector::new(true);
        let first=GamePresence::from_rpc("424004941485572097","RPCS3",&json!({"details":"Game A"})).unwrap();
        let second=GamePresence::from_rpc("424004941485572097","RPCS3",&json!({"details":"Game B"})).unwrap();
        detector.last_sent=Some(("online".into(),Some(first.clone())));detector.last_automatic_attempt=Some(now);
        assert!(!detector.may_send(&("online".into(),Some(first)),now+Duration::from_secs(60)),"Identical complete activity is deduplicated even after the interval");
        assert!(!detector.may_send(&("online".into(),Some(second.clone())),now+Duration::from_secs(14)));
        assert!(detector.may_send(&("online".into(),Some(second.clone())),now+Duration::from_secs(15)));
        assert!(detector.may_send(&("online".into(),None),now));
        assert!(detector.may_send(&("idle".into(),Some(second.clone())),now));
        detector.set_manual("Explicit manual activity".into());assert!(detector.may_send(&("online".into(),Some(second)),now));
    }
    #[test]
    fn manual_rpc_and_process_priority_distinguishes_idle_emulator_from_a_running_game(){
        let mut detector=Detector::new(true);detector.detected=Some("Overwatch".into());
        detector.rpc.activity=Some(GamePresence::from_rpc("424004941485572097","RPCS3",&json!({"details":"Idle"})).unwrap());
        assert_eq!(detector.activity(true).as_deref(),Some("Overwatch"));
        detector.rpc.activity.as_mut().unwrap().details=Some("Synthetic PS3 game".into());assert_eq!(detector.activity(true).as_deref(),Some("RPCS3 · Synthetic PS3 game"));
        detector.set_manual("Explicit choice".into());assert_eq!(detector.activity(true).as_deref(),Some("Explicit choice"));assert!(detector.activity(false).is_none());
        detector.set_manual(String::new());detector.set_enabled(false);assert!(detector.rpc.activity.is_none());assert!(detector.activity(true).is_none());
    }
    #[test]
    fn rpc_start_gate_requires_real_session_confirmed_visibility_ready_gateway_and_local_opt_in(){
        let mut state=AppSnapshot::welcome();state.signed_in=true;state.game_activity_configured=true;state.game_activity_enabled=true;state.self_presence="online".into();
        assert!(rpc_allowed(&state,true,true));assert!(!rpc_allowed(&state,false,true));assert!(!rpc_allowed(&state,true,false));
        for status in ["invisible","offline","unknown",""]{state.self_presence=status.into();assert!(!rpc_allowed(&state,true,true));}
        state.self_presence="online".into();state.game_activity_configured=false;assert!(!rpc_allowed(&state,true,true));state.game_activity_configured=true;
        state.game_activity_enabled=false;assert!(!rpc_allowed(&state,true,true));state.game_activity_enabled=true;
        state.demo=true;assert!(!rpc_allowed(&state,true,true));state.demo=false;state.signed_in=false;assert!(!rpc_allowed(&state,true,true));state.signed_in=true;
        state.network_safety=SafetyState::VerificationRequired;assert!(!rpc_allowed(&state,true,true));
    }
    #[tokio::test]
    async fn real_actor_publishes_rich_metadata_coalesces_changes_and_withdraws_on_privacy_without_an_account_or_pipe(){
        let(mut controller,_)=Controller::test_fixture();controller.state.signed_in=true;controller.state.game_activity_configured=true;controller.state.game_activity_enabled=true;controller.state.self_presence="online".into();
        controller.state.current_user=Some(Person{id:"1".into(),username:"Synthetic local".into(),..Default::default()});
        let(gateway,mut values,worker)=GatewayHandle::offline_recorder(SafetyGate::default());controller.gateway=Some(gateway);
        let(listener,updates)=crate::game_rpc::Listener::fake();controller.game_detector.rpc.install_fake(listener);
        let activity=crate::game_rpc::Activity{client_id:"424004941485572097".into(),pid:123,executable:"rpcs3.exe".into(),name:Some("Ignored untrusted name".into()),details:"Synthetic game A".into(),state:"[BLUS31604]".into(),timestamps:Some(crate::game_rpc::Timestamps{start_ms:Some(1_700_000_000_000),end_ms:None}),assets:None,party:None,instance:None};
        updates.send_modify(|snapshot|{snapshot.activity=Some(activity);snapshot.revision+=1;});
        controller.tick_game_activity().await;
        let first=values.try_recv().unwrap();assert_eq!(first["d"]["activities"][0]["name"],"RPCS3");assert_eq!(first["d"]["activities"][0]["details"],"Synthetic game A");assert_eq!(first["d"]["activities"][0]["timestamps"]["start"],1_700_000_000_000u64);
        updates.send_modify(|snapshot|{snapshot.activity.as_mut().unwrap().details="Synthetic game B".into();snapshot.revision+=1;});
        controller.tick_game_activity().await;assert!(values.try_recv().is_err(),"Automatic details updates wait for the minimum interval");
        controller.game_detector.last_automatic_attempt=Some(Instant::now()-AUTOMATIC_PRESENCE_PERIOD);
        controller.tick_game_activity().await;assert_eq!(values.try_recv().unwrap()["d"]["activities"][0]["details"],"Synthetic game B");
        controller.state.game_activity_enabled=false;controller.tick_game_activity().await;
        let cleared=tokio::time::timeout(Duration::from_secs(1),values.recv()).await.unwrap().unwrap();assert_eq!(cleared["d"]["activities"],json!([]));assert!(!updates.borrow().listening);assert!(updates.borrow().activity.is_none());assert!(controller.state.custom_activity_label.is_empty());
        controller.gateway.take().unwrap().stop();worker.await.unwrap();assert!(controller.api.is_none()&&controller.media.is_none());
    }
}
