//! Whole-clip event playback. File IO and scaling finish before submission;
//! Windows owns the playback clock and optional looping, with no refill timer.
use super::*;
use super::clip_output::NativeClip;
use std::sync::{Condvar, MutexGuard, OnceLock, atomic::{AtomicBool, AtomicU64, Ordering}};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{CreateEventW, ResetEvent, SetEvent, WaitForMultipleObjects},
};

struct OutputEvent(usize);
impl OutputEvent {
    fn handle(&self)->HANDLE {HANDLE(self.0 as *mut std::ffi::c_void)}
}
impl Drop for OutputEvent {
    fn drop(&mut self) {let _=unsafe {CloseHandle(self.handle())};}
}

#[derive(Default)]
pub(super) struct Wake {
    revision: Mutex<u64>,
    changed: Condvar,
    output_event: OnceLock<OutputEvent>,
}
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
impl Wake {
    pub(super) fn notify(&self) {
        let mut revision=lock(&self.revision);
        *revision=revision.wrapping_add(1);
        self.changed.notify_all();
        if let Some(event)=self.output_event.get() {let _=unsafe {SetEvent(event.handle())};}
    }
    fn output_event(&self)->Result<HANDLE,String> {
        if self.output_event.get().is_none() {
            let handle=unsafe {CreateEventW(None,true,false,windows::core::PCWSTR::null())}
                .map_err(|_|"No se pudo crear la señal de sonidos.".to_owned())?;
            let _=self.output_event.set(OutputEvent(handle.0 as usize));
        }
        Ok(self.output_event.get().unwrap().handle())
    }
    fn clear_output_event(&self)->Result<(),String> {
        if let Some(event)=self.output_event.get() {
            unsafe {ResetEvent(event.handle())}.map_err(|_|"No se pudo reiniciar la señal de sonidos.".to_owned())?;
        }
        Ok(())
    }
    fn wait(&self,observed:&mut u64,duration:Duration) {
        let mut revision=lock(&self.revision);
        if *revision==*observed {
            revision=self.changed.wait_timeout(revision,duration)
                .unwrap_or_else(|poisoned|poisoned.into_inner()).0;
        }
        *observed=*revision;
    }
}

#[derive(Clone)]
struct Control {
    ring: Arc<Mutex<Option<Ring>>>,
    shutdown: Arc<AtomicBool>,
    ring_generation: Arc<AtomicU64>,
    effect_generation: Arc<AtomicU64>,
    wake: Arc<Wake>,
}
#[derive(Clone,Copy)]
enum Lane {Effect,Ring}
struct LoadJob {
    ticket:u64,
    event:SoundEvent,
    setting:SoundSetting,
}
struct Loaded {
    ticket:u64,
    samples:Result<Box<[i16]>,String>,
}
#[derive(Default)]
struct LoadLane {
    ticket:u64,
    job:Option<LoadJob>,
    ready:Option<Loaded>,
}
#[derive(Default)]
struct Loads {
    effect:LoadLane,
    ring:LoadLane,
    clear_cache:bool,
}
impl Loads {
    fn lane(&mut self,lane:Lane)->&mut LoadLane {
        match lane {Lane::Effect=>&mut self.effect,Lane::Ring=>&mut self.ring}
    }
    fn replace(&mut self,lane:Lane,sound:Option<(SoundEvent,SoundSetting)>) {
        let slot=self.lane(lane);
        slot.ticket=slot.ticket.wrapping_add(1);
        slot.ready=None;
        slot.job=sound.map(|(event,setting)|LoadJob {ticket:slot.ticket,event,setting});
    }
}

fn load(control:Control,loads:Arc<Mutex<Loads>>) {
    let mut bank=SoundBank::default();
    let mut observed=0;
    while !control.shutdown.load(Ordering::Acquire) {
        let (clear,job)={
            let mut slots=lock(&loads);
            let clear=std::mem::take(&mut slots.clear_cache);
            let job=slots.ring.job.take().map(|job|(Lane::Ring,job))
                .or_else(||slots.effect.job.take().map(|job|(Lane::Effect,job)));
            (clear,job)
        };
        if clear {bank=SoundBank::default();}
        let Some((lane,job))=job else {
            control.wake.wait(&mut observed,Duration::from_millis(250));
            continue;
        };
        // One serial loader, latest-only slots and one bounded clip in flight.
        // No driver handle or application lock is held across this file IO.
        let cadence=matches!(lane,Lane::Ring)&&job.setting.file.is_none();
        let samples=bank.load(job.event,&job.setting)
            .and_then(|clip|prepare_clip(clip,job.setting.volume,cadence));
        if control.shutdown.load(Ordering::Acquire) {break;}
        let mut slots=lock(&loads);
        let slot=slots.lane(lane);
        if slot.ticket==job.ticket {
            slot.ready=Some(Loaded {ticket:job.ticket,samples});
            control.wake.notify();
        }
    }
}

