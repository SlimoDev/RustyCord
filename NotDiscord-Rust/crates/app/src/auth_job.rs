//! A named, private-to-the-login-job process group. The child joins before it
//! creates WebView2; the parent is the sole long-lived owner of the job handle.
use std::{
    ffi::c_void,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    time::Duration,
};

const ERROR: &str = "No se pudo proteger el árbol de procesos del inicio de sesión.";
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> *mut c_void;
    fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
    fn SetInformationJobObject(job: *mut c_void, class: u32, info: *const c_void, size: u32)
    -> i32;
    fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
    fn QueryInformationJobObject(
        job: *mut c_void,
        class: u32,
        info: *mut c_void,
        size: u32,
        returned: *mut u32,
    ) -> i32;
    fn TerminateJobObject(job: *mut c_void, exit_code: u32) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn SetLastError(error: u32);
    #[cfg(test)]
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
    #[cfg(test)]
    fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
}
#[repr(C)]
#[derive(Default)]
struct BasicLimit {
    process_time: i64,
    job_time: i64,
    flags: u32,
    min_working_set: usize,
    max_working_set: usize,
    active_processes: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedLimit {
    basic: BasicLimit,
    io: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}
#[repr(C)]
#[derive(Default)]
struct Accounting {
    times: [i64; 4],
    faults: u32,
    total_processes: u32,
    active_processes: u32,
    terminated_processes: u32,
}
fn name(id: &str) -> Result<Vec<u16>, String> {
    if !super::super::valid_job_id(id) {
        return Err(ERROR.into());
    }
    Ok(format!("Local\\NotDiscord.Login.{id}")
        .encode_utf16()
        .chain(Some(0))
        .collect())
}
pub(super) struct ProcessTree {
    handle: OwnedHandle,
}
impl ProcessTree {
    pub(super) fn create(id: &str) -> Result<Self, String> {
        let name = name(id)?;
        unsafe {
            SetLastError(0);
            let raw = CreateJobObjectW(std::ptr::null_mut(), name.as_ptr());
            if raw.is_null() {
                return Err(ERROR.into());
            }
            let existing = std::io::Error::last_os_error().raw_os_error() == Some(183);
            let handle = OwnedHandle::from_raw_handle(raw);
            if existing {
                return Err(ERROR.into());
            }
            let mut limits = ExtendedLimit::default();
            limits.basic.flags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            if SetInformationJobObject(
                handle.as_raw_handle(),
                9,
                (&limits as *const ExtendedLimit).cast(),
                std::mem::size_of::<ExtendedLimit>() as u32,
            ) == 0
            {
                return Err(ERROR.into());
            }
            Ok(Self { handle })
        }
    }
    pub(super) fn terminate(&self) -> Result<(), String> {
        if unsafe { TerminateJobObject(self.handle.as_raw_handle(), 1) } == 0 {
            return Err(ERROR.into());
        }
        Ok(())
    }
    fn active(&self) -> Result<u32, String> {
        let mut accounting = Accounting::default();
        if unsafe {
            QueryInformationJobObject(
                self.handle.as_raw_handle(),
                1,
                (&mut accounting as *mut Accounting).cast(),
                std::mem::size_of::<Accounting>() as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(ERROR.into());
        }
        Ok(accounting.active_processes)
    }
    pub(super) fn wait_empty(&self) -> Result<(), String> {
        for attempt in 0..40 {
            if self.active()? == 0 {
                return Ok(());
            }
            if attempt < 39 {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Err(ERROR.into())
    }
    #[cfg(test)]
    pub(super) fn active_for_test(&self) -> u32 {
        self.active().unwrap()
    }
    #[cfg(test)]
    pub(super) fn contains_process_for_test(&self, process_id: u32) -> Result<bool, String> {
        unsafe {
            let raw = OpenProcess(0x1000, 0, process_id); // QUERY_LIMITED_INFORMATION
            if raw.is_null() {
                return Err(ERROR.into());
            }
            let process = OwnedHandle::from_raw_handle(raw);
            let mut belongs = 0;
            if IsProcessInJob(
                process.as_raw_handle(),
                self.handle.as_raw_handle(),
                &mut belongs,
            ) == 0
            {
                return Err(ERROR.into());
            }
            Ok(belongs != 0)
        }
    }
}
pub(super) fn join_current_process(id: &str) -> Result<(), String> {
    let name = name(id)?;
    unsafe {
        let raw = OpenJobObjectW(1, 0, name.as_ptr()); // JOB_OBJECT_ASSIGN_PROCESS
        if raw.is_null() {
            return Err(ERROR.into());
        }
        let handle = OwnedHandle::from_raw_handle(raw);
        if AssignProcessToJobObject(handle.as_raw_handle(), GetCurrentProcess()) == 0 {
            return Err(ERROR.into());
        }
        // Close the child's temporary job handle before it can create WebView.
        // If the parent died in this interval, the last close kills this child.
        drop(handle);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_fresh_owned_job_can_be_created_and_missing_jobs_cannot_be_joined() {
        assert!(join_current_process("../foreign-job").is_err());
        let id = uuid::Uuid::new_v4().simple().to_string();
        assert!(join_current_process(&id).is_err());
        let tree = ProcessTree::create(&id).unwrap();
        assert!(ProcessTree::create(&id).is_err());
        assert_eq!(tree.active_for_test(), 0);
        tree.terminate().unwrap();
        tree.wait_empty().unwrap();
        drop(tree);
        assert!(join_current_process(&id).is_err());
        // This test never assigns the test harness itself to a job. Only the
        // explicitly spawned helper in auth_lifecycle performs assignment.
    }
}
