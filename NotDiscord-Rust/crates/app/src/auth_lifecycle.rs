//! Ownership records and bounded cleanup for this executable's login jobs.
//! No browser-wide scans, user credentials, process names or external profiles.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

const OWNER: &str = "NotDiscord.Rust.LoginJob";
const MAX_RECORDS: usize = 64;
const MAX_SCAN_ENTRIES: usize = 256;
const REPARSE_POINT: u32 = 0x400;
const CLEANUP_FAILED: &str = "No se pudo completar la limpieza del inicio de sesión. Los recursos propios pendientes se revisarán en el próximo intento; la sesión no se ha guardado.";

fn cleanup_failure(stage: &'static str, error: Option<&std::io::Error>) -> String {
    #[cfg(test)]
    {
        // Never include paths, credentials, browser contents or arbitrary
        // error display strings in native lifecycle diagnostics.
        format!(
            "{CLEANUP_FAILED} [stage={stage}; win32={}]",
            error.and_then(std::io::Error::raw_os_error).unwrap_or(0)
        )
    }
    #[cfg(not(test))]
    {
        let _ = (stage, error);
        CLEANUP_FAILED.into()
    }
}

#[path = "auth_job.rs"]
mod process_job;

#[cfg(test)]
#[path = "auth_webview_probe.rs"]
mod webview_probe;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    owner: String,
    version: u8,
    id: String,
}

#[derive(Clone)]
struct Paths {
    jobs: PathBuf,
    handoffs: PathBuf,
    profiles: PathBuf,
    id: String,
}
impl Paths {
    fn prepare(data: &Path, id: &str) -> Result<Self, String> {
        if !super::valid_job_id(id) {
            return Err(CLEANUP_FAILED.into());
        }
        fs::create_dir_all(data).map_err(|_| CLEANUP_FAILED)?;
        let root = fs::canonicalize(data).map_err(|_| CLEANUP_FAILED)?;
        Ok(Self {
            jobs: owned_directory(&root, "login-jobs")?,
            handoffs: owned_directory(&root, "login-handoff")?,
            profiles: owned_directory(&root, "login-webview")?,
            id: id.to_owned(),
        })
    }
    fn record(&self) -> PathBuf {
        self.jobs.join(format!("{}.json", self.id))
    }
    fn parent_lock(&self) -> PathBuf {
        self.jobs.join(format!("{}.parent.lock", self.id))
    }
    fn child_lock(&self) -> PathBuf {
        self.jobs.join(format!("{}.child.lock", self.id))
    }
    fn profile(&self) -> PathBuf {
        self.profiles.join(&self.id)
    }
    fn handoff(&self) -> PathBuf {
        self.handoffs.join(format!("{}.dpapi", self.id))
    }
    fn record_valid(&self) -> bool {
        plain_file(&self.record())
            && crate::storage::read_bounded(&self.record(), 512)
                .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok())
                .is_some_and(|record| {
                    record.owner == OWNER && record.version == 1 && record.id == self.id
                })
    }
}

