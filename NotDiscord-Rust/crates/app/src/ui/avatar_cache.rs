//! Encoded avatar cache: private account namespace, explicit freshness and quota.
//! Paths and metadata never contain the URL, account identifier or credentials.
use reqwest::header::{AGE, CACHE_CONTROL, DATE, EXPIRES, HeaderMap, VARY};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_BODY: u64 = 25 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;
const MAX_SCAN: usize = 2048;
const MAX_TTL: u64 = 24 * 60 * 60;
const METADATA_RESERVE: u64 = 2048;
const OWNER_RESERVE: u64 = 4096;
const REPARSE_POINT: u32 = 0x400;
const OPEN_REPARSE_POINT: u32 = 0x00200000;
const ERROR: &str = "La caché local de avatares no está disponible.";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    owner: String,
    version: u32,
    namespace: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    version: u32,
    key: String,
    stored: u64,
    accessed: u64,
    expires: u64,
    bytes: u64,
    content_sha256: String,
}
pub(super) struct AvatarCache {
    base: PathBuf,
    root: PathBuf,
    directory: PathBuf,
    namespace: String,
    _lease: File,
    lock: Mutex<()>,
}
/// Keeps the exact cached file immutable while a decoder opens its filename.
pub(super) struct CachedAvatar {
    path: PathBuf,
    _file: File,
    #[cfg(test)]
    len: u64,
}
impl CachedAvatar {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    #[cfg(test)]
    pub(super) fn len(&self) -> u64 {
        self.len
    }
}
fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn plain_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| {
        m.is_dir() && !m.file_type().is_symlink() && m.file_attributes() & REPARSE_POINT == 0
    })
}
fn ensure_directory(path: &Path) -> Result<bool, String> {
    let created = match fs::create_dir(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(_) => return Err(ERROR.into()),
    };
    if !plain_directory(path) {
        return Err(ERROR.into());
    }
    Ok(created)
}
fn open_plain(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| ERROR)?;
    let m = file.metadata().map_err(|_| ERROR)?;
    if !m.is_file() || m.file_type().is_symlink() || m.file_attributes() & REPARSE_POINT != 0 {
        return Err(ERROR.into());
    }
    Ok(file)
}
fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, String> {
    let mut file = open_plain(path)?;
    if file.metadata().map_err(|_| ERROR)?.len() > 2048 {
        return Err(ERROR.into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(2049)
        .read_to_end(&mut bytes)
        .map_err(|_| ERROR)?;
    if bytes.len() > 2048 {
        return Err(ERROR.into());
    }
    serde_json::from_slice(&bytes).map_err(|_| ERROR.into())
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| ERROR)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| ERROR)?;
        file.write_all(&bytes).map_err(|_| ERROR)?;
        file.flush().map_err(|_| ERROR)?;
        drop(file);
        // This runs under both the namespace lease and per-cache mutex. Only
        // known metadata in this namespace may be replaced.
        match fs::symlink_metadata(path) {
            Ok(m) if m.is_file() && m.file_attributes() & REPARSE_POINT == 0 => {
                fs::remove_file(path).map_err(|_| ERROR)?;
            }
            Ok(_) => return Err(ERROR),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ERROR),
        }
        fs::rename(&temporary, path).map_err(|_| ERROR)
    })();
    let _ = fs::remove_file(temporary);
    result.map_err(str::to_owned)
}
fn avatar_key(value: &str, now: u64) -> Option<(String, Option<u64>)> {
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return None;
    }
    let mut url = url::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(
            url.host_str(),
            Some("cdn.discordapp.com" | "media.discordapp.net")
        )
        || !(url.path().contains("/avatars/") || url.path().starts_with("/embed/avatars/"))
    {
        return None;
    }
    let mut signed_expiry = None;
    for (key, value) in url.query_pairs() {
        if key == "ex" {
            if value.is_empty() || value.len() > 16 || !value.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return None;
            }
            let expiry = u64::from_str_radix(&value, 16).ok()?;
            if expiry <= now {
                return None;
            }
            signed_expiry = Some(signed_expiry.map_or(expiry, |old: u64| old.min(expiry)));
        }
    }
    url.set_fragment(None);
    Some((digest(url.as_str().as_bytes()), signed_expiry))
}
fn http_time(headers: &HeaderMap, name: reqwest::header::HeaderName) -> Option<u64> {
    let value = headers.get(name)?.to_str().ok()?;
    let time = chrono::DateTime::parse_from_rfc2822(value)
        .ok()?
        .timestamp();
    u64::try_from(time).ok()
}
/// No freshness metadata means no persistent cache. Expired/no-cache entries
/// are fetched again, never served as an implicit stale-on-error response.
pub(super) fn expiry(url: &str, headers: &HeaderMap, now: u64) -> Option<u64> {
    let (_, signature) = avatar_key(url, now)?;
    if headers.get_all(VARY).iter().any(|v| {
        v.to_str()
            .map_or(true, |v| v.split(',').any(|name| name.trim() == "*"))
    }) {
        return None;
    }
    let mut max_age = None;
    for value in headers.get_all(CACHE_CONTROL) {
        for directive in value.to_str().ok()?.split(',') {
            let (name, argument) = directive
                .trim()
                .split_once('=')
                .unwrap_or((directive.trim(), ""));
            if ["no-store", "no-cache", "private"]
                .iter()
                .any(|key| name.eq_ignore_ascii_case(key))
            {
                return None;
            }
            if name.eq_ignore_ascii_case("max-age") {
                let seconds = argument.trim().trim_matches('"').parse::<u64>().ok()?;
                max_age = Some(max_age.map_or(seconds, |old: u64| old.min(seconds)));
            }
        }
    }
    let received_age = match headers.get(AGE) {
        Some(value) => value.to_str().ok()?.parse::<u64>().ok()?,
        None => 0,
    };
    let age = received_age.max(http_time(headers, DATE).map_or(0, |date| now.saturating_sub(date)));
    let ttl = if let Some(seconds) = max_age {
        seconds.saturating_sub(age)
    } else {
        let expires = http_time(headers, EXPIRES)?;
        expires
            .saturating_sub(http_time(headers, DATE).unwrap_or(now))
            .saturating_sub(age)
    };
    let mut until = now.checked_add(ttl.min(MAX_TTL))?;
    if let Some(signature) = signature {
        until = until.min(signature);
    }
    (until > now).then_some(until)
}
impl AvatarCache {
    /// `base` is an existing, explicitly selected cache root. No migration or
    /// scanning of a browser/account data directory is performed here.
    pub(super) fn open(base: &Path, account: &str) -> Result<Self, String> {
        if account.is_empty() || account.len() > 128 || !plain_directory(base) {
            return Err(ERROR.into());
        }
        let base = fs::canonicalize(base).map_err(|_| ERROR)?;
        let root = base.join("avatars-v1");
        ensure_directory(&root)?;
        let namespace = digest(account.as_bytes());
        let directory = root.join(&namespace);
        let created = ensure_directory(&directory)?;
        let owner_path = directory.join("owner.json");
        if created {
            let owner = Owner {
                owner: "NotDiscord.AvatarCache".into(),
                version: 1,
                namespace: namespace.clone(),
            };
            atomic_json(&owner_path, &owner)?;
        }
        let owner: Owner = read_json(&owner_path)?;
        if owner.owner != "NotDiscord.AvatarCache"
            || owner.version != 1
            || owner.namespace != namespace
        {
            return Err(ERROR.into());
        }
        let lease_path = directory.join("cache.lock");
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(OPEN_REPARSE_POINT)
            .open(&lease_path)
            .map_err(|_| ERROR)?;
        if lease.metadata().map_err(|_| ERROR)?.file_attributes() & REPARSE_POINT != 0 {
            return Err(ERROR.into());
        }
        let cache = Self {
            base,
            root,
            directory,
            namespace,
            _lease: lease,
            lock: Mutex::new(()),
        };
        cache.valid_root()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ERROR)?
            .as_secs();
        cache.entries(now)?;
        Ok(cache)
    }
    fn valid_root(&self) -> Result<(), String> {
        for path in [&self.base, &self.root, &self.directory] {
            if !plain_directory(path) || fs::canonicalize(path).map_err(|_| ERROR)? != *path {
                return Err(ERROR.into());
            }
        }
        if self.root.parent() != Some(self.base.as_path())
            || self.directory.parent() != Some(self.root.as_path())
        {
            return Err(ERROR.into());
        }
        let owner: Owner = read_json(&self.directory.join("owner.json"))?;
        if owner.owner != "NotDiscord.AvatarCache"
            || owner.version != 1
            || owner.namespace != self.namespace
        {
            return Err(ERROR.into());
        }
        Ok(())
    }
    fn body(&self, key: &str) -> PathBuf {
        self.directory.join(format!("{key}.bin"))
    }
    fn metadata(&self, key: &str) -> PathBuf {
        self.directory.join(format!("{key}.json"))
    }
    fn remove(&self, key: &str) -> Result<(), String> {
        if !valid_digest(key) {
            return Err(ERROR.into());
        }
        for path in [self.body(key), self.metadata(key)] {
            match fs::symlink_metadata(&path) {
                Ok(m) if m.is_file() && m.file_attributes() & REPARSE_POINT == 0 => {
                    fs::remove_file(path).map_err(|_| ERROR)?;
                }
                Ok(_) => return Err(ERROR.into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(ERROR.into()),
            }
        }
        Ok(())
    }
    pub(super) fn invalidate(&self, url: &str, now: u64) {
        let Ok(_lock) = self.lock.lock() else { return };
        if self.valid_root().is_ok()
            && let Some((key, _)) = avatar_key(url, now)
        {
            let _ = self.remove(&key);
        }
    }
    pub(super) fn lookup(&self, url: &str, now: u64) -> Option<CachedAvatar> {
        let _lock = self.lock.lock().ok()?;
        self.valid_root().ok()?;
        let (key, signature) = avatar_key(url, now)?;
        let result = (|| {
            let mut entry: Entry = read_json(&self.metadata(&key)).ok()?;
            if entry.version != 1
                || entry.key != key
                || entry.stored > now
                || entry.expires <= now
                || entry.expires > entry.stored.saturating_add(MAX_TTL)
                || entry.bytes == 0
                || entry.bytes > MAX_BODY
                || !valid_digest(&entry.content_sha256)
                || signature.is_some_and(|until| until < entry.expires)
            {
                return None;
            }
            let mut file = open_plain(&self.body(&key)).ok()?;
            if file.metadata().ok()?.len() != entry.bytes {
                return None;
            }
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 32 * 1024];
            loop {
                let n = file.read(&mut buffer).ok()?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
            }
            if format!("{:x}", hash.finalize()) != entry.content_sha256 {
                return None;
            }
            if entry.accessed != now {
                entry.accessed = now;
                let _ = atomic_json(&self.metadata(&key), &entry);
            }
            Some(CachedAvatar {
                path: self.body(&key),
                _file: file,
                #[cfg(test)]
                len: entry.bytes,
            })
        })();
        if result.is_none() {
            let _ = self.remove(&key);
        }
        result
    }
    fn entries(&self, now: u64) -> Result<Vec<Entry>, String> {
        let mut entries = Vec::new();
        for (index, item) in fs::read_dir(&self.directory)
            .map_err(|_| ERROR)?
            .enumerate()
        {
            if index >= MAX_SCAN {
                return Err(ERROR.into());
            }
            let item = item.map_err(|_| ERROR)?;
            let name = item.file_name();
            if let Some(name) = name.to_str() {
                let stem = name.strip_suffix(".tmp");
                let temporary = stem.is_some_and(|stem| {
                    (stem.len() == 32 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
                        || stem.rsplit_once('.').is_some_and(|(prefix, random)| {
                            (prefix == "owner" || valid_digest(prefix))
                                && random.len() == 32
                                && random.bytes().all(|b| b.is_ascii_hexdigit())
                        })
                });
                if temporary {
                    let m = fs::symlink_metadata(item.path()).map_err(|_| ERROR)?;
                    if !m.is_file() || m.file_attributes() & REPARSE_POINT != 0 {
                        return Err(ERROR.into());
                    }
                    fs::remove_file(item.path()).map_err(|_| ERROR)?;
                    continue;
                }
                if let Some(key) = name.strip_suffix(".json").filter(|key| valid_digest(key))
                    && matches!(fs::symlink_metadata(self.body(key)), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
                {
                    self.remove(key)?;
                    continue;
                }
            }
            let Some(key) = name
                .to_str()
                .and_then(|s| s.strip_suffix(".bin"))
                .filter(|s| valid_digest(s))
            else {
                continue;
            };
            let m = fs::symlink_metadata(item.path()).map_err(|_| ERROR)?;
            if !m.is_file() || m.file_attributes() & REPARSE_POINT != 0 {
                return Err(ERROR.into());
            }
            match read_json::<Entry>(&self.metadata(key)) {
                Ok(entry)
                    if entry.version == 1
                        && entry.key == key
                        && entry.bytes == m.len()
                        && entry.bytes > 0
                        && entry.bytes <= MAX_BODY
                        && entry.stored <= now
                        && entry.expires > now
                        && entry.expires <= entry.stored.saturating_add(MAX_TTL)
                        && valid_digest(&entry.content_sha256) =>
                {
                    entries.push(entry)
                }
                _ => self.remove(key)?,
            }
        }
        Ok(entries)
    }
    /// Cache insertion is optional. The caller stores only a successfully
    /// decoded avatar and falls back to its fresh spool when caching fails.
    pub(super) fn store(
        &self,
        url: &str,
        headers: &HeaderMap,
        source: &Path,
        now: u64,
    ) -> Result<bool, String> {
        let Some(until) = expiry(url, headers, now) else {
            return Ok(false);
        };
        let Some((key, _)) = avatar_key(url, now) else {
            return Ok(false);
        };
        let _lock = self.lock.lock().map_err(|_| ERROR)?;
        self.valid_root()?;
        let mut input = open_plain(source)?;
        let len = input.metadata().map_err(|_| ERROR)?.len();
        if len == 0 || len > MAX_BODY || len > MAX_BYTES {
            return Ok(false);
        }
        let mut entries = self.entries(now)?;
        entries.sort_by_key(|e| (e.accessed, e.stored));
        let mut bytes = entries
            .iter()
            .map(|e| e.bytes + METADATA_RESERVE)
            .sum::<u64>();
        let mut count = entries.len();
        if let Some(previous) = entries.iter().find(|e| e.key == key) {
            bytes = bytes.saturating_sub(previous.bytes + METADATA_RESERVE);
            count -= 1;
        }
        for entry in entries.iter().filter(|entry| entry.key != key) {
            if bytes.saturating_add(len + METADATA_RESERVE + OWNER_RESERVE) <= MAX_BYTES
                && count < MAX_ENTRIES
            {
                break;
            }
            self.remove(&entry.key)?;
            bytes = bytes.saturating_sub(entry.bytes + METADATA_RESERVE);
            count -= 1;
        }
        if bytes.saturating_add(len + METADATA_RESERVE + OWNER_RESERVE) > MAX_BYTES
            || count >= MAX_ENTRIES
        {
            return Ok(false);
        }
        let temporary = self
            .directory
            .join(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        let result = (|| {
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| ERROR)?;
            let mut hash = Sha256::new();
            let mut copied = 0u64;
            let mut buffer = [0u8; 32 * 1024];
            loop {
                let n = input.read(&mut buffer).map_err(|_| ERROR)?;
                if n == 0 {
                    break;
                }
                copied = copied.saturating_add(n as u64);
                if copied > len {
                    return Err(ERROR.to_owned());
                }
                output.write_all(&buffer[..n]).map_err(|_| ERROR)?;
                hash.update(&buffer[..n]);
            }
            if copied != len {
                return Err(ERROR.to_owned());
            }
            output.flush().map_err(|_| ERROR)?;
            drop(output);
            self.remove(&key)?;
            fs::rename(&temporary, self.body(&key)).map_err(|_| ERROR)?;
            let entry = Entry {
                version: 1,
                key: key.clone(),
                stored: now,
                accessed: now,
                expires: until,
                bytes: len,
                content_sha256: format!("{:x}", hash.finalize()),
            };
            if let Err(error) = atomic_json(&self.metadata(&key), &entry) {
                let _ = self.remove(&key);
                return Err(error);
            }
            Ok(true)
        })();
        let _ = fs::remove_file(temporary);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const URL: &str = "https://cdn.discordapp.com/avatars/42/hash.png?size=256";
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "notdiscord-avatar-cache-test-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            // A new test-owned UUID tree; reject redirection before recursing.
            fn remove(path: &Path) {
                let m = fs::symlink_metadata(path).unwrap();
                if m.file_attributes() & REPARSE_POINT != 0 {
                    return;
                }
                if m.is_dir() {
                    for item in fs::read_dir(path).unwrap().flatten() {
                        remove(&item.path());
                    }
                    let _ = fs::remove_dir(path);
                } else {
                    let _ = fs::remove_file(path);
                }
            }
            remove(&self.0);
        }
    }
    fn headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(CACHE_CONTROL, "public, max-age=3600".parse().unwrap());
        h
    }
    #[test]
    fn freshness_respects_signed_urls_age_and_explicit_no_store_without_logging_urls() {
        let h = headers();
        assert_eq!(expiry(URL, &h, 100), Some(3700));
        assert_eq!(
            expiry(&format!("{URL}&ex={:x}&hm=private", 200), &h, 100),
            Some(200)
        );
        assert_eq!(expiry(&format!("{URL}&ex={:x}", 99), &h, 100), None);
        assert_eq!(expiry(&format!("{URL}&ex=invalid"), &h, 100), None);
        let mut h = h;
        h.insert(AGE, "3590".parse().unwrap());
        assert_eq!(expiry(URL, &h, 100), Some(110));
        for directive in [
            "no-store",
            "public, no-cache",
            "private, max-age=10",
            "max-age=0",
        ] {
            h.insert(CACHE_CONTROL, directive.parse().unwrap());
            assert_eq!(expiry(URL, &h, 100), None);
        }
        assert_eq!(expiry(URL, &HeaderMap::new(), 100), None);
        assert_ne!(
            avatar_key(&format!("{URL}&ex=ff&hm=a"), 100).unwrap().0,
            avatar_key(&format!("{URL}&ex=ff&hm=b"), 100).unwrap().0
        );
        let mut old = HeaderMap::new();
        old.insert(DATE, "Thu, 01 Jan 1970 01:46:40 GMT".parse().unwrap()); // 6400
        old.insert(EXPIRES, "Thu, 01 Jan 1970 02:16:40 GMT".parse().unwrap()); // 8200
        assert_eq!(expiry(URL, &old, 10_000), None);
        old.insert(CACHE_CONTROL, "max-age=1800".parse().unwrap());
        assert_eq!(expiry(URL, &old, 10_000), None);
    }
    #[test]
    fn cache_is_account_scoped_fresh_integrity_checked_and_never_adopts_unmarked_resources() {
        let root = Directory::new();
        let source = root.0.join("source.png");
        fs::write(&source, b"synthetic encoded payload").unwrap();
        let first = AvatarCache::open(&root.0, "account-one").unwrap();
        let second = AvatarCache::open(&root.0, "account-two").unwrap();
        assert!(AvatarCache::open(&root.0, "account-one").is_err());
        assert!(first.store(URL, &headers(), &source, 100).unwrap());
        let file = first.lookup(URL, 101).unwrap();
        assert_eq!(file.len(), 25);
        assert_eq!(fs::read(file.path()).unwrap(), b"synthetic encoded payload");
        assert!(second.lookup(URL, 101).is_none());
        assert!(OpenOptions::new().write(true).open(file.path()).is_err());
        let path = file.path().to_owned();
        drop(file);
        fs::write(&path, b"corrupt").unwrap();
        assert!(first.lookup(URL, 102).is_none());
        assert!(!path.exists());
        assert!(first.store(URL, &headers(), &source, 100).unwrap());
        assert!(first.lookup(URL, 3700).is_none());
        let foreign = root.0.join("avatars-v1").join(digest(b"unmarked"));
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("keep.txt"), b"keep").unwrap();
        assert!(AvatarCache::open(&root.0, "unmarked").is_err());
        assert_eq!(fs::read(foreign.join("keep.txt")).unwrap(), b"keep");
    }
    #[test]
    fn entry_quota_evicts_lru_and_unknown_neighbor_files_survive() {
        let root = Directory::new();
        let source = root.0.join("source.png");
        fs::write(&source, b"tiny").unwrap();
        let cache = AvatarCache::open(&root.0, "quota").unwrap();
        let keep = cache.directory.join("keep.txt");
        fs::write(&keep, b"foreign").unwrap();
        for i in 0..MAX_ENTRIES {
            let url = format!("https://cdn.discordapp.com/avatars/42/{i}.png");
            assert!(
                cache
                    .store(&url, &headers(), &source, 100 + i as u64)
                    .unwrap()
            );
        }
        let first = "https://cdn.discordapp.com/avatars/42/0.png";
        assert!(cache.lookup(first, 700).is_some());
        assert!(cache.store(URL, &headers(), &source, 701).unwrap());
        assert!(cache.lookup(first, 702).is_some());
        assert!(
            cache
                .lookup("https://cdn.discordapp.com/avatars/42/1.png", 702)
                .is_none()
        );
        assert_eq!(cache.entries(702).unwrap().len(), MAX_ENTRIES);
        assert_eq!(fs::read(keep).unwrap(), b"foreign");
    }
    #[test]
    fn byte_quota_and_owned_temporary_recovery_do_not_touch_unknown_neighbors() {
        let root = Directory::new();
        let source = root.0.join("large.png");
        File::create(&source).unwrap().set_len(MAX_BODY).unwrap();
        let cache = AvatarCache::open(&root.0, "bytes").unwrap();
        let orphan = cache
            .directory
            .join(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        fs::write(&orphan, b"abandoned").unwrap();
        let foreign = cache.directory.join("leave.tmp");
        fs::write(&foreign, b"keep").unwrap();
        assert!(cache.store(URL, &headers(), &source, 100).unwrap());
        assert!(!orphan.exists());
        let other = "https://cdn.discordapp.com/avatars/42/other.png";
        assert!(cache.store(other, &headers(), &source, 101).unwrap());
        assert!(cache.lookup(URL, 102).is_none());
        assert!(cache.lookup(other, 102).is_some());
        let owned = cache.entries(102).unwrap();
        assert!(
            owned
                .iter()
                .map(|e| e.bytes + METADATA_RESERVE)
                .sum::<u64>()
                + OWNER_RESERVE
                <= MAX_BYTES
        );
        assert_eq!(fs::read(foreign).unwrap(), b"keep");
    }
    #[test]
    fn a_parent_junction_cannot_redirect_an_existing_cache_into_another_owned_tree() {
        let root = Directory::new();
        let source = root.0.join("source.png");
        fs::write(&source, b"keep").unwrap();
        let mut cache = AvatarCache::open(&root.0, "junction").unwrap();
        // Release only the test's namespace lock so the fixture can emulate a
        // directory replacement; production never relinquishes this lock.
        cache._lease = File::open(&source).unwrap();
        let retired = root.0.join("retired");
        fs::rename(&cache.root, &retired).unwrap();
        assert!(cache.root.starts_with(fs::canonicalize(&root.0).unwrap()));
        assert!(
            fs::canonicalize(&retired)
                .unwrap()
                .starts_with(fs::canonicalize(&root.0).unwrap())
        );
        let command = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&cache.root)
            .arg(&retired)
            .output()
            .unwrap();
        assert!(command.status.success());
        assert!(cache.valid_root().is_err());
        assert!(cache.lookup(URL, 100).is_none());
        assert!(cache.store(URL, &headers(), &source, 100).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"keep");
        fs::remove_dir(&cache.root).unwrap(); // remove the junction itself
        fs::rename(retired, &cache.root).unwrap();
    }
}
