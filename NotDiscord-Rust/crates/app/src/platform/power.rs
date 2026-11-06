//! Positive Windows power notifications, owned outside the controller actor.
//!
//! The callback only records a bounded transition. It does not reconnect,
//! acquire devices, infer sleep from elapsed time or block Windows on network.
//! https://learn.microsoft.com/en-us/windows/win32/power/wm-powerbroadcast
//! https://learn.microsoft.com/en-us/windows/win32/power/pbt-apmresumeautomatic
//! https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#message-only-windows
use std::sync::{Arc,atomic::{AtomicU64,AtomicU8,Ordering}};

const POWER_BROADCAST:u32=0x0218;
const SUSPEND:usize=4;
const RESUME_AUTOMATIC:usize=18;
const RESUME_USER:usize=7;
const STARTING:u8=0;
#[cfg(not(test))]
const ACTIVE:u8=1;
const STOPPED:u8=2;
const FAILED:u8=3;

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum ResumeSource { Automatic, User }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum PowerEvent {
    Suspend {episode:u64},
    Resume {episode:u64,source:ResumeSource},
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
enum Notification { Suspend, Resume(ResumeSource) }
fn parse(message:u32,parameter:usize)->Option<Notification> {
    if message!=POWER_BROADCAST {return None;}
    match parameter {
        SUSPEND=>Some(Notification::Suspend),
        RESUME_AUTOMATIC=>Some(Notification::Resume(ResumeSource::Automatic)),
        RESUME_USER=>Some(Notification::Resume(ResumeSource::User)),
        _=>None,
    }
}
#[derive(Default)]
struct Episodes { episode:u64, suspended:bool }
impl Episodes {
    fn observe(&mut self,notification:Notification)->Option<PowerEvent> {
        match notification {
            Notification::Suspend if !self.suspended=>{
                self.episode=self.episode.saturating_add(1).min(u64::MAX>>2);
                self.suspended=true;Some(PowerEvent::Suspend {episode:self.episode})
            },
            Notification::Resume(source) if self.suspended=>{
                self.suspended=false;Some(PowerEvent::Resume {episode:self.episode,source})
            },
            // A later user-presence notification belongs to the same wake.
            // A resume with no observed suspend cannot revive an old call.
            _=>None,
        }
    }
}
fn stamp(event:PowerEvent)->u64 {
    match event {
        PowerEvent::Suspend {episode}=>episode<<2|1,
        PowerEvent::Resume {episode,source:ResumeSource::Automatic}=>episode<<2|2,
        PowerEvent::Resume {episode,source:ResumeSource::User}=>episode<<2|3,
    }
}
/// Retain only the latest episode. Even if the actor was busy during sleep,
/// it first receives its suspend barrier and then exactly one matching resume.
fn next_event(latest:u64,delivered:&mut u64)->Option<PowerEvent> {
    if latest==0 || latest<=*delivered {return None;}
    let episode=latest>>2;
    if *delivered>>2!=episode || latest&3==1 {
        *delivered=episode<<2|1;
        return Some(PowerEvent::Suspend {episode});
    }
    *delivered=latest;
    Some(PowerEvent::Resume {episode,source:if latest&3==2 {ResumeSource::Automatic}else{ResumeSource::User}})
}

pub struct PowerMonitor {
    latest:Arc<AtomicU64>,
    delivered:u64,
    status:Arc<AtomicU8>,
    #[cfg(not(test))]
    stop:Option<Arc<std::os::windows::io::OwnedHandle>>,
    #[cfg(not(test))]
    worker:Option<std::thread::JoinHandle<()>>,
}
impl PowerMonitor {
    pub fn new()->Self {
        let monitor=Self {latest:Arc::new(AtomicU64::new(0)),delivered:0,
            status:Arc::new(AtomicU8::new(STARTING)),
            #[cfg(not(test))] stop:None,
            #[cfg(not(test))] worker:None,
        };
        #[cfg(not(test))]
        let mut monitor=monitor;
        #[cfg(not(test))]
        native::start(&mut monitor);
        #[cfg(test)]
        monitor.status.store(STOPPED,Ordering::Release);
        monitor
    }
    pub fn take(&mut self)->Option<PowerEvent> {
        next_event(self.latest.load(Ordering::Acquire),&mut self.delivered)
    }
    pub fn failed(&self)->bool {self.status.load(Ordering::Acquire)==FAILED}
}
impl Default for PowerMonitor {fn default()->Self {Self::new()}}
impl Drop for PowerMonitor {
    fn drop(&mut self) {
        #[cfg(not(test))]
        {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::{Foundation::HANDLE,System::Threading::SetEvent};
            if let Some(stop)=&self.stop {let _=unsafe {SetEvent(HANDLE(stop.as_raw_handle()))};}
            if let Some(worker)=self.worker.take() {let _=worker.join();}
        }
    }
}

#[cfg(not(test))]
mod native {
    use super::*;
    use std::{cell::RefCell,os::windows::io::{AsRawHandle,FromRawHandle,OwnedHandle}};
    use windows::{core::PCWSTR,Win32::{
        Foundation::{HANDLE,HINSTANCE,HWND,LPARAM,LRESULT,WAIT_OBJECT_0,WPARAM},
        System::{LibraryLoader::GetModuleHandleW,Threading::{CreateEventW,INFINITE,WaitForSingleObject}},
        UI::WindowsAndMessaging::*,
    }};
    // User32's Windows 8+ window registration also covers modern standby.
    // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registersuspendresumenotification
    #[link(name="user32")]
    unsafe extern "system" {
        fn RegisterSuspendResumeNotification(recipient:*mut std::ffi::c_void,flags:u32)->*mut std::ffi::c_void;
        fn UnregisterSuspendResumeNotification(registration:*mut std::ffi::c_void)->i32;
    }

    struct Recorder {episodes:Episodes,latest:Arc<AtomicU64>}
    thread_local! {static RECORDER:RefCell<Option<Recorder>>=const {RefCell::new(None)};}
    unsafe extern "system" fn window_proc(hwnd:HWND,message:u32,w:WPARAM,l:LPARAM)->LRESULT {
        if let Some(notification)=parse(message,w.0) {
            RECORDER.with(|recorder| {
                if let Some(recorder)=recorder.borrow_mut().as_mut()
                    && let Some(event)=recorder.episodes.observe(notification) {
                    recorder.latest.store(stamp(event),Ordering::Release);
                }
            });
            return LRESULT(1);
        }
        if message==WM_CLOSE {unsafe {PostQuitMessage(0)};return LRESULT(0);}
        if message==WM_DESTROY {unsafe {PostQuitMessage(0)};return LRESULT(0);}
        unsafe {DefWindowProcW(hwnd,message,w,l)}
    }
    struct WindowOwner {hwnd:Option<HWND>,class:Vec<u16>,instance:HINSTANCE,registration:*mut std::ffi::c_void}
    impl Drop for WindowOwner {
        fn drop(&mut self) {
            if !self.registration.is_null() {let _=unsafe {UnregisterSuspendResumeNotification(self.registration)};}
            if let Some(hwnd)=self.hwnd.take() && unsafe {IsWindow(Some(hwnd))}.as_bool() {let _=unsafe {DestroyWindow(hwnd)};}
            let _=unsafe {UnregisterClassW(PCWSTR(self.class.as_ptr()),Some(self.instance))};
            RECORDER.with(|recorder|*recorder.borrow_mut()=None);
        }
    }
    pub(super) fn start(monitor:&mut PowerMonitor) {
        let Ok(stop)=(unsafe {CreateEventW(None,true,false,PCWSTR::null())}) else {
            monitor.status.store(FAILED,Ordering::Release);return;
        };
        // OwnedHandle closes only after both the monitor and its worker release
        // it. Drop signals this object, never a potentially recycled thread ID.
        let stop=Arc::new(unsafe {OwnedHandle::from_raw_handle(stop.0)});
        let worker_stop=stop.clone();let latest=monitor.latest.clone();let status=monitor.status.clone();
        match std::thread::Builder::new().name("notdiscord-power".into()).spawn(move || {
            let result=run(latest,worker_stop,&status);
            status.store(if result.is_ok(){STOPPED}else{FAILED},Ordering::Release);
        }) {
            Ok(worker)=>{monitor.stop=Some(stop);monitor.worker=Some(worker);},
            Err(_)=>monitor.status.store(FAILED,Ordering::Release),
        }
    }
    fn run(latest:Arc<AtomicU64>,stop:Arc<OwnedHandle>,status:&AtomicU8)->Result<(),()> {
        let instance=HINSTANCE(unsafe {GetModuleHandleW(None)}.map_err(|_|())?.0);
        let class=format!("NotDiscord.Power.{}",uuid::Uuid::new_v4().simple()).encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let descriptor=WNDCLASSW {lpfnWndProc:Some(window_proc),hInstance:instance,lpszClassName:PCWSTR(class.as_ptr()),..Default::default()};
        if unsafe {RegisterClassW(&descriptor)}==0 {return Err(());}
        let mut window=WindowOwner {hwnd:None,class,instance,registration:std::ptr::null_mut()};
        RECORDER.with(|recorder|*recorder.borrow_mut()=Some(Recorder {episodes:Episodes::default(),latest}));
        // No WS_VISIBLE and no HWND_MESSAGE: this hidden top-level window
        // receives broadcasts without displaying or activating another window.
        window.hwnd=Some(unsafe {CreateWindowExW(WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE,
            PCWSTR(window.class.as_ptr()),PCWSTR::null(),WS_POPUP,0,0,0,0,None,None,Some(instance),None)}.map_err(|_|())?);
        window.registration=unsafe {RegisterSuspendResumeNotification(window.hwnd.unwrap().0,DEVICE_NOTIFY_WINDOW_HANDLE.0)};
        if window.registration.is_null() {return Err(());}
        status.store(ACTIVE,Ordering::Release);
        let handles=[HANDLE(stop.as_raw_handle())];
        loop {
            let mut message=MSG::default();
            while unsafe {PeekMessageW(&mut message,None,0,0,PM_REMOVE)}.as_bool() {
                if message.message==WM_QUIT || unsafe {WaitForSingleObject(handles[0],0)}==WAIT_OBJECT_0 {return Ok(());}
                let _=unsafe {TranslateMessage(&message)};
                unsafe {DispatchMessageW(&message)};
            }
            let result=unsafe {MsgWaitForMultipleObjects(Some(&handles),false,INFINITE,QS_ALLINPUT)};
            if result==WAIT_OBJECT_0 {return Ok(());}
            if result.0!=WAIT_OBJECT_0.0+1 {return Err(());}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_documented_power_notifications_create_typed_events() {
        assert_eq!(parse(POWER_BROADCAST,SUSPEND),Some(Notification::Suspend));
        assert_eq!(parse(POWER_BROADCAST,RESUME_AUTOMATIC),Some(Notification::Resume(ResumeSource::Automatic)));
        assert_eq!(parse(POWER_BROADCAST,RESUME_USER),Some(Notification::Resume(ResumeSource::User)));
        assert_eq!(parse(0,RESUME_USER),None);
        assert_eq!(parse(POWER_BROADCAST,0x8013),None);
        assert_eq!(parse(POWER_BROADCAST,10),None);
    }
    #[test]
    fn automatic_and_later_user_wake_share_one_episode_without_recovery_rearming() {
        let mut episodes=Episodes::default();
        assert_eq!(episodes.observe(Notification::Resume(ResumeSource::User)),None);
        assert_eq!(episodes.observe(Notification::Suspend),Some(PowerEvent::Suspend {episode:1}));
        assert_eq!(episodes.observe(Notification::Suspend),None);
        assert_eq!(episodes.observe(Notification::Resume(ResumeSource::Automatic)),Some(PowerEvent::Resume {episode:1,source:ResumeSource::Automatic}));
        for source in [ResumeSource::User,ResumeSource::Automatic,ResumeSource::User] {assert_eq!(episodes.observe(Notification::Resume(source)),None);}
        assert_eq!(episodes.observe(Notification::Suspend),Some(PowerEvent::Suspend {episode:2}));
        assert_eq!(episodes.observe(Notification::Resume(ResumeSource::User)),Some(PowerEvent::Resume {episode:2,source:ResumeSource::User}));
    }
    #[test]
    fn a_busy_actor_gets_only_the_latest_suspend_and_resume_in_order() {
        let mut monitor=PowerMonitor::new();
        let mut episodes=Episodes::default();
        for _ in 0..100 {
            for notification in [Notification::Suspend,Notification::Resume(ResumeSource::Automatic)] {
                monitor.latest.store(stamp(episodes.observe(notification).unwrap()),Ordering::Release);
            }
        }
        assert_eq!(monitor.take(),Some(PowerEvent::Suspend {episode:100}));
        assert_eq!(monitor.take(),Some(PowerEvent::Resume {episode:100,source:ResumeSource::Automatic}));
        assert_eq!(monitor.take(),None);
    }
    #[test]
    fn a_new_suspend_replaces_an_unconsumed_old_wake_and_duplicates_stay_quiet() {
        let mut delivered=0;
        assert_eq!(next_event(stamp(PowerEvent::Suspend {episode:1}),&mut delivered),Some(PowerEvent::Suspend {episode:1}));
        assert_eq!(next_event(stamp(PowerEvent::Suspend {episode:1}),&mut delivered),None);
        assert_eq!(next_event(stamp(PowerEvent::Suspend {episode:2}),&mut delivered),Some(PowerEvent::Suspend {episode:2}));
        assert_eq!(next_event(stamp(PowerEvent::Resume {episode:2,source:ResumeSource::User}),&mut delivered),Some(PowerEvent::Resume {episode:2,source:ResumeSource::User}));
        assert_eq!(next_event(stamp(PowerEvent::Resume {episode:2,source:ResumeSource::User}),&mut delivered),None);
    }
}
