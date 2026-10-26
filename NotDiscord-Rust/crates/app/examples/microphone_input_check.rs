//! Explicit, local microphone diagnostic. No Controller, account, credentials,
//! API client, voice session, audio file or recorded waveform is constructed.
//! All media commands are constrained by LocalCommand below.
use notdiscord::media::MediaBridge;
use serde_json::{Value, json};
use std::{path::{Path, PathBuf}, time::Duration};
use tokio::{sync::mpsc, time::{Instant, timeout, timeout_at}};

#[derive(Clone, Copy)]
enum LocalCommand { Start, Stop, Shutdown }
impl LocalCommand {
    const fn name(self) -> &'static str {
        match self { Self::Start => "startMicTest", Self::Stop => "stopMicTest", Self::Shutdown => "shutdown" }
    }
}
async fn local_command(bridge: &MediaBridge, command: LocalCommand, parameters: Value) -> Result<Value, String> {
    timeout(Duration::from_secs(1), bridge.command(command.name(), parameters)).await
        .map_err(|_| format!("{} timed out", command.name()))?
}
struct CloseOnDrop(MediaBridge);
impl Drop for CloseOnDrop { fn drop(&mut self) { self.0.close(); } }

struct Phase {
    threshold: f64,
    callbacks: u64,
    nonzero: u64,
    speaking: u64,
    below_threshold: u64,
    at_or_above_threshold: u64,
    gate_mismatches: u64,
    invalid_levels: u64,
    minimum_db: f64,
    maximum_db: f64,
}
impl Phase {
    fn new(threshold: f64) -> Self {
        Self { threshold, callbacks: 0, nonzero: 0, speaking: 0, below_threshold: 0,
            at_or_above_threshold: 0, gate_mismatches: 0, invalid_levels: 0, minimum_db: 0., maximum_db: -100. }
    }
    fn observe(&mut self, event: &Value) -> Result<(), String> {
        if event["monitoring"].as_bool() != Some(false) || event["localOnly"].as_bool() != Some(true) {
            return Err("Microphone callback is not isolated local monitoring".into());
        }
        let level = event["level"].as_f64().ok_or("Missing microphone level")?;
        let db = event["db"].as_f64().ok_or("Missing microphone dB")?;
        let speaking = event["speaking"].as_bool().ok_or("Missing manual gate state")?;
        self.callbacks += 1;
        if !level.is_finite() || !(0.0..=1.0).contains(&level) || !db.is_finite() || !(-100.0..=0.0).contains(&db) {
            self.invalid_levels += 1;
            return Ok(());
        }
        let expected_db = if level > 0. { (20. * level.log10()).clamp(-100., 0.) } else { -100. };
        if (expected_db - db).abs() > 0.01 { self.invalid_levels += 1; }
        self.minimum_db = self.minimum_db.min(db);
        self.maximum_db = self.maximum_db.max(db);
        // A negative native dB floor can normalize to a tiny positive float.
        // Receiving that sentinel is not evidence of a physical input signal.
        self.nonzero += u64::from(level > 0. && db > -99.);
        self.speaking += u64::from(speaking);
        self.below_threshold += u64::from(db < self.threshold);
        self.at_or_above_threshold += u64::from(db >= self.threshold);
        self.gate_mismatches += u64::from(speaking != (level > 0. && db >= self.threshold));
        if let Some(applied) = event.get("thresholdDb").and_then(Value::as_f64) {
            self.gate_mismatches += u64::from((applied - self.threshold).abs() > 0.01);
        }
        Ok(())
    }
    fn summary(&self) -> Value {
        json!({"thresholdDb":self.threshold,"callbacks":self.callbacks,"aboveFloorCallbacks":self.nonzero,
            "speakingCallbacks":self.speaking,"belowThresholdCallbacks":self.below_threshold,
            "atOrAboveThresholdCallbacks":self.at_or_above_threshold,"gateMismatches":self.gate_mismatches,
            "invalidLevels":self.invalid_levels,"minimumDb":(self.callbacks>0).then_some(self.minimum_db),
            "maximumDb":(self.callbacks>0).then_some(self.maximum_db)})
    }
}

