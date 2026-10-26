//! Minimal local stop markers. No credentials, challenge payloads or request bodies.
//! Verification is account-scoped and is not cleared by logout or a new token.
use crate::storage;
use notdiscord_core::{
    parsing,
    safety::{SafetyGate, SafetyState},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_BYTES: u64 = 64 * 1024;
const MAX_ACCOUNTS: usize = 64;
const MAX_CREDENTIALS: usize = 128;
const STORE_ERROR: &str = "No se pudo conservar el registro local de seguridad. Las operaciones de esta sesión siguen suspendidas; utiliza Discord oficial.";

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Markers {
    version: u8,
    verification_accounts: BTreeSet<String>,
    credentials: BTreeMap<String, CredentialMarker>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialMarker {
    account: Option<String>,
    state: SafetyState,
}

pub(crate) struct SafetyStore {
    // Kernel sharing restrictions serialize independent processes and path
    // aliases, not merely Arc clones of this in-memory registry.
    _lease: Option<File>,
    path: PathBuf,
    markers: Mutex<Markers>,
    unavailable: bool,
    dirty: AtomicBool,
}

pub(crate) fn credential_key(credential: &str) -> String {
    // This is an opaque association only; never serialize the original credential.
    format!("{:x}", Sha256::digest(credential.as_bytes()))
}

pub(crate) fn message(state: SafetyState) -> &'static str {
    match state {
        SafetyState::Allowed => "",
        SafetyState::VerificationRequired => {
            "Es necesario revisar la seguridad de esta sesión. Las operaciones están suspendidas; continúa en Discord oficial."
        }
        SafetyState::AuthenticationRequired => {
            "La sesión fue rechazada. Las operaciones están suspendidas; inicia una nueva sesión válida."
        }
        SafetyState::ProtocolUnsupported => {
            "Discord rechazó el protocolo de conexión. Las operaciones están suspendidas; comprueba si hay una actualización del cliente."
        }
        SafetyState::SynchronizationRequired => {
            "La conexión perdió la sincronización. Las operaciones están suspendidas; vuelve a conectar para actualizar el estado."
        }
    }
}

impl SafetyStore {
    pub(crate) fn load(directory: &Path) -> Self {
        let path = directory.join("network-safety.dpapi");
        let lease = std::fs::create_dir_all(directory).ok().and_then(|_| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .share_mode(0)
                .open(directory.join("network-safety.lock"))
                .ok()
        });
        let loaded = (|| {
            if lease.is_none() {
                return Err(());
            }
            // A write-ahead flag contains no identity or secret. If encryption,
            // replacement or shutdown failed mid-write, the next start cannot
            // mistake the previous registry for a successfully saved stop.
            match std::fs::metadata(directory.join("network-safety.pending")) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(()),
            }
            match std::fs::metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Markers {
                        version: 1,
                        ..Default::default()
                    });
                }
                Err(_) => return Err(()),
                Ok(metadata) if !metadata.is_file() || metadata.len() > MAX_BYTES => return Err(()),
                Ok(_) => {}
            }
            let bytes = storage::read_bounded(&path, MAX_BYTES).ok_or(())?;
            let plain =
                zeroize::Zeroizing::new(storage::protect(&bytes, true, false).map_err(|_| ())?);
            let markers: Markers = serde_json::from_slice(&plain).map_err(|_| ())?;
            if markers.version != 1
                || markers.verification_accounts.len() > MAX_ACCOUNTS
                || markers.credentials.len() > MAX_CREDENTIALS
                || markers
                    .verification_accounts
                    .iter()
                    .any(|id| !parsing::valid_id(id))
                || markers.credentials.iter().any(|(key, marker)| {
                    !valid_key(key)
                        || marker
                            .account
                            .as_ref()
                            .is_some_and(|id| !parsing::valid_id(id))
                        || !matches!(
                            marker.state,
                            SafetyState::Allowed
                                | SafetyState::VerificationRequired
                                | SafetyState::AuthenticationRequired
                        )
                        || marker.state == SafetyState::Allowed && marker.account.is_none()
                })
            {
                return Err(());
            }
            Ok(markers)
        })();
        let unavailable = loaded.is_err();
        Self {
            _lease: lease,
            path,
            markers: Mutex::new(loaded.unwrap_or_default()),
            unavailable,
            dirty: AtomicBool::new(false),
        }
    }

    pub(crate) fn initial_state(&self, key: &str) -> SafetyState {
        if self.unavailable || !valid_key(key) {
            return SafetyState::AuthenticationRequired;
        }
        let markers = self.markers.lock().unwrap_or_else(|e| e.into_inner());
        markers
            .credentials
            .get(key)
            .map(|marker| {
                if marker
                    .account
                    .as_ref()
                    .is_some_and(|account| markers.verification_accounts.contains(account))
                {
                    SafetyState::VerificationRequired
                } else {
                    marker.state
                }
            })
            .unwrap_or_default()
    }

    pub(crate) fn unavailable_message(&self) -> Option<&'static str> {
        self.unavailable.then_some("No se pudo abrir el registro local de seguridad; puede estar en uso por otra ventana. No se enviarán peticiones autenticadas; utiliza Discord oficial o restaura ese registro local.")
    }

    pub(crate) fn account_for_key(&self, key: &str) -> Option<String> {
        self.markers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .credentials
            .get(key)
            .and_then(|marker| marker.account.clone())
    }

    /// Called immediately after identity validation, before loading settings or
    /// opening Gateway. A changed token cannot bypass an account verification.
    pub(crate) fn inspect_identity(
        &self,
        key: &str,
        account: &str,
        gate: &SafetyGate,
    ) -> Result<(), String> {
        gate.block(self.initial_state(key));
        if !valid_key(key) || !parsing::valid_id(account) || self.unavailable {
            gate.block(SafetyState::AuthenticationRequired);
            return Err(self
                .unavailable_message()
                .unwrap_or("Discord no confirmó la identidad de esta sesión.")
                .into());
        }
        if self
            .account_for_key(key)
            .is_some_and(|known| known != account)
        {
            gate.block(SafetyState::AuthenticationRequired);
            return Err(
                "La identidad de esta sesión cambió; las operaciones están suspendidas.".into(),
            );
        }
        let blocked = self
            .markers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .verification_accounts
            .contains(account);
        if blocked {
            gate.block(SafetyState::VerificationRequired);
            // Bind the rotated credential so the next startup can stop before
            // even the identity request. Failure never unblocks the live gate.
            self.record(key, Some(account), SafetyState::VerificationRequired)?;
        } else if let Err(error) = self.record(key, Some(account), gate.state()) {
            // An identity binding is needed to retain a later stop by account.
            // Never continue a session whose safety record cannot be protected.
            gate.block(SafetyState::AuthenticationRequired);
            return Err(error);
        }
        gate.check().map_err(|error| error.to_string())
    }

    pub(crate) fn record(
        &self,
        key: &str,
        account: Option<&str>,
        state: SafetyState,
    ) -> Result<(), String> {
        if !matches!(
            state,
            SafetyState::Allowed
                | SafetyState::VerificationRequired
                | SafetyState::AuthenticationRequired
        ) {
            return Ok(());
        }
        if self.unavailable || !valid_key(key) || account.is_some_and(|id| !parsing::valid_id(id)) {
            return Err(STORE_ERROR.into());
        }
        let mut markers = self.markers.lock().unwrap_or_else(|e| e.into_inner());
        let account = account.map(str::to_owned).or_else(|| {
            markers
                .credentials
                .get(key)
                .and_then(|marker| marker.account.clone())
        });
        if state == SafetyState::Allowed && account.is_none() {
            return Err(STORE_ERROR.into());
        }
        if !markers.credentials.contains_key(key) && markers.credentials.len() >= MAX_CREDENTIALS {
            return Err(STORE_ERROR.into());
        }
        if state == SafetyState::VerificationRequired {
            if let Some(account) = &account {
                if !markers.verification_accounts.contains(account)
                    && markers.verification_accounts.len() >= MAX_ACCOUNTS
                {
                    return Err(STORE_ERROR.into());
                }
                markers.verification_accounts.insert(account.clone());
            }
        }
        // Identity confirmation cannot clear a stop, nor can an auth-only error
        // replace a known verification. No challenge reset is exposed here.
        let previous = markers.credentials.get(key);
        let state = previous
            .map(|marker| match (marker.state, state) {
                (SafetyState::VerificationRequired, _) => SafetyState::VerificationRequired,
                (SafetyState::AuthenticationRequired, SafetyState::Allowed) => {
                    SafetyState::AuthenticationRequired
                }
                _ => state,
            })
            .unwrap_or(state);
        let replacement = CredentialMarker { account, state };
        if previous == Some(&replacement) && !self.dirty.load(Ordering::Relaxed) {
            return Ok(());
        }
        markers.credentials.insert(key.into(), replacement);
        self.dirty.store(true, Ordering::Relaxed);
        markers.version = 1;
        let plain =
            zeroize::Zeroizing::new(serde_json::to_vec(&*markers).map_err(|_| STORE_ERROR)?);
        if plain.len() as u64 > MAX_BYTES {
            return Err(STORE_ERROR.into());
        }
        let pending = self.path.with_file_name("network-safety.pending");
        storage::atomic_write(&pending, b"1").map_err(|_| STORE_ERROR.to_owned())?;
        let encrypted = storage::protect(&plain, true, true).map_err(|_| STORE_ERROR)?;
        storage::atomic_write(&self.path, &encrypted).map_err(|_| STORE_ERROR.to_owned())?;
        std::fs::remove_file(&pending).map_err(|_| STORE_ERROR.to_owned())?;
        self.dirty.store(false, Ordering::Relaxed);
        Ok(())
    }
}

