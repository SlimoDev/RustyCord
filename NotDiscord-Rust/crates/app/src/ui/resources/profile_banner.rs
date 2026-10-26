//! Profile animation has its own receiver and never changes the explicit chat
//! GIF player. Encoded sources live in bounded temporary spools; the decoder
//! keeps a sequential composition/disposal state and one reduced result, not
//! every full-size frame. Retained UI textures are bounded separately.
use super::*;

const ENCODED_LIMIT: u64 = 8 * 1024 * 1024;
const SPOOL_BUDGET: u64 = 16 * 1024 * 1024;
const TEXTURE_BUDGET: usize = 4 * 1024 * 1024;
const MAX_ITEMS: usize = 4;
const MAX_FRAMES: usize = 600;
const TTL: Duration = Duration::from_secs(45);
const INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 30);
const HOVER_DELAY: Duration = Duration::from_millis(150);

struct Source {
    spool: EncodedSpool,
    bytes: u64,
    delays: Vec<Duration>,
    seen: std::cell::Cell<Instant>,
}
struct Stream {
    frames: image::Frames<'static>,
    source: Arc<Source>,
    index: usize,
    pixels: Option<Pixels>,
}
impl Stream {
    fn new(source: Arc<Source>) -> Result<Self, String> {
        let file =
            std::fs::File::open(source.spool.path()).map_err(|_| "No se pudo abrir el banner.")?;
        let reader = std::io::BufReader::new(file);
        let frames = if is_webp(source.spool.path())? {
            image::codecs::webp::WebPDecoder::new(reader)
                .map_err(|_| "Banner WebP dañado.")?
                .into_frames()
        } else {
            image::codecs::gif::GifDecoder::new(reader)
                .map_err(|_| "Banner GIF dañado.")?
                .into_frames()
        };
        Ok(Self {
            frames,
            source,
            index: 0,
            pixels: None,
        })
    }
    fn frame(&mut self, index: usize, cancelled: impl Fn() -> bool) -> PictureResult {
        if self.pixels.is_some() && index + 1 < self.index {
            *self = Self::new(self.source.clone())?;
        }
        while self.index <= index {
            if cancelled() {
                return Err("Carga del banner cancelada.".into());
            }
            let frame = self
                .frames
                .next()
                .ok_or("El banner contiene una secuencia incompleta.")?
                .map_err(|_| "No se pudo decodificar el banner.")?;
            // The image iterators perform source blending and disposal before
            // this resize. Skipping presentation never skips composition.
            let pixels = frame.into_buffer();
            let reduced = image::imageops::thumbnail(&pixels, 768, 320);
            self.pixels = Some((reduced.width(), reduced.height(), reduced.into_raw()));
            self.index += 1;
        }
        self.pixels
            .clone()
            .ok_or_else(|| "El banner no contiene imágenes.".into())
    }
}
fn is_webp(path: &Path) -> Result<bool, String> {
    let mut file = std::fs::File::open(path).map_err(|_| "No se pudo leer el banner.")?;
    let mut magic = [0; 12];
    file.read_exact(&mut magic)
        .map_err(|_| "Banner incompleto.")?;
    Ok(&magic[..4] == b"RIFF" && &magic[8..] == b"WEBP")
}
fn source_info(
    spool: &EncodedSpool,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<Duration>, String> {
    let error = || "El banner animado supera los límites locales o está dañado.".to_owned();
    if is_webp(spool.path())? {
        // Headers only: validate frame rectangles/durations without decoding or
        // retaining a complete animation. Encoded input is bounded on disk.
        use std::io::{Seek, SeekFrom};
        let mut file = std::fs::File::open(spool.path()).map_err(|_| error())?;
        let length = file.metadata().map_err(|_| error())?.len();
        let mut header = [0; 12];
        file.read_exact(&mut header).map_err(|_| error())?;
        if u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap())) + 8 != length {
            return Err(error());
        }
        let u24 = |p: &[u8]| u32::from(p[0]) | u32::from(p[1]) << 8 | u32::from(p[2]) << 16;
        let mut canvas = None;
        let mut delays = Vec::new();
        let mut position = 12u64;
        let mut chunks = 0;
        while position < length {
            if cancelled() || position + 8 > length {
                return Err(error());
            }
            let mut chunk = [0; 8];
            file.read_exact(&mut chunk).map_err(|_| error())?;
            let size = u64::from(u32::from_le_bytes(chunk[4..].try_into().unwrap()));
            let next = position
                .checked_add(8 + size + size % 2)
                .filter(|n| *n <= length)
                .ok_or_else(error)?;
            match &chunk[..4] {
                b"VP8X" => {
                    if size != 10 || canvas.is_some() {
                        return Err(error());
                    }
                    let mut info = [0; 10];
                    file.read_exact(&mut info).map_err(|_| error())?;
                    let (w, h) = (u24(&info[4..7]) + 1, u24(&info[7..10]) + 1);
                    if info[0] & 2 == 0 || !valid_canvas(w, h) {
                        return Err(error());
                    }
                    canvas = Some((w, h));
                }
                b"ANMF" => {
                    if size < 24 {
                        return Err(error());
                    }
                    let (w, h) = canvas.ok_or_else(error)?;
                    let mut info = [0; 16];
                    file.read_exact(&mut info).map_err(|_| error())?;
                    if u24(&info[..3]) * 2 + u24(&info[6..9]) + 1 > w
                        || u24(&info[3..6]) * 2 + u24(&info[9..12]) + 1 > h
                    {
                        return Err(error());
                    }
                    delays.push(Duration::from_millis(u64::from(
                        u24(&info[12..15]).clamp(10, 10_000),
                    )));
                    if delays.len() > MAX_FRAMES
                        || u64::from(w) * u64::from(h) * delays.len() as u64 > 128_000_000
                    {
                        return Err(error());
                    }
                }
                _ => {}
            }
            chunks += 1;
            if chunks > MAX_FRAMES * 4 + 16 {
                return Err(error());
            }
            file.seek(SeekFrom::Start(next)).map_err(|_| error())?;
            position = next;
        }
        return (!delays.is_empty()).then_some(delays).ok_or_else(error);
    }
    let file = std::fs::File::open(spool.path()).map_err(|_| error())?;
    let mut scan = gif::DecodeOptions::new()
        .read_info(std::io::BufReader::new(file))
        .map_err(|_| error())?;
    let (w, h) = (u32::from(scan.width()), u32::from(scan.height()));
    if !valid_canvas(w, h) {
        return Err(error());
    }
    let mut delays = Vec::new();
    while let Some(frame) = scan.next_frame_info().map_err(|_| error())? {
        if cancelled()
            || frame.width == 0
            || frame.height == 0
            || u32::from(frame.left) + u32::from(frame.width) > w
            || u32::from(frame.top) + u32::from(frame.height) > h
        {
            return Err(error());
        }
        delays.push(Duration::from_millis(
            (u64::from(frame.delay) * 10).clamp(10, 10_000),
        ));
        if delays.len() > MAX_FRAMES
            || u64::from(w) * u64::from(h) * delays.len() as u64 > 128_000_000
        {
            return Err(error());
        }
    }
    (!delays.is_empty()).then_some(delays).ok_or_else(error)
}
fn valid_canvas(w: u32, h: u32) -> bool {
    w > 0 && h > 0 && w <= 1536 && h <= 1024 && u64::from(w) * u64::from(h) <= 1024 * 1024
}
struct Job {
    epoch: u64,
    serial: u64,
    url: String,
    elapsed: Duration,
}
type Reply = (
    u64,
    u64,
    String,
    Result<(Pixels, Duration), String>,
    DecodePermit,
);
struct Worker {
    request: mpsc::SyncSender<Job>,
    ready: mpsc::Receiver<Reply>,
}
struct Entry {
    texture: Option<TextureHandle>,
    bytes: usize,
    seen: Instant,
    hover: Option<Instant>,
    started: Option<Instant>,
    requested: Instant,
    next: Instant,
    pending: bool,
    serial: u64,
    error: Option<(Instant, String)>,
}
pub(super) struct Cache {
    worker: Option<Worker>,
    entries: HashMap<String, Entry>,
    requested: HashSet<String>,
    visible: HashSet<String>,
    wanted: Arc<Mutex<HashMap<String, u64>>>,
    epoch: Arc<AtomicU64>,
    gate: Arc<DecodeGate>,
    download_limit: Arc<AtomicU64>,
    serial: u64,
}
impl Cache {
    pub(super) fn new(gate: Arc<DecodeGate>, download_limit: Arc<AtomicU64>) -> Self {
        Self {
            worker: None,
            entries: HashMap::new(),
            requested: HashSet::new(),
            visible: HashSet::new(),
            wanted: Arc::new(Mutex::new(HashMap::new())),
            epoch: Arc::new(AtomicU64::new(0)),
            gate,
            download_limit,
            serial: 0,
        }
    }
    fn initialize(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            return;
        }
        let (request, receive) = mpsc::sync_channel::<Job>(8);
        let (done, ready) = mpsc::sync_channel(1);
        let epoch = self.epoch.clone();
        let wanted = self.wanted.clone();
        let gate = self.gate.clone();
        let wake = ctx.clone();
        let download_limit = self.download_limit.clone();
        std::thread::spawn(move || {
            let mut sources = HashMap::<String, Arc<Source>>::new();
            let mut stream: Option<(String, Stream)> = None;
            let mut known_epoch = epoch.load(Ordering::Acquire);
            loop {
                if known_epoch != epoch.load(Ordering::Acquire) {
                    stream = None;
                    sources.clear();
                    known_epoch = epoch.load(Ordering::Acquire);
                }
                sources.retain(|_, source| source.seen.get().elapsed() < TTL);
                if stream.as_ref().is_some_and(|(url, _)| {
                    !wanted
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .contains_key(url)
                }) {
                    stream = None;
                }
                let job = match receive.recv_timeout(Duration::from_millis(250)) {
                    Ok(job) => job,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let cancelled = || {
                    epoch.load(Ordering::Acquire) != job.epoch
                        || wanted
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get(&job.url)
                            != Some(&job.serial)
                };
                if cancelled() {
                    continue;
                }
                let source = if let Some(source) = sources.get(&job.url) {
                    source.seen.set(Instant::now());
                    Ok(source.clone())
                } else {
                    download_spool(
                        &job.url,
                        ENCODED_LIMIT.min(download_limit.load(Ordering::Acquire)),
                        cancelled,
                    )
                    .and_then(|(spool, _)| {
                        let bytes = std::fs::metadata(spool.path())
                            .map_err(|_| "No se pudo leer el banner.")?
                            .len();
                        let delays = source_info(&spool, cancelled)?;
                        while sources.len() >= MAX_ITEMS
                            || sources.values().map(|s| s.bytes).sum::<u64>() + bytes > SPOOL_BUDGET
                        {
                            let Some(oldest) = sources
                                .iter()
                                .min_by_key(|(_, s)| s.seen.get())
                                .map(|(url, _)| url.clone())
                            else {
                                break;
                            };
                            sources.remove(&oldest);
                        }
                        let source = Arc::new(Source {
                            spool,
                            bytes,
                            delays,
                            seen: std::cell::Cell::new(Instant::now()),
                        });
                        sources.insert(job.url.clone(), source.clone());
                        Ok(source)
                    })
                };
                let Some(permit) = gate.acquire(cancelled) else {
                    continue;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    source.and_then(|source| {
                        let (index, after) = animation_clock::source_frame(
                            source.delays.iter().copied(),
                            job.elapsed,
                        );
                        if stream.as_ref().is_none_or(|(url, _)| *url != job.url) {
                            stream = Some((job.url.clone(), Stream::new(source)?));
                        }
                        stream
                            .as_mut()
                            .unwrap()
                            .1
                            .frame(index, cancelled)
                            .map(|pixels| (pixels, after))
                    })
                }))
                .unwrap_or_else(|_| {
                    Err(
                        "No se pudo decodificar el banner; la imagen estática sigue disponible."
                            .into(),
                    )
                });
                if cancelled() {
                    continue;
                }
                if done
                    .send((job.epoch, job.serial, job.url, result, permit))
                    .is_err()
                {
                    break;
                }
                wake.request_repaint();
            }
        });
        self.worker = Some(Worker { request, ready });
    }
    pub(super) fn begin_frame(&mut self) {
        self.requested.clear();
        self.visible.clear();
    }
    pub(super) fn error(&self, url: &str) -> Option<&str> {
        self.entries
            .get(url)?
            .error
            .as_ref()
            .map(|(_, error)| error.as_str())
    }
    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        let Some(worker) = &self.worker else {
            return;
        };
        while let Ok((epoch, serial, url, result, _permit)) = worker.ready.try_recv() {
            if epoch != self.epoch.load(Ordering::Acquire) {
                continue;
            }
            let Some(entry) = self
                .entries
                .get_mut(&url)
                .filter(|entry| entry.serial == serial && entry.pending)
            else {
                continue;
            };
            entry.pending = false;
            match result {
                Ok(((w, h, pixels), after)) => {
                    entry.next = Instant::now() + after.max(INTERVAL);
                    entry.bytes = pixels.len();
                    let image =
                        ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
                    if let Some(texture) = &mut entry.texture {
                        texture.set(image, TextureOptions::LINEAR);
                    } else {
                        entry.texture = Some(ctx.load_texture(
                            format!("profile-banner:{url}"),
                            image,
                            TextureOptions::LINEAR,
                        ));
                    }
                    entry.error = None;
                }
                Err(error) => entry.error = Some((Instant::now(), error)),
            }
        }
    }
    pub(super) fn texture(
        &mut self,
        ctx: &egui::Context,
        url: &str,
        hovered: bool,
    ) -> Option<TextureHandle> {
        self.poll(ctx);
        let now = Instant::now();
        self.visible.insert(url.to_owned());
        if !self.entries.contains_key(url) && self.entries.len() >= MAX_ITEMS {
            if let Some(oldest) = self
                .entries
                .iter()
                .filter(|(url, _)| !self.requested.contains(*url))
                .min_by_key(|(_, entry)| entry.seen)
                .map(|(url, _)| url.clone())
            {
                self.entries.remove(&oldest);
            } else {
                return None;
            }
        }
        let entry = self.entries.entry(url.to_owned()).or_insert_with(|| Entry {
            texture: None,
            bytes: 0,
            seen: now,
            hover: None,
            started: None,
            requested: now - INTERVAL,
            next: now,
            pending: false,
            serial: 0,
            error: None,
        });
        entry.seen = now;
        if !hovered {
            entry.hover = None;
            entry.started = None;
            return None;
        }
        let hover = *entry.hover.get_or_insert(now);
        if now.saturating_duration_since(hover) < HOVER_DELAY {
            ctx.request_repaint_after(
                HOVER_DELAY.saturating_sub(now.saturating_duration_since(hover)),
            );
            return None;
        }
        if entry.started.is_none() {
            entry.next = now;
            entry.requested = now - INTERVAL;
        }
        let elapsed = now.saturating_duration_since(*entry.started.get_or_insert(now));
        self.requested.insert(url.to_owned());
        let should_send = !entry.pending
            && now >= entry.next
            && now.saturating_duration_since(entry.requested) >= INTERVAL
            && entry
                .error
                .as_ref()
                .is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(30));
        let texture = entry.texture.clone();
        if should_send {
            self.initialize(ctx);
            self.serial = self.serial.wrapping_add(1);
            let entry = self.entries.get_mut(url).unwrap();
            entry.serial = self.serial;
            self.wanted
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(url.into(), entry.serial);
            let job = Job {
                epoch: self.epoch.load(Ordering::Acquire),
                serial: entry.serial,
                url: url.into(),
                elapsed,
            };
            if self.worker.as_ref().unwrap().request.try_send(job).is_ok() {
                entry.pending = true;
                entry.requested = now;
            }
        }
        let next = self.entries.get(url).map_or(INTERVAL, |entry| {
            entry.error.as_ref().map_or_else(
                || entry.next.saturating_duration_since(now).max(INTERVAL),
                |(at, _)| {
                    Duration::from_secs(30)
                        .saturating_sub(at.elapsed())
                        .max(INTERVAL)
                },
            )
        });
        ctx.request_repaint_after(next);
        texture
    }
    pub(super) fn finish_frame(&mut self, minimized: bool) {
        if minimized {
            self.clear();
            return;
        }
        self.wanted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|url, _| self.requested.contains(url));
        self.entries.retain(|_, entry| entry.seen.elapsed() < TTL);
        for (url, entry) in &mut self.entries {
            if !self.requested.contains(url) {
                entry.pending = false;
            }
            if !self.visible.contains(url) {
                entry.hover = None;
                entry.started = None;
            }
        }
        while self
            .entries
            .values()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            > TEXTURE_BUDGET
        {
            let Some(oldest) = self
                .entries
                .iter()
                .filter(|(url, _)| !self.requested.contains(*url))
                .min_by_key(|(_, entry)| entry.seen)
                .map(|(url, _)| url.clone())
            else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }
    pub(super) fn clear(&mut self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.entries.clear();
        self.requested.clear();
        self.visible.clear();
        self.wanted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        if let Some(worker) = &self.worker {
            while worker.ready.try_recv().is_ok() {}
        }
    }
}
impl Drop for Cache {
    fn drop(&mut self) {
        self.clear();
    }
}