async fn sample(bridge: &MediaBridge, events: &mut mpsc::UnboundedReceiver<Value>) -> Result<Vec<Phase>, String> {
    let mut phases = Vec::new();
    for (index, threshold) in [-100., -40., 0.].into_iter().enumerate() {
        let token = index as u64 * 2 + 1;
        let response = local_command(bridge, LocalCommand::Start, json!({"token":token,"monitorOnly":true,"settings":{
            "inputId":"default","outputId":"default","inputVolume":100.,"outputVolume":0.,
            "inputSensitivityDb":threshold,"noiseSuppression":false,"echoCancellation":false,"pushToTalk":false
        }})).await?;
        if response["active"].as_bool()!=Some(true) || response["localOnly"].as_bool()!=Some(true)
            || response["monitorOnly"].as_bool()!=Some(true) {
            return Err("Native host did not confirm silent local monitoring".into());
        }
        let mut phase = Phase::new(threshold);
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            let event = match timeout_at(until, events.recv()).await {
                Err(_) => break,
                Ok(None) => return Err("Microphone event stream closed".into()),
                Ok(Some(event)) => event,
            };
            if event["event"] == "fatal" { return Err("Local media host closed during microphone check".into()); }
            // The start/stop status notifications are not captured samples.
            if event["event"] == "micTest" && event["active"].as_bool() == Some(true)
                && event["token"].as_u64() == Some(token) && event.get("monitoring").is_some() {
                phase.observe(&event)?;
            }
        }
        local_command(bridge, LocalCommand::Stop, json!({"token":token+1})).await?;
        println!("{}", json!({"microphonePhase":phase.summary()}));
        phases.push(phase);
    }
    Ok(phases)
}

fn arguments() -> Result<(PathBuf, PathBuf), String> {
    let mut runtime = None;
    let mut data = None;
    let mut consent = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--real-microphone" => consent = true,
            "--runtime-root" => runtime = Some(PathBuf::from(arguments.next().ok_or("Missing runtime root")?)),
            "--data-dir" => data = Some(PathBuf::from(arguments.next().ok_or("Missing new data directory")?)),
            _ => return Err("Usage: microphone_input_check --real-microphone --runtime-root E:\\isolated-runtime --data-dir E:\\new-isolated-directory".into()),
        }
    }
    if !consent { return Err("No microphone was opened: explicit --real-microphone is required".into()); }
    let runtime = runtime.ok_or("--runtime-root is required")?;
    let data = data.ok_or("--data-dir is required")?;
    if !runtime.is_absolute() || !data.is_absolute() || !on_e_drive(&runtime) || !on_e_drive(&data) {
        return Err("Both paths must be absolute paths on E:".into());
    }
    if data.exists() || !data.parent().is_some_and(Path::is_dir) {
        return Err("The isolated data directory must be new and its parent must already exist".into());
    }
    if !runtime.canonicalize().is_ok_and(|path| on_e_drive(&path))
        || !data.parent().unwrap().canonicalize().is_ok_and(|path| on_e_drive(&path)) {
        return Err("Runtime and isolated data parent must resolve to E:".into());
    }
    if !MediaBridge::is_available(&runtime) { return Err("The specified runtime is incomplete".into()); }
    Ok((runtime, data))
}
fn on_e_drive(path: &Path) -> bool {
    path.components().next().is_some_and(|component| match component {
        std::path::Component::Prefix(prefix) => matches!(prefix.kind(), std::path::Prefix::Disk(drive) | std::path::Prefix::VerbatimDisk(drive) if drive.eq_ignore_ascii_case(&b'E')),
        _ => false,
    })
}

