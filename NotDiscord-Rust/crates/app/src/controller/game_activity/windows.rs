//! Limited-information process names and visible windows from this Windows session only.
use super::detection::{self,Process,Program,ScanResult,Window};
use ::windows::{core::{BOOL,PWSTR},Win32::{Foundation::{CloseHandle,HWND,LPARAM},System::{ProcessStatus::EnumProcesses,RemoteDesktop::ProcessIdToSessionId,Threading::{GetCurrentProcessId,OpenProcess,PROCESS_NAME_WIN32,PROCESS_QUERY_LIMITED_INFORMATION,QueryFullProcessImageNameW}},UI::WindowsAndMessaging::{EnumWindows,GetForegroundWindow,GetWindowTextW,GetWindowThreadProcessId,IsWindowVisible}}};

struct Windows<'a>{processes:&'a[Process],foreground:u32,windows:Vec<Window>,visited:usize,overflow:bool}
unsafe extern "system" fn collect(hwnd:HWND,param:LPARAM)->BOOL {
    // EnumWindows invokes this synchronously; the stack context outlives the complete enumeration.
    let state=unsafe{&mut*(param.0 as *mut Windows<'_>)};
    state.visited+=1;if state.visited>8192{state.overflow=true;return BOOL(0);}
    if !unsafe{IsWindowVisible(hwnd)}.as_bool(){return BOOL(1);}
    let mut pid=0;unsafe{GetWindowThreadProcessId(hwnd,Some(&mut pid));}
    let Some(process)=state.processes.iter().find(|process|process.pid==pid) else{return BOOL(1);};
    if state.windows.len()>=256{state.overflow=true;return BOOL(0);}
    let title=if process.program==Program::Rpcs3 {
        let mut title=[0u16;1024];let length=unsafe{GetWindowTextW(hwnd,&mut title)};
        if length<=0||length as usize>=title.len()-1{None}else{Some(String::from_utf16_lossy(&title[..length as usize]))}
    }else{None};
    state.windows.push(Window{pid,foreground:pid==state.foreground,rpcs3_title:title});BOOL(1)
}

pub(super) fn scan(previous:Option<&str>)->Result<ScanResult,String> {
    unsafe {
        let mut own_session=0;ProcessIdToSessionId(GetCurrentProcessId(),&mut own_session).map_err(|_|"No se pudo comprobar la sesión de Windows.".to_string())?;
        let mut ids=[0u32;4096];let mut needed=0;
        EnumProcesses(ids.as_mut_ptr(),std::mem::size_of_val(&ids) as u32,&mut needed).map_err(|_|"Windows no permitió enumerar los procesos.".to_string())?;
        if needed as usize>=std::mem::size_of_val(&ids){return Err("La lista de procesos supera el límite local.".into());}
        let mut processes=Vec::new();
        for pid in ids.into_iter().take(needed as usize/4).filter(|pid|*pid!=0) {
            let mut session=u32::MAX;if ProcessIdToSessionId(pid,&mut session).is_err()||session!=own_session{continue;}
            let Ok(handle)=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid) else{continue;};
            let mut path=[0u16;4096];let mut length=path.len() as u32;
            let result=QueryFullProcessImageNameW(handle,PROCESS_NAME_WIN32,PWSTR(path.as_mut_ptr()),&mut length);let _=CloseHandle(handle);
            if result.is_err()||length as usize>=path.len(){continue;}
            // Only the recognized identity survives this expression; no path is stored or emitted.
            if let Some(program)=detection::identify(&String::from_utf16_lossy(&path[..length as usize])) {
                if processes.len()>=64{return Err("Hay demasiados procesos de juegos para la detección local.".into());}
                processes.push(Process{pid,program});
            }
        }
        let mut foreground=0;GetWindowThreadProcessId(GetForegroundWindow(),Some(&mut foreground));
        let mut state=Windows{processes:&processes,foreground,windows:Vec::new(),visited:0,overflow:false};
        let result=EnumWindows(Some(collect),LPARAM(&mut state as *mut Windows<'_> as isize));
        if state.overflow{return Err("La lista de ventanas supera el límite local.".into());}
        result.map_err(|_|"Windows no permitió consultar las ventanas de los juegos.".to_string())?;
        Ok(detection::choose(&processes,&state.windows,previous))
    }
}
