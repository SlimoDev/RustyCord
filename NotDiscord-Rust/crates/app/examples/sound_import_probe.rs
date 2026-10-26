//! Offline import/reuse memory probe. No controller, network, or audio device.
use notdiscord::sounds::{
    RATE, SoundEvent,
    import::{ImportedSound, import_file},
};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, mpsc},
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::FILETIME,
    System::{
        ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
            PROCESS_MEMORY_COUNTERS_EX2,
        },
        Threading::{GetCurrentProcess, GetProcessTimes},
    },
};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct Memory {
    private_usage_bytes: usize,
    private_working_set_bytes: Option<usize>,
    working_set_bytes: usize,
}
fn memory() -> Memory {
    unsafe {
        let mut ex2 = PROCESS_MEMORY_COUNTERS_EX2::default();
        ex2.cb = std::mem::size_of_val(&ex2) as u32;
        if GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut ex2 as *mut PROCESS_MEMORY_COUNTERS_EX2).cast::<PROCESS_MEMORY_COUNTERS>(),
            ex2.cb,
        )
        .is_ok()
        {
            Memory {
                private_usage_bytes: ex2.PrivateUsage,
                private_working_set_bytes: Some(ex2.PrivateWorkingSetSize),
                working_set_bytes: ex2.WorkingSetSize,
            }
        } else {
            let mut ex = PROCESS_MEMORY_COUNTERS_EX::default();
            ex.cb = std::mem::size_of_val(&ex) as u32;
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&mut ex as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
                ex.cb,
            )
            .expect("own process memory");
            Memory {
                private_usage_bytes: ex.PrivateUsage,
                private_working_set_bytes: None,
                working_set_bytes: ex.WorkingSetSize,
            }
        }
    }
}
fn cpu() -> u64 {
    unsafe {
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u)
            .expect("own process CPU time");
        ((k.dwHighDateTime as u64) << 32 | k.dwLowDateTime as u64)
            + ((u.dwHighDateTime as u64) << 32 | u.dwLowDateTime as u64)
    }
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct Peaks {
    sample_count: usize,
    private_usage_bytes: usize,
    private_working_set_bytes: Option<usize>,
    working_set_bytes: usize,
}
impl Peaks {
    fn new(m: Memory) -> Self {
        Self {
            sample_count: 0,
            private_usage_bytes: 0,
            private_working_set_bytes: m.private_working_set_bytes.map(|_| 0),
            working_set_bytes: 0,
        }
    }
    fn observe(&mut self, m: Memory) {
        self.sample_count += 1;
        self.private_usage_bytes = self.private_usage_bytes.max(m.private_usage_bytes);
        self.working_set_bytes = self.working_set_bytes.max(m.working_set_bytes);
        self.private_working_set_bytes =
            match (self.private_working_set_bytes, m.private_working_set_bytes) {
                (Some(a), Some(b)) => Some(a.max(b)),
                _ => None,
            };
    }
}
enum MonitorCommand {
    Begin(mpsc::SyncSender<Memory>),
    End(mpsc::SyncSender<(Memory, Peaks)>),
    Stop,
}
struct Sampler {
    tx: mpsc::Sender<MonitorCommand>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Sampler {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("offline-import-sampler".into())
            .spawn(move || {
                let mut active: Option<Peaks> = None;
                loop {
                    match rx.recv_timeout(Duration::from_millis(2)) {
                        Ok(MonitorCommand::Begin(reply)) => {
                            assert!(active.is_none());
                            let m = memory();
                            let mut peaks = Peaks::new(m);
                            peaks.observe(m);
                            active = Some(peaks);
                            let _ = reply.send(m);
                        }
                        Ok(MonitorCommand::End(reply)) => {
                            let m = memory();
                            let mut peaks = active.take().expect("active phase");
                            peaks.observe(m);
                            let _ = reply.send((m, peaks));
                        }
                        Ok(MonitorCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if let Some(peaks) = &mut active {
                                peaks.observe(memory());
                            }
                        }
                    }
                }
            })
            .expect("offline memory sampler");
        Self {
            tx,
            thread: Some(thread),
        }
    }
    fn begin(&self) -> Memory {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx.send(MonitorCommand::Begin(tx)).unwrap();
        rx.recv().unwrap()
    }
    fn end(&self) -> (Memory, Peaks) {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx.send(MonitorCommand::End(tx)).unwrap();
        rx.recv().unwrap()
    }
}
impl Drop for Sampler {
    fn drop(&mut self) {
        let _ = self.tx.send(MonitorCommand::Stop);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("sampler stopped");
        }
    }
}
fn hash_file(path: &Path) -> String {
    let mut input = File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut block = [0u8; 16384];
    loop {
        let n = input.read(&mut block).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&block[..n]);
    }
    format!("{:x}", hash.finalize())
}
fn inspect_pcm(path: &Path) -> (usize, u64) {
    let mut file = File::open(path).unwrap();
    let length = file.metadata().unwrap().len();
    let mut h = [0u8; 44];
    file.read_exact(&mut h).unwrap();
    let u16at = |i| u16::from_le_bytes([h[i], h[i + 1]]);
    let u32at = |i| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
    assert_eq!(&h[..4], b"RIFF");
    assert_eq!(&h[8..20], b"WAVEfmt \x10\0\0\0");
    assert_eq!(&h[36..40], b"data");
    assert_eq!(u16at(20), 1);
    assert_eq!(u16at(22), 1);
    assert_eq!(u16at(34), 16);
    assert_eq!(u32at(24), RATE as u32);
    assert_eq!(u32at(28), (RATE * 2) as u32);
    assert_eq!(u16at(32), 2);
    assert_eq!(u32at(4) as u64 + 8, length);
    assert_eq!(u32at(40) as u64 + 44, length);
    assert_eq!(u32at(40) % 2, 0);
    let frames = u32at(40) as usize / 2;
    assert!((RATE * 59..=RATE * 60).contains(&frames));
    (frames, length)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Phase {
    name: &'static str,
    reused: bool,
    wall_ms: f64,
    cpu_ms: f64,
    before: Memory,
    after_immediate: Memory,
    peak: Peaks,
    after_settle: Memory,
    settle_ms: u64,
}
fn measure(
    sampler: &Sampler,
    name: &'static str,
    source: &Path,
    data: &Path,
    protected: &[PathBuf],
) -> (ImportedSound, Phase) {
    let before = sampler.begin();
    let first_cpu = cpu();
    let clock = Instant::now();
    let imported = import_file(
        source,
        SoundEvent::IncomingRing,
        data,
        protected,
        &AtomicBool::new(false),
    );
    let wall_ms = clock.elapsed().as_secs_f64() * 1000.;
    let cpu_ms = (cpu() - first_cpu) as f64 / 10000.;
    let (after_immediate, peak) = sampler.end();
    let imported = imported.unwrap_or_else(|error| panic!("{name}: {error}"));
    thread::sleep(Duration::from_millis(250));
    let after_settle = memory();
    let phase = Phase {
        name,
        reused: imported.reused,
        wall_ms,
        cpu_ms,
        before,
        after_immediate,
        peak,
        after_settle,
        settle_ms: 250,
    };
    (imported, phase)
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    assert_eq!(args.len(), 4, "codec fixture fresh-E-output-directory");
    let codec = &args[1];
    assert!(matches!(codec.as_str(), "mp3" | "ogg"));
    let fixture = PathBuf::from(&args[2]);
    let root = PathBuf::from(&args[3]);
    for path in [&fixture, &root] {
        assert!(path.is_absolute() && path.starts_with(r"E:\DESCARGAS\REA\NotDiscordRust"));
    }
    let filename = format!("synthetic-60s.{codec}");
    assert_eq!(
        fixture.file_name().unwrap().to_str(),
        Some(filename.as_str())
    );
    assert!(!root.exists(), "use a fresh owned directory");
    fs::create_dir_all(&root).unwrap();
    let data = root.join("data");
    let source = root.join(&filename);
    fs::copy(&fixture, &source).unwrap();
    let original_hash = hash_file(&fixture);
    let source_hash = hash_file(&source);
    assert_eq!(source_hash, original_hash);
    let source_length = fs::metadata(&source).unwrap().len();
    assert!(source_length <= 8 * 1024 * 1024);
    let sampler = Sampler::new();
    thread::sleep(Duration::from_millis(100));
    let (first, fresh) = measure(&sampler, "fresh", &source, &data, &[]);
    assert!(!first.reused);
    assert_eq!(first.source_name, filename);
    let (frames, output_length) = inspect_pcm(&first.path);
    let output_hash = hash_file(&first.path);
    let first_modified = fs::metadata(&first.path).unwrap().modified().unwrap();
    let (again, reuse) = measure(
        &sampler,
        "reuse",
        &source,
        &data,
        std::slice::from_ref(&first.path),
    );
    assert!(again.reused);
    assert_eq!(again.path, first.path);
    assert_eq!(again.source_name, filename);
    assert_eq!(inspect_pcm(&again.path), (frames, output_length));
    assert_eq!(hash_file(&again.path), output_hash);
    assert_eq!(
        fs::metadata(&again.path).unwrap().modified().unwrap(),
        first_modified
    );
    let source_intact = hash_file(&source) == source_hash && hash_file(&fixture) == original_hash;
    assert!(source_intact);
    let assets = fs::read_dir(data.join("custom-sounds-v1"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(assets, vec![first.path.clone()]);
    drop(sampler);
    let report = json!({
        "success":true,"codec":codec,"source":source,"fixture":fixture,"sourceSha256":source_hash,"fixtureSha256":original_hash,"sourceBytes":source_length,"sourceIntact":source_intact,
        "output":first.path,"outputSha256":output_hash,"outputBytes":output_length,"outputFrames":frames,"outputSeconds":frames as f64/RATE as f64,"outputPcmBits":16,"outputChannels":1,"outputRate":RATE,"outputRewrittenOnReuse":false,"sourceName":first.source_name,
        "samplePeriodMs":2,"phases":[fresh,reuse],"accountUsed":false,"audioDeviceOpened":false,"controllerCreated":false,"networkRequestsExecuted":false,"samplerStopped":true,"managedAssetCount":assets.len(),"temporaryAssets":0,
        "scope":"Real production import_file call, synthetic stereo 44.1 kHz 60 s MP3 or Vorbis, canonical mono 48 kHz PCM16 output. Fresh conversion then reuse in the same process; reuse has a warmed allocator and file cache. No playback, worker/queue, GUI, controller, account or audio device. Separate sampler thread measures process EX2/private commit every approximately 2 ms plus endpoints, retaining only maxima/counters. Sampled maxima can miss briefer peaks and include the sampler; they are not exact allocator maxima. CPU is whole-process GetProcessTimes over each call and includes sampler overhead; very short calls can quantize to zero. Source/output hashes and WAV header checks are outside the timed calls and use fixed-size buffers. Post-call 250 ms snapshots show process retention, including allocator pages; they do not prove a leak. System file cache is excluded."
    });
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "{}",
        json!({"success":true,"codec":codec,"freshMs":report["phases"][0]["wallMs"],"reuseMs":report["phases"][1]["wallMs"],"outputBytes":output_length})
    );
}