async fn run(runtime: PathBuf, data: PathBuf) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    // Last-resort process watchdog: closing the parent process closes its
    // existing production kill-on-close job and cannot leave capture alive.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        eprintln!("Microphone diagnostic watchdog expired; terminating the owned local media job");
        std::process::exit(3);
    });
    std::fs::create_dir(&data).map_err(|_| "Could not create the new isolated data directory")?;
    let (sender, mut events) = mpsc::unbounded_channel();
    let bridge = timeout(Duration::from_secs(5), MediaBridge::start(&runtime, &data, sender, eframe::egui::Context::default()))
        .await.map_err(|_| "Local media startup exceeded five seconds")??;
    let guard = CloseOnDrop(bridge.clone());
    let result = timeout_at(deadline - Duration::from_secs(3), sample(&bridge, &mut events)).await
        .unwrap_or_else(|_| Err("Microphone sample deadline expired".into()));
    // Both cleanup commands are attempted after every result, including a
    // rejected start or a stalled callback. close() also cancels the owned job.
    let stopped = local_command(&bridge, LocalCommand::Stop, json!({"token":100})).await.is_ok();
    let shutdown = local_command(&bridge, LocalCommand::Shutdown, json!({})).await.is_ok();
    drop(guard);
    let phases = result?;
    let captured = phases.iter().all(|phase| phase.callbacks>0) && phases.iter().any(|phase| phase.nonzero>0);
    let gate_consistent = phases.iter().all(|phase| phase.gate_mismatches==0 && phase.invalid_levels==0);
    println!("{}", json!({"localOnly":true,"accountUsed":false,"silentMonitor":true,"audioSaved":false,
        "maximumSeconds":15,"capturedPhysicalInput":captured,"manualGateConsistent":gate_consistent,
        "networkTransmissionVerified":false,"stopAcknowledged":stopped,"shutdownAcknowledged":shutdown,
        "phases":phases.iter().map(Phase::summary).collect::<Vec<_>>()}));
    if !stopped || !shutdown { return Err("Cleanup was not acknowledged; owned media job was closed".into()); }
    if !captured { return Err("A phase had no callbacks or input remained at the minimum level; a real voice signal was not verified".into()); }
    if !gate_consistent { return Err("Captured level or configured manual threshold did not match the emitted gate".into()); }
    Ok(())
}

fn main() {
    let result = arguments().and_then(|(runtime, data)| {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()
            .map_err(|_| "Could not start isolated diagnostic runtime".to_owned())?.block_on(run(runtime, data))
    });
    if let Err(error) = result { eprintln!("{error}"); std::process::exit(2); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimum_level_callbacks_do_not_claim_that_a_physical_signal_was_verified() {
        let mut phase=Phase::new(-100.);
        phase.observe(&json!({"monitoring":false,"localOnly":true,"level":0.000001,"db":-100.,"speaking":true})).unwrap();
        assert_eq!(phase.callbacks,1);
        assert_eq!(phase.nonzero,0);
        assert_eq!(phase.invalid_levels,0);
    }
    #[test]
    fn aggregate_gate_check_rejects_wrong_manual_state_without_retaining_samples() {
        let mut phase = Phase::new(-40.);
        let event = |level, db, speaking| json!({"monitoring":false,"localOnly":true,"level":level,"db":db,"speaking":speaking});
        phase.observe(&event(0.01, -40., true)).unwrap();
        phase.observe(&event(0.001, -60., false)).unwrap();
        phase.observe(&event(0., -100., false)).unwrap();
        assert_eq!((phase.callbacks, phase.nonzero, phase.speaking, phase.gate_mismatches, phase.invalid_levels), (3, 2, 1, 0, 0));
        phase.observe(&event(0.001, -60., true)).unwrap();
        assert_eq!(phase.gate_mismatches, 1);
        assert_eq!((phase.minimum_db, phase.maximum_db), (-100., -40.));
    }
    #[test]
    fn monitoring_that_belongs_to_a_voice_session_is_rejected() {
        let mut phase = Phase::new(-40.);
        assert!(phase.observe(&json!({"monitoring":true,"localOnly":true,"level":0.1,"db":-20.,"speaking":true})).is_err());
        assert_eq!(phase.callbacks, 0);
    }
}
