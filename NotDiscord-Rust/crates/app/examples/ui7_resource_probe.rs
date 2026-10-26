//! Offline UI ownership/CPU probe. No GPU renderer or real media transport.
use notdiscord::ui::ui7_probe::Ui7ResourceProbe;
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

fn cpu() -> u64 {
    unsafe {
        let (mut create, mut exit, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(
            GetCurrentProcess(),
            &mut create,
            &mut exit,
            &mut kernel,
            &mut user,
        )
        .expect("own process CPU");
        ((kernel.dwHighDateTime as u64) << 32 | kernel.dwLowDateTime as u64)
            + ((user.dwHighDateTime as u64) << 32 | user.dwLowDateTime as u64)
    }
}
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
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let mode = args
        .get(1)
        .expect("baseline|mentions|dialogs|video|emoji|cleanup");
    assert!(matches!(
        mode.as_str(),
        "baseline" | "mentions" | "dialogs" | "video" | "emoji" | "cleanup"
    ));
    let root = PathBuf::from(args.get(2).expect("fresh absolute E: output directory"));
    assert!(
        root.is_absolute()
            && root.starts_with(r"E:\DESCARGAS\REA\NotDiscordRust")
            && !root.exists()
    );
    let seconds = args
        .get(3)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(25)
        .clamp(10, 60);
    let deadline_seconds = seconds + 120;
    let complete = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let complete_on_watchdog = complete.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        while !complete_on_watchdog.load(std::sync::atomic::Ordering::Acquire) {
            if started.elapsed() > Duration::from_secs(deadline_seconds) {
                eprintln!("Owned offline resource probe exceeded its hard deadline");
                std::process::exit(3);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
    fs::create_dir_all(&root).unwrap();
    let mut probe = Ui7ResourceProbe::new(&root.join("synthetic-data"));
    let initialized = memory();
    let precondition_started = Instant::now();
    if mode == "cleanup" {
        for scenario in ["mentions", "dialogs", "video", "emoji"] {
            for frame in 0..100 * 12 {
                probe.step(scenario, frame);
            }
            probe.cleanup();
        }
    }
    let precondition_ms = precondition_started.elapsed().as_millis();
    let after_precondition = memory();
    let start = Instant::now();
    let period = Duration::from_nanos(16_666_667);
    let mut next_frame = start;
    let mut next_sample = start;
    let mut iteration = 0usize;
    let mut missed_frames = 0u64;
    let mut samples = Vec::new();
    let mut measured_start = None;
    let mut first_cpu = 0;
    let mut peak_nominal_texture_bytes = 0;
    let mut peak_animation_bytes = 0;
    let mut peak_video_handles = 0;
    while start.elapsed() < Duration::from_secs(seconds) {
        let now = Instant::now();
        if now >= next_frame {
            probe.step(mode, iteration);
            iteration += 1;
            next_frame += period;
            if now.saturating_duration_since(next_frame) > Duration::from_millis(250) {
                missed_frames +=
                    now.duration_since(next_frame).as_nanos() as u64 / period.as_nanos() as u64;
                next_frame = now + period;
            }
        }
        if now >= next_sample {
            if measured_start.is_none() && start.elapsed() >= Duration::from_secs(5) {
                measured_start = Some(Instant::now());
                first_cpu = cpu();
            }
            let ui = probe.metrics();
            peak_nominal_texture_bytes =
                peak_nominal_texture_bytes.max(ui["nominalTextureBytes"].as_u64().unwrap());
            peak_animation_bytes =
                peak_animation_bytes.max(ui["customAnimation"]["bytes"].as_u64().unwrap());
            peak_video_handles =
                peak_video_handles.max(ui["videoTextureHandles"].as_u64().unwrap());
            let mut sample = memory();
            sample["elapsedMs"] = json!(start.elapsed().as_millis());
            sample["cpu100ns"] = json!(cpu());
            sample["ui"] = ui;
            samples.push(sample);
            next_sample += Duration::from_millis(250);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let end_cpu = cpu();
    let measured_seconds = measured_start.unwrap().elapsed().as_secs_f64();
    let cpu_ms = (end_cpu - first_cpu) as f64 / 10_000.;
    let before_cleanup = probe.metrics();
    probe.cleanup();
    let after_cleanup = probe.metrics();
    std::thread::sleep(Duration::from_millis(300));
    let released = memory();
    assert_eq!(after_cleanup["videoTextureHandles"], 0);
    assert_eq!(after_cleanup["videoBufferedBytes"], 0);
    assert_eq!(after_cleanup["customAnimation"]["bytes"], 0);
    assert_eq!(after_cleanup["editStateRetained"], false);
    if mode == "video" {
        assert_eq!(peak_video_handles, 2);
    }
    if mode == "emoji" {
        assert!(peak_animation_bytes > 0 && peak_animation_bytes <= 4 * 1024 * 1024);
    }
    let report = json!({"success":true,"mode":mode,"seconds":seconds,"warmupSeconds":5,"targetFramesPerSecond":60,
        "cpuPercentOneCore":cpu_ms/(measured_seconds*10.),"cpuMs":cpu_ms,"measuredSeconds":measured_seconds,
        "framesInTimedPhase":iteration,"missedFrames":missed_frames,"hardDeadlineSeconds":deadline_seconds,"frameLimit":20000,"samples":samples,"initialized":initialized,
        "preconditionMs":precondition_ms,"afterPrecondition":after_precondition,"beforeCleanup":before_cleanup,
        "afterCleanup":after_cleanup,"afterReleaseMemory":released,"peakNominalTextureBytes":peak_nominal_texture_bytes,
        "peakAnimationBytes":peak_animation_bytes,"peakVideoTextureHandles":peak_video_handles,
        "accountUsed":false,"controllerCreated":false,"networkRequestsExecuted":false,"audioDeviceOpened":false,
        "captureEngineCreated":false,"gpuRendererCreated":false,
        "scope":"Same UI7 diagnostic binary and 60Hz headless egui chat. Baseline is current UI7 reference, not historical UI6. Modes repeat open/use/close at 12-frame cycles. Cleanup mode exercises 100 cycles per feature before its measured reference phase. Process RAM/CPU measured; nominal texture byte counters are not GPU measurements. Video uses two synthetic 256x144 RGBA sinks, no codecs/transport/GPU. Emoji uses twelve synthetic 32x32 animations with eight frames each, no download/encoded decode. No Unicode catalog experiment."});
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "{}",
        json!({"success":true,"mode":mode,"cpuPercentOneCore":report["cpuPercentOneCore"],"frames":iteration,"peakAnimationBytes":peak_animation_bytes,"peakVideoTextureHandles":peak_video_handles})
    );
    complete.store(true, std::sync::atomic::Ordering::Release);
}