fn plain_metadata(path: &Path) -> Option<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).ok()?;
    (!metadata.file_type().is_symlink() && metadata.file_attributes() & REPARSE_POINT == 0)
        .then_some(metadata)
}
fn plain_file(path: &Path) -> bool {
    plain_metadata(path).is_some_and(|metadata| metadata.is_file())
}
fn owned_directory(root: &Path, name: &str) -> Result<PathBuf, String> {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path).map_err(|_| CLEANUP_FAILED)?
        }
        Err(_) => return Err(CLEANUP_FAILED.into()),
        Ok(_) => {}
    }
    let metadata = plain_metadata(&path).ok_or(CLEANUP_FAILED)?;
    let resolved = fs::canonicalize(&path).map_err(|_| CLEANUP_FAILED)?;
    if !metadata.is_dir()
        || resolved.parent() != Some(root)
        || resolved.file_name() != Some(std::ffi::OsStr::new(name))
    {
        return Err(CLEANUP_FAILED.into());
    }
    Ok(resolved)
}
fn open_lease(path: &Path, create: bool) -> std::io::Result<File> {
    if !create {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.file_attributes() & REPARSE_POINT != 0
        {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
        }
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(create)
        .share_mode(0)
        .open(path)
}

struct Job {
    paths: Paths,
    parent: Option<File>,
    child: Option<File>,
    process_tree: Option<process_job::ProcessTree>,
}
impl Job {
    fn cleanup(mut self) -> Result<(), String> {
        if let Some(tree) = &self.process_tree {
            tree.wait_empty()?;
        }
        // A real child owns the second lease until its login window has closed.
        // Parent cleanup must not race a WebView process still using its profile.
        if self.child.is_none() {
            self.child = Some(
                open_lease(&self.paths.child_lock(), false)
                    .map_err(|error| cleanup_failure("child-lease", Some(&error)))?,
            );
        }
        if !self.paths.record_valid() {
            return Err(cleanup_failure("ownership-record", None));
        }
        remove_owned_file(&self.paths.handoff())?;
        remove_atomic_temps(&self.paths.handoffs, &self.paths.id)?;
        cleanup_profile(&self.paths)?;
        remove_owned_file(&self.paths.record())?;
        remove_atomic_temps(&self.paths.jobs, &self.paths.id)?;
        self.child.take();
        self.parent.take();
        let _ = remove_owned_file(&self.paths.child_lock());
        let _ = remove_owned_file(&self.paths.parent_lock());
        Ok(())
    }
}

fn remove_owned_file(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(cleanup_failure("owned-file-metadata", Some(&error))),
        Ok(_) if !plain_file(path) => Err(cleanup_failure("owned-file-plain-validation", None)),
        Ok(_) => fs::remove_file(path)
            .map_err(|error| cleanup_failure("owned-file-remove", Some(&error))),
    }
}
fn remove_atomic_temps(parent: &Path, id: &str) -> Result<(), String> {
    // storage::atomic_write uses exactly <job>.<32 hex>.tmp. Do not interpret
    // arbitrary filenames beginning with a UUID as owned resources.
    let entries = fs::read_dir(parent)
        .map_err(|error| cleanup_failure("atomic-temp-enumerate", Some(&error)))?;
    for entry in entries.take(MAX_SCAN_ENTRIES) {
        let entry = entry.map_err(|error| cleanup_failure("atomic-temp-entry", Some(&error)))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(suffix) = name
            .strip_prefix(id)
            .and_then(|value| value.strip_prefix('.'))
            .and_then(|value| value.strip_suffix(".tmp"))
        else {
            continue;
        };
        if super::valid_job_id(suffix) {
            remove_owned_file(&entry.path())?;
        }
    }
    Ok(())
}
#[cfg(test)]
fn safe_profile_tree(path: &Path, depth: usize, budget: &mut usize) -> bool {
    check_profile_tree(path, depth, budget).is_ok()
}
struct ProfileTreeFailure {
    stage: &'static str,
    io: Option<std::io::Error>,
}
impl ProfileTreeFailure {
    fn unsafe_tree(stage: &'static str) -> Self {
        Self { stage, io: None }
    }
    fn io(stage: &'static str, error: std::io::Error) -> Self {
        Self {
            stage,
            io: Some(error),
        }
    }
    fn retryable(&self) -> bool {
        self.io
            .as_ref()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }
    fn message(&self) -> String {
        cleanup_failure(self.stage, self.io.as_ref())
    }
}
fn check_profile_tree(
    path: &Path,
    depth: usize,
    budget: &mut usize,
) -> Result<(), ProfileTreeFailure> {
    check_profile_tree_with_metadata(path, depth, budget, &|path| fs::symlink_metadata(path))
}
fn check_profile_tree_with_metadata(
    path: &Path,
    depth: usize,
    budget: &mut usize,
    metadata: &impl Fn(&Path) -> std::io::Result<fs::Metadata>,
) -> Result<(), ProfileTreeFailure> {
    if depth > 32 || *budget == 0 {
        return Err(ProfileTreeFailure::unsafe_tree("profile-tree-budget"));
    }
    *budget -= 1;
    let attributes =
        metadata(path).map_err(|error| ProfileTreeFailure::io("profile-tree-metadata", error))?;
    if attributes.file_type().is_symlink() || attributes.file_attributes() & REPARSE_POINT != 0 {
        return Err(ProfileTreeFailure::unsafe_tree("profile-tree-reparse"));
    }
    if !attributes.is_dir() {
        return attributes
            .is_file()
            .then_some(())
            .ok_or_else(|| ProfileTreeFailure::unsafe_tree("profile-tree-file-type"));
    }
    let entries = fs::read_dir(path)
        .map_err(|error| ProfileTreeFailure::io("profile-tree-enumerate", error))?;
    for entry in entries {
        let entry = entry.map_err(|error| ProfileTreeFailure::io("profile-tree-entry", error))?;
        check_profile_tree_with_metadata(&entry.path(), depth + 1, budget, metadata)?;
    }
    Ok(())
}
fn cleanup_profile(paths: &Paths) -> Result<(), String> {
    cleanup_profile_with_scan(paths, |path| check_profile_tree(path, 0, &mut 20_000))
}
fn cleanup_profile_with_scan(
    paths: &Paths,
    mut scan: impl FnMut(&Path) -> Result<(), ProfileTreeFailure>,
) -> Result<(), String> {
    let path = paths.profile();
    let mut last_error = None;
    for attempt in 0..12 {
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(cleanup_failure("profile-root-metadata", Some(&error))),
            Ok(_) => {}
        }
        let resolved = fs::canonicalize(&path)
            .map_err(|error| cleanup_failure("profile-root-canonicalize", Some(&error)))?;
        if resolved.parent() != Some(paths.profiles.as_path())
            || resolved.file_name().and_then(|value| value.to_str()) != Some(&paths.id)
        {
            return Err(cleanup_failure("profile-root-boundary", None));
        }
        match scan(&path) {
            Ok(()) => match fs::remove_dir_all(&resolved) {
                Ok(()) => return Ok(()),
                Err(error) => last_error = Some(cleanup_failure("profile-remove", Some(&error))),
            },
            // WebView can finish deleting an entry between read_dir and its
            // metadata query even after its process tree has become empty.
            // Retry the whole validation; never delete a partially checked tree.
            Err(error) if error.retryable() => last_error = Some(error.message()),
            Err(error) => return Err(error.message()),
        }
        if attempt < 11 {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    Err(last_error.unwrap_or_else(|| cleanup_failure("profile-remove-retry-exhausted", None)))
}

/// A fresh job keeps ownership until cleanup completes, even if its async caller
/// is cancelled. Drop schedules bounded work on a native thread independent of
/// the cancelled Tokio task; a failure leaves its small recovery record.
pub(super) struct Resources {
    job: Option<Job>,
    child: Option<tokio::process::Child>,
    state: Arc<AtomicU8>,
    #[cfg(test)]
    failure: Arc<std::sync::Mutex<Option<String>>>,
}
impl Resources {
    pub(super) fn prepare(data: &Path, id: &str) -> Result<Self, String> {
        let paths = Paths::prepare(data, id)?;
        let process_tree = process_job::ProcessTree::create(id)?;
        let parent = open_lease(&paths.parent_lock(), true).map_err(|_| CLEANUP_FAILED)?;
        // Both files exist before the child can run. Recovery acquires both and
        // a delayed child cannot recreate a lease after its job was removed.
        let child_file = open_lease(&paths.child_lock(), true).map_err(|_| CLEANUP_FAILED)?;
        drop(child_file);
        let record = serde_json::to_vec(&Record {
            owner: OWNER.into(),
            version: 1,
            id: id.into(),
        })
        .map_err(|_| CLEANUP_FAILED)?;
        crate::storage::atomic_write(&paths.record(), &record)?;
        Ok(Self {
            job: Some(Job {
                paths,
                parent: Some(parent),
                child: None,
                process_tree: Some(process_tree),
            }),
            child: None,
            state: Arc::new(AtomicU8::new(0)),
            #[cfg(test)]
            failure: Arc::new(std::sync::Mutex::new(None)),
        })
    }
    pub(super) fn attach(&mut self, child: tokio::process::Child) {
        self.child = Some(child);
    }
    pub(super) fn child_mut(&mut self) -> &mut tokio::process::Child {
        self.child
            .as_mut()
            .expect("login child attached before waiting")
    }
    pub(super) fn read_handoff(&self) -> Option<Vec<u8>> {
        let path = self.job.as_ref()?.paths.handoff();
        if !plain_file(&path) {
            return None;
        }
        crate::storage::read_bounded(&path, 65536)
    }
    pub(super) async fn finish(self) -> Result<(), String> {
        let (send, receive) = tokio::sync::oneshot::channel();
        self.schedule(Some(send));
        receive.await.unwrap_or_else(|_| Err(CLEANUP_FAILED.into()))
    }
    fn schedule(mut self, reply: Option<tokio::sync::oneshot::Sender<Result<(), String>>>) {
        self.schedule_inner(reply);
    }
    fn schedule_inner(&mut self, reply: Option<tokio::sync::oneshot::Sender<Result<(), String>>>) {
        let Some(job) = self.job.take() else { return };
        let mut child = self.child.take();
        // The login child joins this group before creating WebView2, so this
        // terminates its owned browser descendants as well as the login window.
        if let Some(tree) = &job.process_tree {
            let _ = tree.terminate();
        }
        // start_kill is synchronous: cancellation does not need an active Tokio
        // executor to request termination of the exact child handle it owns.
        if let Some(child) = &mut child {
            let _ = child.start_kill();
        }
        let state = self.state.clone();
        let fallback_state = state.clone();
        #[cfg(test)]
        let failure = self.failure.clone();
        let spawned = std::thread::Builder::new()
            .name("notdiscord-login-cleanup".into())
            .spawn(move || {
                let stopped = if let Some(child) = &mut child {
                    let mut stopped = false;
                    for attempt in 0..40 {
                        match child.try_wait() {
                            Ok(Some(_)) => {
                                stopped = true;
                                break;
                            }
                            Err(_) => break,
                            Ok(None) => {
                                if attempt < 39 {
                                    std::thread::sleep(Duration::from_millis(50));
                                }
                            }
                        }
                    }
                    stopped
                } else {
                    true
                };
                let result = if stopped {
                    job.cleanup()
                } else {
                    Err(cleanup_failure("child-reap", None))
                };
                #[cfg(test)]
                if let Err(error) = &result {
                    *failure.lock().unwrap() = Some(error.clone());
                }
                state.store(if result.is_ok() { 1 } else { 2 }, Ordering::Release);
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            });
        if spawned.is_err() {
            fallback_state.store(2, Ordering::Release);
        }
    }
}
impl Drop for Resources {
    fn drop(&mut self) {
        self.schedule_inner(None);
    }
}

pub(super) struct ChildLease {
    _lease: File,
    paths: Paths,
}
impl ChildLease {
    pub(super) fn acquire(data: &Path, id: &str) -> Option<Self> {
        let paths = Paths::prepare(data, id).ok()?;
        let lease = open_lease(&paths.child_lock(), false).ok()?;
        if !paths.record_valid() {
            return None;
        }
        Some(Self {
            _lease: lease,
            paths,
        })
    }
    pub(super) fn join_process(&self) -> Result<(), String> {
        process_job::join_current_process(&self.paths.id)
    }
    pub(super) fn profile(&self) -> PathBuf {
        self.paths.profile()
    }
    pub(super) fn handoff(&self) -> PathBuf {
        self.paths.handoff()
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct RecoveryReport {
    pub removed: usize,
    pub active: usize,
    pub deferred: usize,
}
pub(super) fn recover_inactive(data: &Path) -> Result<RecoveryReport, String> {
    // A dummy UUID only selects the fixed owned directories; it is never used
    // as a job candidate or cleanup target.
    let roots = Paths::prepare(data, "00000000000000000000000000000000")?;
    let mut report = RecoveryReport::default();
    let entries = fs::read_dir(&roots.jobs).map_err(|_| CLEANUP_FAILED)?;
    let mut considered = 0;
    for entry in entries.take(MAX_SCAN_ENTRIES) {
        if considered >= MAX_RECORDS {
            break;
        }
        let entry = entry.map_err(|_| CLEANUP_FAILED)?;
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|name| name.strip_suffix(".json"))
            .filter(|id| super::valid_job_id(id))
        else {
            continue;
        };
        let paths = Paths {
            id: id.into(),
            ..roots.clone()
        };
        if !paths.record_valid() {
            continue;
        }
        considered += 1;
        let parent = match open_lease(&paths.parent_lock(), false) {
            Ok(lease) => lease,
            Err(error) => {
                record_lease_failure(&mut report, &error);
                continue;
            }
        };
        let child = match open_lease(&paths.child_lock(), false) {
            Ok(lease) => lease,
            Err(error) => {
                record_lease_failure(&mut report, &error);
                continue;
            }
        };
        if (Job {
            paths,
            parent: Some(parent),
            child: Some(child),
            process_tree: None,
        })
        .cleanup()
        .is_ok()
        {
            report.removed += 1;
        } else {
            report.deferred += 1;
        }
    }
    Ok(report)
}
fn record_lease_failure(report: &mut RecoveryReport, error: &std::io::Error) {
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        report.active += 1;
    } else {
        report.deferred += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "notdiscord-auth-lifecycle-{}",
                uuid::Uuid::new_v4()
            )))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let Ok(resolved) = fs::canonicalize(&self.0) else {
                return;
            };
            let Ok(temporary) = fs::canonicalize(std::env::temp_dir()) else {
                return;
            };
            if resolved.parent() == Some(temporary.as_path())
                && resolved.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with("notdiscord-auth-lifecycle-")
                })
            {
                let _ = fs::remove_dir_all(resolved);
            }
        }
    }
    fn id() -> String {
        uuid::Uuid::new_v4().simple().to_string()
    }
    fn seed(resources: &Resources) -> Paths {
        let paths = resources.job.as_ref().unwrap().paths.clone();
        fs::create_dir(&paths.profile()).unwrap();
        fs::write(
            paths.profile().join("synthetic-cache.txt"),
            b"synthetic profile only",
        )
        .unwrap();
        fs::write(paths.handoff(), b"synthetic encrypted handoff only").unwrap();
        paths
    }
    async fn await_cleanup(state: &AtomicU8) {
        tokio::time::timeout(Duration::from_secs(8), async {
            while state.load(Ordering::Acquire) == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn cancellation_removes_only_the_owned_handoff_and_profile_without_an_async_caller() {
        let fixture = Fixture::new();
        let resources = Resources::prepare(&fixture.0, &id()).unwrap();
        let paths = seed(&resources);
        let unrelated = fixture.0.join("unrelated.txt");
        fs::write(&unrelated, b"must remain").unwrap();
        let state = resources.state.clone();
        let (started, waiting) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _resources = resources;
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        waiting.await.unwrap();
        task.abort();
        let _ = task.await;
        await_cleanup(&state).await;
        assert_eq!(state.load(Ordering::Acquire), 1);
        assert!(!paths.profile().exists() && !paths.handoff().exists() && !paths.record().exists());
        assert_eq!(fs::read(unrelated).unwrap(), b"must remain");
    }
    #[test]
    fn recovery_skips_active_jobs_and_unmarked_or_malformed_profiles() {
        let fixture = Fixture::new();
        let mut active = Resources::prepare(&fixture.0, &id()).unwrap();
        let active_paths = seed(&active);
        let mut abandoned = Resources::prepare(&fixture.0, &id()).unwrap();
        let abandoned_paths = seed(&abandoned);
        let abandoned_job = abandoned.job.take().unwrap();
        drop(abandoned_job);
        let legacy = active_paths.profiles.join(id());
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("untouched.txt"), b"unmarked profile").unwrap();
        fs::write(
            active_paths.jobs.join(format!("{}.json", id())),
            br#"{"owner":"other-app","version":1,"id":"wrong"}"#,
        )
        .unwrap();
        let report = recover_inactive(&fixture.0).unwrap();
        assert_eq!(
            report,
            RecoveryReport {
                removed: 1,
                active: 1,
                deferred: 0
            }
        );
        assert!(active_paths.profile().exists() && active_paths.handoff().exists());
        assert!(!abandoned_paths.profile().exists() && !abandoned_paths.handoff().exists());
        assert!(legacy.join("untouched.txt").exists());
        active.job.take().unwrap().cleanup().unwrap();
    }
    #[test]
    fn child_lease_prevents_recovery_after_the_parent_was_lost() {
        let fixture = Fixture::new();
        let mut resources = Resources::prepare(&fixture.0, &id()).unwrap();
        let paths = seed(&resources);
        let child = ChildLease::acquire(&fixture.0, &paths.id).unwrap();
        drop(resources.job.take());
        assert_eq!(recover_inactive(&fixture.0).unwrap().active, 1);
        assert!(paths.profile().exists());
        drop(child);
        assert_eq!(recover_inactive(&fixture.0).unwrap().removed, 1);
        assert!(
            ChildLease::acquire(&fixture.0, &paths.id).is_none(),
            "A delayed child cannot recreate its lease"
        );
    }
    #[tokio::test]
    async fn a_failed_cleanup_retains_ownership_for_a_bounded_later_retry() {
        let fixture = Fixture::new();
        let resources = Resources::prepare(&fixture.0, &id()).unwrap();
        let paths = seed(&resources);
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(paths.profile().join("synthetic-cache.txt"))
            .unwrap();
        assert!(resources.finish().await.is_err());
        assert!(paths.record().exists() && paths.profile().exists());
        drop(held);
        assert_eq!(recover_inactive(&fixture.0).unwrap().removed, 1);
        assert!(!paths.profile().exists());
    }
    #[tokio::test]
    async fn an_entry_removed_between_enumeration_and_metadata_restarts_full_validation() {
        let fixture = Fixture::new();
        let resources = Resources::prepare(&fixture.0, &id()).unwrap();
        let paths = seed(&resources);
        let disappearing = paths.profile().join("synthetic-disappearing-entry.tmp");
        fs::write(&disappearing, b"synthetic transient entry").unwrap();
        let mut scans = 0;
        cleanup_profile_with_scan(&paths, |profile| {
            scans += 1;
            if scans == 1 {
                // read_dir has yielded this real entry, but another process
                // finishes its deletion just before the metadata query.
                let failure = check_profile_tree_with_metadata(profile, 0, &mut 20_000, &|path| {
                    if path == disappearing {
                        fs::remove_file(path).unwrap();
                    }
                    fs::symlink_metadata(path)
                })
                .err()
                .expect("the deliberately removed entry must fail metadata");
                assert_eq!(failure.stage, "profile-tree-metadata");
                assert_eq!(
                    failure.io.as_ref().unwrap().kind(),
                    std::io::ErrorKind::NotFound
                );
                assert!(failure.retryable());
                assert!(
                    profile.exists(),
                    "failed validation must not start recursive deletion"
                );
                Err(failure)
            } else {
                check_profile_tree(profile, 0, &mut 20_000)
            }
        })
        .unwrap();
        assert_eq!(scans, 2);
        assert!(!paths.profile().exists());
        for stage in [
            "profile-tree-reparse",
            "profile-tree-budget",
            "profile-tree-file-type",
        ] {
            assert!(!ProfileTreeFailure::unsafe_tree(stage).retryable());
        }
        assert!(
            !ProfileTreeFailure::io(
                "profile-tree-metadata",
                std::io::Error::from_raw_os_error(5)
            )
            .retryable()
        );
        resources.finish().await.unwrap();
        assert!(!paths.record().exists());
    }
    #[tokio::test]
    async fn atomic_handoff_temps_are_owned_precisely_and_cleanup_is_idempotent() {
        let fixture = Fixture::new();
        let resources = Resources::prepare(&fixture.0, &id()).unwrap();
        let paths = seed(&resources);
        let owned_temp = paths.handoffs.join(format!("{}.{}.tmp", paths.id, id()));
        let foreign_temp = paths.handoffs.join(format!("{}.keep.tmp", paths.id));
        fs::write(&owned_temp, b"encrypted synthetic temporary").unwrap();
        fs::write(&foreign_temp, b"not an owned atomic temporary").unwrap();
        resources.finish().await.unwrap();
        assert!(!owned_temp.exists() && foreign_temp.exists());
        assert_eq!(
            recover_inactive(&fixture.0).unwrap(),
            RecoveryReport::default()
        );
    }
    #[test]
    fn invalid_job_ids_and_unusable_owned_directories_do_not_touch_foreign_files() {
        let fixture = Fixture::new();
        fs::create_dir_all(&fixture.0).unwrap();
        let foreign = fixture.0.join("login-webview");
        fs::write(&foreign, b"unrelated file blocks the expected directory").unwrap();
        assert!(Resources::prepare(&fixture.0, "../../outside").is_err());
        assert!(Resources::prepare(&fixture.0, &id()).is_err());
        assert_eq!(
            fs::read(foreign).unwrap(),
            b"unrelated file blocks the expected directory"
        );
    }
    #[tokio::test]
    async fn a_profile_junction_cannot_redirect_recursive_cleanup() {
        let fixture = Fixture::new();
        let resources = Resources::prepare(&fixture.0, &id()).unwrap();
        let paths = resources.job.as_ref().unwrap().paths.clone();
        let external = fixture.0.join("unrelated-profile");
        fs::create_dir(&external).unwrap();
        fs::write(external.join("must-remain.txt"), b"unrelated profile").unwrap();
        // A directory junction needs no developer-mode symlink privilege. Both
        // paths are synthetic and remain inside this test's unique directory.
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(paths.profile())
            .arg(&external)
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(result.success());
        assert!(resources.finish().await.is_err());
        assert_eq!(
            fs::read(external.join("must-remain.txt")).unwrap(),
            b"unrelated profile"
        );
        assert!(paths.record().exists());
        // Remove the junction itself non-recursively, then recover the owned job.
        fs::remove_dir(paths.profile()).unwrap();
        assert_eq!(recover_inactive(&fixture.0).unwrap().removed, 1);
    }
    #[test]
    fn synthetic_login_child() {
        match std::env::var("NOTDISCORD_SYNTHETIC_LOGIN_CHILD").as_deref() {
            Ok("child") => {
                let job_id = std::env::var("NOTDISCORD_SYNTHETIC_JOB_ID").unwrap();
                process_job::join_current_process(&job_id).unwrap();
                // The grandchild inherits the Windows job automatically. It
                // neither opens a browser nor contacts a network endpoint.
                let mut grandchild = std::process::Command::new(std::env::current_exe().unwrap())
                    .arg("--exact")
                    .arg("authentication::lifecycle::tests::synthetic_login_child")
                    .env("NOTDISCORD_SYNTHETIC_LOGIN_CHILD", "grandchild")
                    .env_remove("NOTDISCORD_SYNTHETIC_JOB_ID")
                    .creation_flags(0x08000000)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap();
                let _ = grandchild.wait();
            }
            Ok("grandchild") => std::thread::sleep(Duration::from_secs(30)),
            _ => {}
        }
    }
    #[tokio::test]
    async fn cancellation_terminates_and_reaps_its_synthetic_child_tree() {
        let fixture = Fixture::new();
        let mut resources = Resources::prepare(&fixture.0, &id()).unwrap();
        let paths = seed(&resources);
        let child = tokio::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("authentication::lifecycle::tests::synthetic_login_child")
            .env("NOTDISCORD_SYNTHETIC_LOGIN_CHILD", "child")
            .env("NOTDISCORD_SYNTHETIC_JOB_ID", &paths.id)
            .kill_on_drop(true)
            .creation_flags(0x08000000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        resources.attach(child);
        assert!(resources.child_mut().try_wait().unwrap().is_none());
        tokio::time::timeout(Duration::from_secs(8), async {
            while resources
                .job
                .as_ref()
                .unwrap()
                .process_tree
                .as_ref()
                .unwrap()
                .active_for_test()
                < 2
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("Synthetic child and its descendant must join before cancellation");
        let state = resources.state.clone();
        drop(resources);
        await_cleanup(&state).await;
        assert_eq!(
            state.load(Ordering::Acquire),
            1,
            "Cleanup only succeeds after its child is reaped and its process tree is empty"
        );
        assert!(!paths.profile().exists() && !paths.record().exists());
    }
}
