//! Comparable offline audio-pipeline benchmark; never opens an audio device.
use notdiscord::sounds::{
    CACHE_LIMIT, CHUNK, RATE, SoundBank, SoundEngine, SoundEvent, SoundMixer, SoundPreferences,
    SoundSetting,
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
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
fn memory() -> serde_json::Value {
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
            json!({"privateUsageBytes":ex2.PrivateUsage,"privateWorkingSetBytes":ex2.PrivateWorkingSetSize,"workingSetBytes":ex2.WorkingSetSize})
        } else {
            let mut ex = PROCESS_MEMORY_COUNTERS_EX::default();
            ex.cb = std::mem::size_of_val(&ex) as u32;
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&mut ex as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
                ex.cb,
            )
            .expect("own process memory");
            json!({"privateUsageBytes":ex.PrivateUsage,"privateWorkingSetBytes":null,"workingSetBytes":ex.WorkingSetSize})
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
        GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u).expect("own CPU time");
        ((k.dwHighDateTime as u64) << 32 | k.dwLowDateTime as u64)
            + ((u.dwHighDateTime as u64) << 32 | u.dwLowDateTime as u64)
    }
}
fn fixture() -> Vec<u8> {
    let data = RATE * 5 * 2;
    let mut b = Vec::with_capacity(data + 44);
    b.extend(b"RIFF");
    b.extend(((data + 36) as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend((RATE as u32).to_le_bytes());
    b.extend((RATE as u32 * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((data as u32).to_le_bytes());
    for i in 0..data / 2 {
        b.extend((((i as f32 * 0.06).sin() * 8000.) as i16).to_le_bytes());
    }
    b
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let mode = args.get(1).expect("mode baseline|idle|default|custom");
    let root = PathBuf::from(args.get(2).expect("fresh output E directory"));
    let seconds = args
        .get(3)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(25)
        .clamp(10, 120);
    assert!(matches!(
        mode.as_str(),
        "baseline"
            | "idle"
            | "warm_idle"
            | "default"
            | "custom"
            | "distinct"
            | "long_baseline"
            | "long_ring"
            | "long_closed"
    ));
    assert!(!root.exists(), "use a fresh directory");
    assert!(root.is_absolute() && root.starts_with(r"E:\DESCARGAS\REA\NotDiscordRust"));
    fs::create_dir_all(&root).unwrap();
    // Equal fixture/config I/O before all measured modes. No real settings are read.
    let path = root.join("synthetic.wav");
    fs::write(&path, fixture()).unwrap();
    let paths = (0..19)
        .map(|i| {
            let p = root.join(format!("synthetic-{i}.wav"));
            fs::copy(&path, &p).unwrap();
            p
        })
        .collect::<Vec<_>>();
    let long_path = root.join("synthetic-minute.wav");
    if mode.starts_with("long_") {
        use std::io::Write;
        let mut header = fixture();
        header.truncate(44);
        let size = (RATE * 60 * 2) as u32;
        header[4..8].copy_from_slice(&(size + 36).to_le_bytes());
        header[40..44].copy_from_slice(&size.to_le_bytes());
        let mut output =
            std::io::BufWriter::with_capacity(8192, fs::File::create(&long_path).unwrap());
        output.write_all(&header).unwrap();
        let block = [0xe8, 0x03].repeat(CHUNK);
        let mut remaining = RATE * 60 * 2;
        while remaining > 0 {
            let count = remaining.min(block.len());
            output.write_all(&block[..count]).unwrap();
            remaining -= count;
        }
        output.flush().unwrap();
    }
    let mut setting = SoundSetting {
        file: (mode == "custom").then_some(path),
        ..Default::default()
    };
    let (mut bank, mut mixer) = (SoundBank::default(), SoundMixer::default());
    if mode == "warm_idle" {
        for (i, event) in SoundEvent::ALL.iter().enumerate() {
            bank.load(
                *event,
                &SoundSetting {
                    file: Some(paths[i].clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        }
    }
    let prefs = SoundPreferences::default();
    let mut idle_engine = (mode == "idle").then(SoundEngine::default);
    if let Some(engine) = &mut idle_engine {
        engine.set_ring(None, &prefs);
    }
    let long_ring = mode == "long_ring" || mode == "long_closed";
    let active = mode == "default" || mode == "custom" || mode == "distinct" || long_ring;
    let started = Instant::now();
    let mut next_chunk = started;
    let mut next_event = started;
    let mut next_sample = started;
    let mut buffer = [0i16; CHUNK];
    let mut samples = Vec::new();
    let mut count = 0usize;
    let mut chunks = 0usize;
    let mut measured_start = None;
    let mut first_cpu = 0;
    let mut checksum = 0i64;
    while started.elapsed() < Duration::from_secs(seconds) {
        let now = Instant::now();
        if now >= next_event {
            if active {
                if long_ring {
                    if count == 0 {
                        let ring_setting = SoundSetting {
                            file: Some(long_path.clone()),
                            ..Default::default()
                        };
                        let clip = bank.load(SoundEvent::IncomingRing, &ring_setting).unwrap();
                        assert_eq!(bank.bytes(), 0);
                        mixer.ring(Some(clip), 0.45);
                    }
                } else {
                    let event = SoundEvent::ALL[count % 19];
                    if mode == "distinct" {
                        setting.file = Some(paths[count % 19].clone());
                    }
                    let clip = bank.load(event, &setting).unwrap();
                    mixer.play(clip.clone(), 0.65);
                    if count == 0 {
                        mixer.ring(Some(clip), 0.45);
                    }
                }
            }
            count += 1;
            next_event += Duration::from_millis(100);
        }
        if mode == "long_closed" && started.elapsed() >= Duration::from_secs(5) {
            mixer.ring(None, 0.);
        }
        while now >= next_chunk {
            if active {
                mixer.render(&mut buffer);
                checksum = checksum.wrapping_add(buffer.iter().map(|s| *s as i64).sum::<i64>());
            }
            chunks += 1;
            next_chunk += Duration::from_nanos((CHUNK as u64 * 1_000_000_000) / RATE as u64);
        }
        if now >= next_sample {
            if measured_start.is_none() && started.elapsed() >= Duration::from_secs(5) {
                measured_start = Some(now);
                first_cpu = cpu();
            }
            let process_cpu = cpu();
            let mut sample = memory();
            sample["elapsedMs"] = json!(started.elapsed().as_millis());
            sample["cpu100ns"] = json!(process_cpu);
            sample["cacheBytes"] = json!(bank.bytes());
            sample["decodes"] = json!(bank.decodes);
            samples.push(sample);
            next_sample += Duration::from_millis(250);
        }
        assert!(bank.bytes() <= CACHE_LIMIT);
        if mode == "warm_idle" && started.elapsed() > Duration::from_secs(15) {
            bank = SoundBank::default();
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    let end_cpu = cpu();
    let elapsed = measured_start.unwrap().elapsed().as_secs_f64();
    let cpu_ms = (end_cpu - first_cpu) as f64 / 10_000.;
    let decodes = bank.decodes;
    let cache = bank.bytes();
    mixer.ring(None, 0.);
    mixer = SoundMixer::default();
    drop(bank);
    std::thread::sleep(Duration::from_millis(300));
    let report = json!({"mode":mode,"durationSeconds":seconds,"warmupSeconds":5,"cpuPercentOneCore":cpu_ms/(elapsed*10.),"cpuMs":cpu_ms,"measuredSeconds":elapsed,"samples":samples,"events":count,"chunks":chunks,"checksum":checksum,"decodes":decodes,"cacheBytes":cache,"cacheLimitBytes":CACHE_LIMIT,"afterRelease":memory(),"mixerReleased":!mixer.active(),"audioDeviceOpened":false,"accountUsed":false,"scope":"Release audio decoder/cache/mixer, paced 48k mono, 10 events/s in effect cases; long_ring=one 60s streaming ring; long_closed=stop it after 5s. Excludes production worker/queue, GUI, WinMM driver/hardware and system file cache."});
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "{}",
        json!({"success":true,"mode":mode,"events":count,"cacheBytes":cache,"decodes":decodes,"cpuPercentOneCore":report["cpuPercentOneCore"]})
    );
}