fn valid_key(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "notdiscord-safety-fixture-{}",
                uuid::Uuid::new_v4()
            )))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn verification_survives_restart_token_rotation_and_keeps_other_accounts_separate() {
        let fixture = Fixture::new();
        let original = credential_key("synthetic-original-credential-not-real");
        let rotated = credential_key("synthetic-rotated-credential-not-real");
        let other = credential_key("synthetic-other-account-not-real");
        let store = SafetyStore::load(&fixture.0);
        store
            .record(&original, Some("42"), SafetyState::VerificationRequired)
            .unwrap();
        drop(store);
        let store = SafetyStore::load(&fixture.0);
        assert_eq!(
            store.initial_state(&original),
            SafetyState::VerificationRequired
        );
        let gate = SafetyGate::default();
        assert!(store.inspect_identity(&rotated, "42", &gate).is_err());
        assert_eq!(gate.state(), SafetyState::VerificationRequired);
        let other_gate = SafetyGate::default();
        assert!(store.inspect_identity(&other, "43", &other_gate).is_ok());
        assert_eq!(other_gate.state(), SafetyState::Allowed);
        drop(store);
        assert_eq!(
            SafetyStore::load(&fixture.0).initial_state(&rotated),
            SafetyState::VerificationRequired
        );
        let disk = std::fs::read(fixture.0.join("network-safety.dpapi")).unwrap();
        assert!(
            !disk
                .windows(original.len())
                .any(|bytes| bytes == original.as_bytes())
        );
        assert!(!disk.windows(10).any(|bytes| bytes == b"synthetic-"));
    }

    #[test]
    fn invalid_credential_marker_does_not_block_a_new_valid_credential_or_downgrade_verification() {
        let fixture = Fixture::new();
        let invalid = credential_key("synthetic-invalid-credential-not-real");
        let fresh = credential_key("synthetic-valid-credential-not-real");
        let store = SafetyStore::load(&fixture.0);
        store
            .record(&invalid, Some("42"), SafetyState::AuthenticationRequired)
            .unwrap();
        assert_eq!(
            store.initial_state(&invalid),
            SafetyState::AuthenticationRequired
        );
        assert_eq!(store.initial_state(&fresh), SafetyState::Allowed);
        assert!(
            store
                .inspect_identity(&fresh, "42", &SafetyGate::default())
                .is_ok()
        );
        store
            .record(&invalid, Some("42"), SafetyState::VerificationRequired)
            .unwrap();
        store
            .record(&invalid, Some("42"), SafetyState::AuthenticationRequired)
            .unwrap();
        assert_eq!(
            store.initial_state(&invalid),
            SafetyState::VerificationRequired
        );
    }

    #[test]
    fn validated_binding_attributes_a_later_early_challenge_to_the_account() {
        let fixture = Fixture::new();
        let known = credential_key("synthetic-known-credential-not-real");
        let alternate = credential_key("synthetic-alternate-credential-not-real");
        let store = SafetyStore::load(&fixture.0);
        assert!(
            store
                .inspect_identity(&known, "42", &SafetyGate::default())
                .is_ok()
        );
        assert!(
            store
                .inspect_identity(&alternate, "42", &SafetyGate::default())
                .is_ok()
        );
        drop(store);
        let store = SafetyStore::load(&fixture.0);
        assert_eq!(store.account_for_key(&known).as_deref(), Some("42"));
        assert_eq!(store.initial_state(&known), SafetyState::Allowed);
        store
            .record(&known, None, SafetyState::VerificationRequired)
            .unwrap();
        assert_eq!(
            store.initial_state(&alternate),
            SafetyState::VerificationRequired
        );
        drop(store);
        assert_eq!(
            SafetyStore::load(&fixture.0).initial_state(&alternate),
            SafetyState::VerificationRequired
        );
    }

    #[test]
    fn unreadable_or_corrupt_markers_stop_before_network_and_write_failure_is_reported() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(&fixture.0).unwrap();
        std::fs::write(
            fixture.0.join("network-safety.dpapi"),
            b"corrupt synthetic data",
        )
        .unwrap();
        let store = SafetyStore::load(&fixture.0);
        let key = credential_key("synthetic-credential-not-real");
        assert_eq!(
            store.initial_state(&key),
            SafetyState::AuthenticationRequired
        );
        assert!(store.unavailable_message().is_some());
        assert!(
            store
                .inspect_identity(&key, "42", &SafetyGate::default())
                .is_err()
        );
        let missing = Fixture::new();
        let store = SafetyStore::load(&missing.0);
        // A directory at the final encrypted-file path rejects replacement.
        std::fs::create_dir(missing.0.join("network-safety.dpapi")).unwrap();
        assert!(
            store
                .record(&key, Some("42"), SafetyState::VerificationRequired)
                .is_err()
        );
        std::fs::remove_dir(missing.0.join("network-safety.dpapi")).unwrap();
        // Retry the same record after a transient storage failure; the in-memory
        // marker must not make a failed write look durable.
        store
            .record(&key, Some("42"), SafetyState::VerificationRequired)
            .unwrap();
        drop(store);
        assert_eq!(
            SafetyStore::load(&missing.0).initial_state(&key),
            SafetyState::VerificationRequired
        );
    }

    #[test]
    fn an_interrupted_safety_write_stops_before_any_identity_request() {
        let fixture = Fixture::new();
        let key = credential_key("synthetic-known-credential-not-real");
        let store = SafetyStore::load(&fixture.0);
        store
            .inspect_identity(&key, "42", &SafetyGate::default())
            .unwrap();
        assert!(!fixture.0.join("network-safety.pending").exists());
        std::fs::write(fixture.0.join("network-safety.pending"), b"1").unwrap();
        drop(store);
        let restarted = SafetyStore::load(&fixture.0);
        assert_eq!(
            restarted.initial_state(&key),
            SafetyState::AuthenticationRequired
        );
        assert!(restarted.unavailable_message().is_some());
    }

    #[test]
    fn a_second_registry_cannot_overwrite_a_stop_from_an_earlier_snapshot() {
        let fixture = Fixture::new();
        let key = credential_key("synthetic-credential-not-real");
        let first = SafetyStore::load(&fixture.0);
        let second = SafetyStore::load(&fixture.0);
        assert_eq!(
            second.initial_state(&key),
            SafetyState::AuthenticationRequired
        );
        assert!(second.unavailable_message().is_some());
        first
            .record(&key, Some("42"), SafetyState::VerificationRequired)
            .unwrap();
        assert!(
            second
                .record(&key, Some("42"), SafetyState::Allowed)
                .is_err()
        );
        drop(first);
        drop(second);
        assert_eq!(
            SafetyStore::load(&fixture.0).initial_state(&key),
            SafetyState::VerificationRequired
        );
    }
}
