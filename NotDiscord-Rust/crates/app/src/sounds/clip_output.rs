//! Submit one complete, bounded PCM clip to the native playback clock.
//! No refill, file IO, mixing or scheduling deadline occurs during playback.
use super::{RATE, RING_SECONDS, WaveFormat, WaveHeader, WinMm, audio_apis, OUTPUT_QUARANTINED};
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::Threading::CreateEventW,
};

const WHDR_DONE: u32 = 0x0000_0001;
const WHDR_BEGINLOOP: u32 = 0x0000_0004;
const WHDR_ENDLOOP: u32 = 0x0000_0008;
const CALLBACK_EVENT: u32 = 0x0005_0000;

struct ClipBuffer {
    // Both allocations stay at their original addresses until unprepare
    // confirms the driver has released the header and the samples it names.
    _samples: Box<[i16]>,
    header: Box<WaveHeader>,
    prepared: bool,
}

pub(super) struct NativeClip {
    apis: &'static WinMm,
    handle: usize,
    wake: Option<HANDLE>,
    buffer: Option<ClipBuffer>,
}

impl NativeClip {
    pub(super) fn new(
        mut samples: Box<[i16]>, repeat: bool, epoch: &AtomicU64, expected: u64,
    ) -> Result<Option<Self>, String> {
        if epoch.load(Ordering::Acquire) != expected {
            return Ok(None);
        }
        if samples.is_empty() || samples.len() > RATE * (RING_SECONDS + 2) {
            return Err("El sonido no tiene una duración de reproducción válida.".into());
        }
        if OUTPUT_QUARANTINED.load(Ordering::Acquire) {
            return Err("El controlador de audio no liberó su memoria. Reinicia NotDiscord para volver a reproducir sonidos.".into());
        }
        let bytes: u32 = samples.len().checked_mul(std::mem::size_of::<i16>())
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| "El sonido supera el tamaño de reproducción permitido.".to_owned())?;
        let apis = audio_apis()?;
        // This lane has a single completion, not a queue that must be drained.
        // Auto-reset also consumes an opening notification without spinning.
        let wake = unsafe {CreateEventW(None, false, false, windows::core::PCWSTR::null())}
            .map_err(|_| "No se pudo crear la señal del sonido.".to_owned())?;
        let format = WaveFormat {
            tag: 1,
            channels: 1,
            rate: RATE as u32,
            bytes: (RATE * 2) as u32,
            align: 2,
            bits: 16,
            extra: 0,
        };
        let mut handle = 0;
        if epoch.load(Ordering::Acquire) != expected {
            let _ = unsafe {CloseHandle(wake)};
            return Ok(None);
        }
        if unsafe {(apis.open)(&mut handle, u32::MAX, &format, wake.0 as usize, 0, CALLBACK_EVENT)} != 0 {
            let _ = unsafe {CloseHandle(wake)};
            return Err("No se pudo abrir la salida del sonido de Windows.".into());
        }
        let header = Box::new(WaveHeader {
            data: samples.as_mut_ptr(),
            len: bytes,
            recorded: 0,
            user: 0,
            flags: 0,
            loops: 0,
            next: std::ptr::null_mut(),
            reserved: 0,
        });
        let mut output = Self {
            apis,
            handle,
            wake: Some(wake),
            buffer: Some(ClipBuffer {_samples: samples, header, prepared: false}),
        };
        let buffer = output.buffer.as_mut().unwrap();
        let header_size = std::mem::size_of::<WaveHeader>() as u32;
        if unsafe {(apis.prepare)(handle, &mut *buffer.header, header_size)} != 0 {
            return Err("No se pudo preparar el sonido para Windows.".into());
        }
        buffer.prepared = true;
        // Preparation requires flags == 0. Only set loop flags afterwards,
        // preserving WHDR_PREPARED. PCM permits both ends on this one block.
        if repeat {
            buffer.header.flags |= WHDR_BEGINLOOP | WHDR_ENDLOOP;
            buffer.header.loops = u32::MAX;
        }
        // Opening or preparing a driver may be slow. A stop that arrived
        // during either operation must not start this now-obsolete clip.
        if epoch.load(Ordering::Acquire) != expected {
            return Ok(None);
        }
        if unsafe {(apis.write)(handle, &mut *buffer.header, header_size)} != 0 {
            return Err("Windows no pudo iniciar el sonido.".into());
        }
        // Cover cancellation during write as well: drop immediately instead
        // of installing the stale clip in the worker's active lane.
        if epoch.load(Ordering::Acquire) != expected {
            return Ok(None);
        }
        Ok(Some(output))
    }

    pub(super) fn finished(&self) -> bool {
        self.buffer.as_ref().is_none_or(|buffer| {
            // WHDR_DONE belongs to the driver after submission.
            (unsafe {std::ptr::addr_of!(buffer.header.flags).read_volatile()}) & WHDR_DONE != 0
        })
    }

    pub(super) fn wake(&self) -> Option<HANDLE> {
        self.wake
    }
}

impl Drop for NativeClip {
    fn drop(&mut self) {
        // Reset cancels even a native loop. Only successful unprepare proves
        // the driver has released the allocations, regardless of reset status.
        let _ = unsafe {(self.apis.reset)(self.handle)};
        if let Some(mut buffer) = self.buffer.take() {
            if buffer.prepared && unsafe {
                (self.apis.unprepare)(
                    self.handle, &mut *buffer.header, std::mem::size_of::<WaveHeader>() as u32,
                )
            } != 0 {
                OUTPUT_QUARANTINED.store(true, Ordering::Release);
                // Do not close a live device or free memory it still owns.
                // The worker has at most one effect and one ring; quarantine
                // blocks further opens, bounding retained resources to them.
                std::mem::forget(buffer);
                return;
            }
        }
        if unsafe {(self.apis.close)(self.handle)} == 0 {
            if let Some(wake) = self.wake.take() {
                let _ = unsafe {CloseHandle(wake)};
            }
        } else {
            // Even after PCM release, the device may still signal its event.
            // Keep that event and handle together until process termination.
            OUTPUT_QUARANTINED.store(true, Ordering::Release);
        }
    }
}
