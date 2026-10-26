//! Visible owners admit work once; queued and in-flight stale work can cancel
//! without invalidating another surface's images or animation.
use super::*;

#[derive(Default)]
struct Lease {
    token: u64,
    _owner: String,
    priority: u8,
}
#[derive(Default)]
pub(super) struct Demand {
    leases: Arc<Mutex<HashMap<String, Lease>>>,
    requested: HashSet<String>,
    serial: u64,
    owner: String,
    priority: u8,
}
impl Demand {
    pub(super) fn owner(&mut self, owner: &str, priority: u8) {
        self.owner.clear();
        self.owner.push_str(owner);
        self.priority = priority;
    }
    pub(super) fn begin(&mut self) {
        self.requested.clear();
    }
    pub(super) fn mark(&mut self, key: &str) -> (u64, u8) {
        self.requested.insert(key.to_owned());
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        let lease = leases.entry(key.to_owned()).or_insert_with(|| {
            self.serial = self.serial.wrapping_add(1);
            Lease {
                token: self.serial,
                _owner: self.owner.clone(),
                priority: self.priority,
            }
        });
        lease.priority = lease.priority.max(self.priority);
        (lease.token, lease.priority)
    }
    pub(super) fn observer(&self) -> Observer {
        Observer(self.leases.clone())
    }
    pub(super) fn finish(&mut self) {
        self.leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|key, _| self.requested.contains(key));
    }
    pub(super) fn clear(&mut self) {
        self.leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.requested.clear();
    }
}
#[derive(Clone)]
pub(super) struct Observer(Arc<Mutex<HashMap<String, Lease>>>);
impl Observer {
    pub(super) fn live(&self, key: &str, token: u64) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .is_some_and(|lease| lease.token == token)
    }
    pub(super) fn contains(&self, key: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(key)
    }
}

struct QueueState<T> {
    items: Vec<(u8, u64, T)>,
    closed: bool,
}
pub(super) struct Queue<T> {
    inner: Arc<(Mutex<QueueState<T>>, Condvar)>,
    serial: Arc<AtomicU64>,
}
impl<T> Clone for Queue<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            serial: self.serial.clone(),
        }
    }
}
impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(QueueState {
                    items: Vec::new(),
                    closed: false,
                }),
                Condvar::new(),
            )),
            serial: Arc::new(AtomicU64::new(0)),
        }
    }
}
impl<T> Queue<T> {
    pub(super) fn send(&self, value: T, priority: u8) -> Result<(), T> {
        let mut state = self.inner.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed || state.items.len() >= 64 {
            return Err(value);
        }
        state
            .items
            .push((priority, self.serial.fetch_add(1, Ordering::Relaxed), value));
        self.inner.1.notify_one();
        Ok(())
    }
    #[cfg(any(debug_assertions, feature = "offline-probes"))]
    pub(super) fn try_send(&self, value: T) -> Result<(), T> {
        self.send(value, 0)
    }
    pub(super) fn recv(&self) -> Option<T> {
        let mut state = self.inner.0.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some((index, _)) = state
                .items
                .iter()
                .enumerate()
                .max_by_key(|(_, (priority, serial, _))| (*priority, std::cmp::Reverse(*serial)))
            {
                return Some(state.items.swap_remove(index).2);
            }
            if state.closed {
                return None;
            }
            state = self.inner.1.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }
    pub(super) fn close(&self) {
        let mut state = self.inner.0.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        state.items.clear();
        self.inner.1.notify_all();
    }
    pub(super) fn retain(&self, mut keep: impl FnMut(&T) -> bool) {
        self.inner
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .retain(|(_, _, value)| keep(value));
    }
}
