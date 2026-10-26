//! Streaming encoded images and admission held until the UI drops the result.
//! No account, network request or decoder is created by this module.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

pub(super) const MAX_ENCODED: u64 = super::super::media_limit::MAX_BYTES;
const REPARSE_POINT: u32 = 0x400;
const COPY_BUFFER: usize = 32 * 1024;

/// A new UUID file created by this operation, never a path supplied by content.
/// Its read handle prevents replacement while a filename-based decoder runs.
pub(super) struct EncodedSpool {
    path: PathBuf,
    handle: Option<File>,
    len: u64,
}
impl EncodedSpool {
    #[cfg(test)]
    pub(super) fn copy_from(
        source: impl Read,
        advertised: Option<u64>,
        limit: u64,
        directory: &Path,
    ) -> Result<Self, String> {
        Self::copy_cancellable(source, advertised, limit, directory, || false)
    }
    pub(super) fn copy_cancellable(
        mut source: impl Read,
        advertised: Option<u64>,
        limit: u64,
        directory: &Path,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self, String> {
        if limit == 0 || limit > MAX_ENCODED || advertised.is_some_and(|n| n > limit) {
            return Err(super::super::media_limit::error(limit));
        }
        let metadata = fs::symlink_metadata(directory)
            .map_err(|_| "No se pudo preparar el archivo temporal de la imagen.")?;
        if !metadata.is_dir() || metadata.file_attributes() & REPARSE_POINT != 0 {
            return Err("No se pudo preparar el archivo temporal de la imagen.".into());
        }
        let directory = fs::canonicalize(directory)
            .map_err(|_| "No se pudo preparar el archivo temporal de la imagen.")?;
        let path = directory.join(format!(
            "NotDiscord.image.{}.tmp",
            uuid::Uuid::new_v4().simple()
        ));
        let handle = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(1)
            .open(&path)
            .map_err(|_| "No se pudo crear el archivo temporal de la imagen.")?;
        let mut spool = Self {
            path,
            handle: Some(handle),
            len: 0,
        };
        let mut buffer = [0u8; COPY_BUFFER];
        loop {
            if cancelled() {
                return Err("Carga de imagen cancelada.".into());
            }
            let count = source
                .read(&mut buffer)
                .map_err(|_| "La descarga de la imagen se interrumpió.")?;
            if count == 0 {
                break;
            }
            spool.len = spool.len.saturating_add(count as u64);
            if spool.len > limit {
                return Err(super::super::media_limit::error(limit));
            }
            spool
                .handle
                .as_mut()
                .unwrap()
                .write_all(&buffer[..count])
                .map_err(|_| "No se pudo guardar la imagen temporal.")?;
        }
        spool
            .handle
            .as_mut()
            .unwrap()
            .flush()
            .map_err(|_| "No se pudo guardar la imagen temporal.")?;
        // WIC opens a second read handle. Closing the writer avoids requiring a
        // decoder to grant write sharing; the retained read handle denies edits.
        spool.handle.take();
        spool.handle = Some(
            OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(&spool.path)
                .map_err(|_| "No se pudo preparar la imagen descargada.")?,
        );
        let metadata = spool
            .handle
            .as_ref()
            .unwrap()
            .metadata()
            .map_err(|_| "No se pudo preparar la imagen descargada.")?;
        if !metadata.is_file()
            || metadata.file_attributes() & REPARSE_POINT != 0
            || metadata.len() != spool.len
        {
            return Err("El archivo temporal de la imagen no es válido.".into());
        }
        Ok(spool)
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    #[cfg(test)]
    pub(super) fn len(&self) -> u64 {
        self.len
    }
}
impl Drop for EncodedSpool {
    fn drop(&mut self) {
        self.handle.take();
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Default, Debug)]
pub(super) struct DecodeGate {
    occupied: Mutex<bool>,
    available: Condvar,
    #[cfg(test)]
    waiting: std::sync::atomic::AtomicUsize,
}
/// Move this guard alongside the decoded result, including into a channel.
/// Releasing it immediately after decoding would let blocked senders each hold
/// another full bitmap despite a bounded channel.
#[derive(Debug)]
pub(super) struct DecodePermit(Arc<DecodeGate>);
impl DecodeGate {
    pub(super) fn acquire(self: &Arc<Self>, cancelled: impl Fn() -> bool) -> Option<DecodePermit> {
        let mut occupied = self.occupied.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if cancelled() {
                return None;
            }
            if !*occupied {
                *occupied = true;
                return Some(DecodePermit(self.clone()));
            }
            #[cfg(test)]
            self.waiting
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            occupied = self
                .available
                .wait_timeout(occupied, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner())
                .0;
            #[cfg(test)]
            self.waiting
                .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
    }
    #[cfg(test)]
    pub(super) fn waiting(&self) -> usize {
        self.waiting.load(std::sync::atomic::Ordering::Acquire)
    }
}
impl Drop for DecodePermit {
    fn drop(&mut self) {
        *self.0.occupied.lock().unwrap_or_else(|e| e.into_inner()) = false;
        self.0.available.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, sync::mpsc};
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "notdiscord-spool-test-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            // Tests only create exact files in this freshly created directory.
            for entry in fs::read_dir(&self.0).unwrap().flatten() {
                let _ = fs::remove_file(entry.path());
            }
            let _ = fs::remove_dir(&self.0);
        }
    }
    #[test]
    fn streamed_spool_survives_short_reads_and_removes_only_its_own_file() {
        let directory = Directory::new();
        let foreign = directory.0.join("keep.txt");
        fs::write(&foreign, b"keep").unwrap();
        let source = vec![37; COPY_BUFFER * 3 + 1];
        let spool = EncodedSpool::copy_from(
            Cursor::new(&source),
            Some(source.len() as u64),
            MAX_ENCODED,
            &directory.0,
        )
        .unwrap();
        assert_eq!(spool.len(), source.len() as u64);
        assert_eq!(fs::read(spool.path()).unwrap(), source);
        assert!(OpenOptions::new().write(true).open(spool.path()).is_err());
        let path = spool.path().to_owned();
        drop(spool);
        assert!(!path.exists());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
    }
    #[test]
    fn over_limit_cancelled_and_interrupted_downloads_leave_no_partial_file() {
        let directory = Directory::new();
        for advertised in [Some(9), None] {
            assert!(
                EncodedSpool::copy_from(Cursor::new([1; 9]), advertised, 8, &directory.0).is_err()
            );
            assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
        }
        assert!(
            EncodedSpool::copy_cancellable(Cursor::new([1; 8]), None, 8, &directory.0, || true)
                .is_err()
        );
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::UnexpectedEof.into())
            }
        }
        assert!(EncodedSpool::copy_from(Broken, None, 8, &directory.0).is_err());
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
    }
    #[test]
    fn slow_ui_can_retain_only_one_admitted_bitmap_even_with_three_producers() {
        let gate = Arc::new(DecodeGate::default());
        let (send, receive) = mpsc::sync_channel(3);
        let mut workers = Vec::new();
        for _ in 0..3 {
            let gate = gate.clone();
            let send = send.clone();
            workers.push(std::thread::spawn(move || {
                let permit = gate.acquire(|| false).unwrap();
                let pixels = vec![25u8; 16 * 1024];
                send.send((permit, pixels)).unwrap();
            }));
        }
        drop(send);
        for _ in 0..3 {
            let result = receive.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(receive.try_recv().is_err());
            assert!(gate.acquire(|| true).is_none());
            assert_eq!(result.1.len(), 16 * 1024);
            drop(result);
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(gate.acquire(|| false).is_some());
    }
}