pub(super) fn run(
    rx:mpsc::Receiver<Command>,ring:Arc<Mutex<Option<Ring>>>,shutdown:Arc<AtomicBool>,
    ring_generation:Arc<AtomicU64>,effect_generation:Arc<AtomicU64>,wake:Arc<Wake>,
    errors:mpsc::SyncSender<String>,
) {
    let control=Control {ring,shutdown,ring_generation,effect_generation,wake};
    let event=match control.wake.output_event() {
        Ok(event)=>event,
        Err(error)=>{let _=errors.try_send(error);control.shutdown.store(true,Ordering::Release);return;}
    };
    let loads=Arc::new(Mutex::new(Loads::default()));
    let loader_control=control.clone();
    let loader_slots=loads.clone();
    if std::thread::Builder::new().name("event-audio-loader".into())
        .spawn(move||load(loader_control,loader_slots)).is_err() {
        let _=errors.try_send("No se pudo iniciar la carga de sonidos.".into());
        control.shutdown.store(true,Ordering::Release);return;
    }
    let mut effect:Option<NativeClip>=None;
    let mut ringing:Option<NativeClip>=None;
    let mut effect_epoch=control.effect_generation.load(Ordering::Acquire);
    let mut ring_epoch=control.ring_generation.load(Ordering::Acquire);
    let mut applied_ring=None;
    let mut last=None;
    let mut idle=Instant::now();
    let mut cleared_idle=false;
    'worker: while !control.shutdown.load(Ordering::Acquire) {
        // Clear before reading commands/tickets, so concurrent publication or
        // cancellation leaves a signal pending for the next wait.
        if let Err(error)=control.wake.clear_output_event() {let _=errors.try_send(error);break;}
        if effect.as_ref().is_some_and(NativeClip::finished) {effect=None;}
        if ringing.as_ref().is_some_and(NativeClip::finished) {ringing=None;}
        let current=control.effect_generation.load(Ordering::Acquire);
        if current!=effect_epoch {
            effect_epoch=current;effect=None;last=None;
            lock(&loads).replace(Lane::Effect,None);
        }
        let current=control.ring_generation.load(Ordering::Acquire);
        let desired=lock(&control.ring).clone();
        if current!=ring_epoch||desired!=applied_ring {
            ring_epoch=current;ringing=None;
            lock(&loads).replace(Lane::Ring,desired.as_ref().map(|ring|(ring.event,ring.setting.clone())));
            applied_ring=desired;
            control.wake.notify();
        }
        for _ in 0..16 {
            match rx.try_recv() {
                Ok(Command::Play(owner,event,setting))=>{
                    let current=control.effect_generation.load(Ordering::Acquire);
                    if owner!=current {continue;}
                    if current!=effect_epoch {
                        effect_epoch=current;effect=None;last=None;
                        lock(&loads).replace(Lane::Effect,None);
                    }
                    let now=Instant::now();
                    if last.is_some_and(|(old,at):(SoundEvent,Instant)|old==event&&now.duration_since(at)<Duration::from_millis(120)) {continue;}
                    last=Some((event,now));
                    lock(&loads).replace(Lane::Effect,Some((event,setting)));
                    control.wake.notify();
                }
                Ok(Command::ClearCache)=>{lock(&loads).clear_cache=true;control.wake.notify();}
                Err(mpsc::TryRecvError::Disconnected)=>break 'worker,
                Err(mpsc::TryRecvError::Empty)=>break,
            }
        }
        let (prepared_effect,prepared_ring)={
            let mut slots=lock(&loads);
            (slots.effect.ready.take(),slots.ring.ready.take())
        };
        if let Some(ready)=prepared_effect {
            let current=lock(&loads).effect.ticket==ready.ticket
                &&control.effect_generation.load(Ordering::Acquire)==effect_epoch;
            if current {
                match ready.samples {
                    Ok(samples)=>{
                        // Release the old lane before opening its replacement;
                        // a driver quarantine must also block that next open.
                        effect=None;
                        match NativeClip::new(samples,false,&control.effect_generation,effect_epoch) {
                            Ok(Some(clip)) if !control.shutdown.load(Ordering::Acquire)
                                &&control.effect_generation.load(Ordering::Acquire)==effect_epoch=>effect=Some(clip),
                            Ok(_)=>{},
                            Err(error)=>{let _=errors.try_send(error);}
                        }
                    }
                    Err(error)=>{let _=errors.try_send(error);}
                }
            }
        }
        if let Some(ready)=prepared_ring {
            let current=lock(&loads).ring.ticket==ready.ticket
                &&control.ring_generation.load(Ordering::Acquire)==ring_epoch;
            if current {
                match ready.samples {
                    Ok(samples)=>{
                        ringing=None;
                        match NativeClip::new(samples,true,&control.ring_generation,ring_epoch) {
                            Ok(Some(clip)) if !control.shutdown.load(Ordering::Acquire)
                                &&control.ring_generation.load(Ordering::Acquire)==ring_epoch=>ringing=Some(clip),
                            Ok(_)=>{},
                            Err(error)=>{let _=errors.try_send(error);}
                        }
                    }
                    Err(error)=>{let _=errors.try_send(error);}
                }
            }
        }
        if effect.is_some()||ringing.is_some() {
            idle=Instant::now();cleared_idle=false;
        } else if !cleared_idle&&idle.elapsed()>Duration::from_secs(15) {
            lock(&loads).clear_cache=true;cleared_idle=true;control.wake.notify();
        }
        // These handles remain owned by this thread throughout the wait.
        // Completion frees a clip; it never feeds or restarts the sample clock.
        let mut events=vec![event];
        if let Some(wake)=effect.as_ref().and_then(NativeClip::wake) {events.push(wake);}
        if let Some(wake)=ringing.as_ref().and_then(NativeClip::wake) {events.push(wake);}
        let status=unsafe {WaitForMultipleObjects(&events,false,1000)};
        if status!=WAIT_TIMEOUT && !(WAIT_OBJECT_0.0..WAIT_OBJECT_0.0+events.len() as u32).contains(&status.0) {
            let _=errors.try_send("No se pudo esperar la reproducción de sonidos.".into());break;
        }
    }
    control.shutdown.store(true,Ordering::Release);
    control.wake.notify();
    // The serial loader owns no device. An uninterruptible local read may
    // finish later; its cancelled result is discarded without holding output.
}
