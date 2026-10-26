//! Small, independent cache for visible custom emoji animations. It does not
//! replace the user's explicit full-size GIF player or load a Unicode catalog.
use super::*;

const MAX_ITEMS: usize = 32;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ENTRY_BYTES: usize = 512 * 1024;
const MAX_ENCODED_EMOJI: u64 = 1024 * 1024;
const MAX_FRAMES: usize = 400;
type Reply = (u64, String, Result<AnimationFrames, String>);
struct Entry {
    frames: Vec<(TextureHandle, Duration)>,
    index: usize,
    next: Instant,
    seen: Instant,
    bytes: usize,
}
struct Worker {
    request: mpsc::SyncSender<(u64, String)>,
    ready: mpsc::Receiver<Reply>,
}
#[derive(Default)]
pub(super) struct Cache {
    entries: HashMap<String, Entry>,
    pending: HashSet<String>,
    failed: HashMap<String, Instant>,
    worker: Option<Worker>,
    epoch: Arc<AtomicU64>,
    wanted: Arc<Mutex<HashSet<String>>>,
    requested_this_frame: HashSet<String>,
}
fn requested_side(url: &str) -> u32 {
    url::Url::parse(url).ok().filter(|url|url.query_pairs().any(|(key,value)|key=="size"&&value=="48"))
        .map_or(32,|_|48)
}
#[cfg(test)]
fn decode(bytes: &[u8], cancelled: impl Fn() -> bool) -> Result<AnimationFrames, String> {
    decode_for_side(bytes,cancelled,32)
}
fn decode_for_side(bytes: &[u8], cancelled: impl Fn() -> bool, max_side: u32) -> Result<AnimationFrames, String> {
    if bytes.len() as u64 > MAX_ENCODED_EMOJI {
        return Err("El emoji supera el límite local.".into());
    }
    if bytes.starts_with(b"RIFF") {
        return decode_webp(bytes, cancelled,max_side);
    }
    decode_gif(bytes, cancelled,max_side)
}
fn decode_gif(bytes: &[u8], cancelled: impl Fn() -> bool, max_side:u32) -> Result<AnimationFrames, String> {
    let error = || "Emoji animado no disponible o fuera del límite local.".to_string();
    let mut scan = gif::DecodeOptions::new()
        .read_info(Cursor::new(bytes))
        .map_err(|_| error())?;
    let (width, height) = (u32::from(scan.width()), u32::from(scan.height()));
    if width == 0 || height == 0 || width > 256 || height > 256 {
        return Err(error());
    }
    let mut count = 0;
    while let Some(frame) = scan.next_frame_info().map_err(|_| error())? {
        if frame.width == 0
            || frame.height == 0
            || u32::from(frame.width) > width
            || u32::from(frame.height) > height
            || u32::from(frame.left) + u32::from(frame.width) > width
            || u32::from(frame.top) + u32::from(frame.height) > height
        {
            return Err(error());
        }
        count += 1;
        if count > MAX_FRAMES || cancelled() {
            return Err(error());
        }
    }
    if count == 0 {
        return Err(error());
    }
    let side = ((MAX_ENTRY_BYTES / (count * 4)) as f64)
        .sqrt()
        .floor()
        .clamp(1., f64::from(max_side.clamp(1,48))) as u32;
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|_| error())?;
    let mut frames = Vec::with_capacity(count);
    for frame in decoder.into_frames() {
        if cancelled() || frames.len() >= count {
            return Err(error());
        }
        let frame = frame.map_err(|_| error())?;
        let (n, d) = frame.delay().numer_denom_ms();
        let duration = Duration::from_millis(u64::from((n / d.max(1)).clamp(40, 10_000)));
        let bitmap = frame.into_buffer();
        let thumb = image::imageops::thumbnail(&bitmap, side, side);
        frames.push((thumb.width(), thumb.height(), thumb.into_raw(), duration));
    }
    if frames.len() != count {
        return Err(error());
    }
    Ok(frames)
}
fn webp_info(bytes: &[u8]) -> Result<(u32, u32, usize), String> {
    let error = || "Emoji WebP no disponible o fuera del límite local.".to_string();
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(error());
    }
    let end = (u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize)
        .checked_add(8)
        .filter(|end| *end == bytes.len())
        .ok_or_else(error)?;
    let u24 = |p: &[u8]| u32::from(p[0]) | (u32::from(p[1]) << 8) | (u32::from(p[2]) << 16);
    let mut canvas = None;
    let mut count = 0;
    let mut position = 12usize;
    let mut chunks = 0;
    while position < end {
        if position.checked_add(8).is_none_or(|next| next > end) {
            return Err(error());
        }
        let length =
            u32::from_le_bytes(bytes[position + 4..position + 8].try_into().unwrap()) as usize;
        let start = position + 8;
        let next = start
            .checked_add(length)
            .filter(|next| *next <= end)
            .ok_or_else(error)?;
        let data = &bytes[start..next];
        match &bytes[position..position + 4] {
            b"VP8X" => {
                if canvas.is_some() || data.len() != 10 || data[0] & 2 == 0 {
                    return Err(error());
                }
                let (width, height) = (u24(&data[4..7]) + 1, u24(&data[7..10]) + 1);
                if width > 256 || height > 256 {
                    return Err(error());
                }
                canvas = Some((width, height));
            }
            b"ANMF" => {
                let (width, height) = canvas.ok_or_else(error)?;
                if data.len() < 24 {
                    return Err(error());
                }
                let (x, y, w, h) = (
                    u24(&data[0..3]) * 2,
                    u24(&data[3..6]) * 2,
                    u24(&data[6..9]) + 1,
                    u24(&data[9..12]) + 1,
                );
                if x + w > width || y + h > height {
                    return Err(error());
                }
                count += 1;
                if count > MAX_FRAMES {
                    return Err(error());
                }
            }
            _ => {}
        }
        chunks += 1;
        if chunks > MAX_FRAMES * 4 + 16 {
            return Err(error());
        }
        position = next
            .checked_add(length % 2)
            .filter(|next| *next <= end)
            .ok_or_else(error)?;
    }
    let (width, height) = canvas.ok_or_else(error)?;
    if count == 0 {
        return Err(error());
    }
    Ok((width, height, count))
}
fn decode_webp(bytes: &[u8], cancelled: impl Fn() -> bool, max_side:u32) -> Result<AnimationFrames, String> {
    use image::ImageDecoder;
    let error = || "Emoji WebP no disponible o fuera del límite local.".to_string();
    let (width, height, count) = webp_info(bytes)?;
    if cancelled() {
        return Err(error());
    }
    let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|_| error())?;
    if !decoder.has_animation() || decoder.dimensions() != (width, height) {
        return Err(error());
    }
    let side = ((MAX_ENTRY_BYTES / (count * 4)) as f64)
        .sqrt()
        .floor()
        .clamp(1., f64::from(max_side.clamp(1,48))) as u32;
    let mut frames = Vec::with_capacity(count);
    for frame in decoder.into_frames() {
        if cancelled() || frames.len() >= count {
            return Err(error());
        }
        let frame = frame.map_err(|_| error())?;
        let (n, d) = frame.delay().numer_denom_ms();
        let duration = Duration::from_millis(u64::from((n / d.max(1)).clamp(40, 10_000)));
        let bitmap = frame.into_buffer();
        if bitmap.dimensions() != (width, height) {
            return Err(error());
        }
        let thumb = image::imageops::thumbnail(&bitmap, side, side);
        frames.push((thumb.width(), thumb.height(), thumb.into_raw(), duration));
    }
    if frames.len() != count {
        return Err(error());
    }
    Ok(frames)
}
impl Cache {
    fn initialize(&mut self, ctx: &egui::Context) {
        if self.worker.is_some() {
            return;
        }
        let (request, receive) = mpsc::sync_channel::<(u64, String)>(MAX_ITEMS);
        let (done, ready) = mpsc::sync_channel(1);
        let epoch = self.epoch.clone();
        let wanted = self.wanted.clone();
        let wake = ctx.clone();
        std::thread::spawn(move || {
            while let Ok((generation, url)) = receive.recv() {
                let cancellation_seen = std::cell::Cell::new(false);
                let cancelled = || {
                    let result = epoch.load(Ordering::Acquire) != generation
                        || !wanted.lock().unwrap_or_else(|error| error.into_inner()).contains(&url);
                    if result { cancellation_seen.set(true); }
                    result
                };
                if cancelled() {
                    continue;
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    download_bytes_cancellable(&url, MAX_ENCODED_EMOJI, cancelled).and_then(|bytes| {
                        decode_for_side(&bytes, cancelled,requested_side(&url))
                    })
                }))
                .unwrap_or_else(|_| Err("No se pudo decodificar este emoji.".into()));
                if cancellation_seen.get() || cancelled() { continue; }
                if done.send((generation, url, result)).is_err() {
                    break;
                }
                wake.request_repaint();
            }
        });
        self.worker = Some(Worker { request, ready });
    }
    pub(super) fn poll(&mut self, ctx: &egui::Context) {
        let replies: Vec<_> = self
            .worker
            .as_ref()
            .map(|worker| worker.ready.try_iter().collect())
            .unwrap_or_default();
        for (generation, url, result) in replies {
            if generation != self.epoch.load(Ordering::Acquire)
                || !self.wanted.lock().unwrap_or_else(|error| error.into_inner()).contains(&url) {
                continue;
            }
            self.pending.remove(&url);
            if self.failed.len() >= 64 {
                if let Some(oldest) = self.failed.iter().min_by_key(|(_, at)| **at).map(|(url, _)| url.clone()) { self.failed.remove(&oldest); }
            }
            let frames = match result {
                Ok(frames) => frames,
                Err(_) => {
                    if self.failed.len() >= 64 {
                        self.failed
                            .retain(|_, at| at.elapsed() < Duration::from_secs(60));
                        if self.failed.len() >= 64 {
                            self.failed.clear();
                        }
                    }
                    self.failed.insert(url, Instant::now());
                    continue;
                }
            };
            let bytes = frames.iter().map(|frame| frame.2.len()).sum::<usize>();
            if bytes > MAX_BYTES {
                self.failed.insert(url, Instant::now());
                continue;
            }
            if self.entries.len() >= MAX_ITEMS
                || self
                    .entries
                    .values()
                    .map(|entry| entry.bytes)
                    .sum::<usize>()
                    + bytes
                    > MAX_BYTES
            {
                // Never evict another emoji from the same hovered message and
                // re-download it on the next pass. Budget overflow uses the
                // existing static image with a bounded retry delay instead.
                self.failed.insert(url, Instant::now());
                continue;
            }
            let frames: Vec<_> = frames
                .into_iter()
                .enumerate()
                .map(|(index, (width, height, pixels, delay))| {
                    (
                        ctx.load_texture(
                            format!("custom-emoji:{url}:{index}"),
                            ColorImage::from_rgba_unmultiplied(
                                [width as usize, height as usize],
                                &pixels,
                            ),
                            TextureOptions::LINEAR,
                        ),
                        delay,
                    )
                })
                .collect();
            if let Some(first) = frames.first() {
                let now = Instant::now();
                let next = now + first.1;
                self.entries.insert(
                    url,
                    Entry {
                        frames,
                        index: 0,
                        next,
                        seen: now,
                        bytes,
                    },
                );
            }
        }
    }
    pub(super) fn texture(&mut self, ctx: &egui::Context, url: &str) -> Option<TextureHandle> {
        // Never let this entry point become a second arbitrary URL downloader.
        if !url::Url::parse(url).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str() == Some("cdn.discordapp.com")
                && url.path().starts_with("/emojis/")
                && (url.path().ends_with(".gif")
                    || url.path().ends_with(".webp")
                        && url
                            .query_pairs()
                            .any(|(key, value)| key == "animated" && value == "true"))
        }) {
            return None;
        }
        self.wanted.lock().unwrap_or_else(|error| error.into_inner()).insert(url.into());
        self.requested_this_frame.insert(url.into());
        self.poll(ctx);
        if let Some(entry) = self.entries.get_mut(url) {
            entry.seen = Instant::now();
            if entry.seen >= entry.next {
                entry.index = (entry.index + 1) % entry.frames.len();
                entry.next = entry.seen + entry.frames[entry.index].1;
            }
            ctx.request_repaint_after(entry.next.saturating_duration_since(entry.seen));
            return Some(entry.frames[entry.index].0.clone());
        }
        // Additional simultaneously visible emojis keep their static fallback.
        // Do not evict visible entries and repeatedly re-download them each frame.
        if self.pending.len() + self.entries.len() < MAX_ITEMS
            && !self.pending.contains(url)
            && self
                .failed
                .get(url)
                .is_none_or(|at| at.elapsed() > Duration::from_secs(60))
        {
            self.initialize(ctx);
            if self
                .worker
                .as_ref()
                .unwrap()
                .request
                .try_send((self.epoch.load(Ordering::Acquire), url.into()))
                .is_ok()
            {
                self.pending.insert(url.into());
            }
        }
        None
    }
    #[cfg(any(test, feature = "offline-probes"))]
    pub(super) fn fixture(&mut self, ctx: &egui::Context, url: &str, count: usize) -> Vec<egui::TextureId> {
        self.fixture_sized(ctx, url, count, 2)
    }
    #[cfg(any(test, feature = "offline-probes"))]
    pub(super) fn fixture_sized(&mut self, ctx: &egui::Context, url: &str, count: usize, side: usize) -> Vec<egui::TextureId> {
        let side = side.clamp(1, 32);
        let count = count.clamp(1, MAX_FRAMES).min(MAX_ENTRY_BYTES / (side * side * 4));
        let frames: Vec<_> = (0..count).map(|index| (
            ctx.load_texture(format!("synthetic-animation-{url}-{index}"),
                ColorImage::new([side, side], vec![if index % 2 == 0 { egui::Color32::RED } else { egui::Color32::GREEN }; side * side]),
                TextureOptions::NEAREST), Duration::from_millis(40))).collect();
        let ids = frames.iter().map(|(texture, _)| texture.id()).collect();
        self.entries.insert(url.into(), Entry { frames, index: 0, next: Instant::now() + Duration::from_secs(60), seen: Instant::now(), bytes: count * side * side * 4 });
        ids
    }
    #[cfg(any(test, feature = "offline-probes"))]
    pub(super) fn metrics(&self) -> serde_json::Value {
        serde_json::json!({ "entries": self.entries.len(), "frames": self.entries.values().map(|entry| entry.frames.len()).sum::<usize>(),
            "bytes": self.entries.values().map(|entry| entry.bytes).sum::<usize>(), "pending": self.pending.len(),
            "requested": self.requested_this_frame.len(), "worker": self.worker.is_some(), "maxBytes": MAX_BYTES, "maxEntries": MAX_ITEMS })
    }
    pub(super) fn begin_frame(&mut self) {
        self.requested_this_frame.clear();
    }
    pub(super) fn finish_frame(&mut self, minimized: bool) {
        if minimized {
            self.clear();
        } else {
            self.wanted.lock().unwrap_or_else(|error| error.into_inner())
                .retain(|url| self.requested_this_frame.contains(url));
            self.entries.retain(|url, _| self.requested_this_frame.contains(url));
            self.pending.retain(|url| self.requested_this_frame.contains(url));
            if self.requested_this_frame.is_empty() {
                // A late decoder result must not resurrect a message after
                // pointer exit, navigation, minimization or clipping.
                self.epoch.fetch_add(1, Ordering::AcqRel);
            }
        }
    }
    pub(super) fn clear(&mut self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.entries = HashMap::new();
        self.pending.clear();
        self.failed.clear();
        self.wanted.lock().unwrap_or_else(|error| error.into_inner()).clear();
        self.requested_this_frame.clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    fn synthetic_webp(count: usize) -> Vec<u8> {
        use image::ImageEncoder;
        fn chunk(out: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(name);
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(data);
            if data.len() % 2 != 0 {
                out.push(0);
            }
        }
        let mut chunks = Vec::new();
        chunk(&mut chunks, b"VP8X", &[2, 0, 0, 0, 1, 0, 0, 1, 0, 0]);
        chunk(&mut chunks, b"ANIM", &[0, 0, 0, 0, 0, 0]);
        for index in 0..count {
            let color = if index % 2 == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            };
            let pixels: Vec<u8> = color.into_iter().cycle().take(16).collect();
            let mut encoded = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
                .write_image(&pixels, 2, 2, image::ExtendedColorType::Rgba8)
                .unwrap();
            let mut frame = vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 50, 0, 0, 2];
            frame.extend_from_slice(&encoded[12..]);
            chunk(&mut chunks, b"ANMF", &frame);
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&((chunks.len() + 4) as u32).to_le_bytes());
        out.extend_from_slice(b"WEBP");
        out.extend_from_slice(&chunks);
        out
    }
    #[test]
    fn webp_animation_preserves_frames_and_checks_container_limits_before_decode() {
        let data = synthetic_webp(3);
        assert_eq!(webp_info(&data).unwrap(), (2, 2, 3));
        let frames = decode(&data, || false).unwrap();
        assert_eq!(frames.len(), 3);
        assert_ne!(frames[0].2, frames[1].2);
        assert_eq!(frames[0].3, Duration::from_millis(50));
        assert!(frames.iter().map(|frame| frame.2.len()).sum::<usize>() <= MAX_ENTRY_BYTES);
        assert!(decode(&synthetic_webp(MAX_FRAMES + 1), || false).is_err());
        assert!(decode(&data, || true).is_err());
        assert!(decode(&data[..data.len() - 1], || false).is_err());
        let mut oversized = data.clone();
        oversized[24..27].fill(0xff);
        assert!(decode(&oversized, || false).is_err());
        let mut malformed = data;
        malformed[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&malformed, || false).is_err());
    }
    fn synthetic_gif(count: usize) -> Vec<u8> {
        synthetic_gif_size(count,2)
    }
    fn synthetic_gif_size(count:usize,side:u16)->Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut out, side, side, &[255, 0, 0, 0, 255, 0]).unwrap();
            for index in 0..count {
                let pixels = vec![(index % 2) as u8; usize::from(side)*usize::from(side)];
                let frame = gif::Frame {
                    width: side,
                    height: side,
                    delay: 5,
                    buffer: std::borrow::Cow::Owned(pixels),
                    ..Default::default()
                };
                encoder.write_frame(&frame).unwrap();
            }
        }
        out
    }
    #[test]
    fn picker_animation_keeps_its_requested_48px_resolution_with_the_existing_entry_budget() {
        let picker="https://cdn.discordapp.com/emojis/401.webp?size=48&animated=true";
        let message="https://cdn.discordapp.com/emojis/401.webp?size=32&animated=true";
        assert_eq!(requested_side(picker),48);
        assert_eq!(requested_side(message),32);
        let data=synthetic_gif_size(3,64);
        let frames=decode_for_side(&data,||false,requested_side(picker)).unwrap();
        assert!(frames.iter().all(|frame|(frame.0,frame.1)==(48,48)));
        let frames=decode_for_side(&data,||false,requested_side(message)).unwrap();
        assert!(frames.iter().all(|frame|(frame.0,frame.1)==(32,32)));
        let frames=decode_for_side(&synthetic_gif_size(100,64),||false,48).unwrap();
        assert!(frames.iter().map(|frame|frame.2.len()).sum::<usize>()<=MAX_ENTRY_BYTES);
        assert!(frames.iter().all(|frame|frame.0<=48&&frame.1<=48));
    }
    #[test]
    fn custom_animation_decode_preserves_frames_color_and_bounded_size_offline() {
        let frames = decode(&synthetic_gif(3), || false).unwrap();
        assert_eq!(frames.len(), 3);
        assert_ne!(frames[0].2, frames[1].2);
        assert!(
            frames.iter().all(|frame| frame.0 <= 32
                && frame.1 <= 32
                && frame.3 >= Duration::from_millis(40))
        );
        assert!(decode(&synthetic_gif(MAX_FRAMES + 1), || false).is_err());
        assert!(decode(&synthetic_gif(2), || true).is_err());
        assert!(decode(b"invalid", || false).is_err());
        let mut oversized = synthetic_gif(1);
        let descriptor = oversized.iter().position(|byte| *byte == 0x2c).unwrap();
        oversized[descriptor + 5] = 0xff;
        oversized[descriptor + 6] = 0xff;
        assert!(decode(&oversized, || false).is_err());
    }
    #[test]
    fn idle_cache_has_no_worker_and_clear_releases_texture_owners() {
        let mut cache = Cache::default();
        assert!(cache.worker.is_none());
        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "fixture-custom",
            ColorImage::new([2, 2], vec![egui::Color32::RED; 4]),
            TextureOptions::LINEAR,
        );
        cache.entries.insert(
            "test".into(),
            Entry {
                frames: vec![(texture, Duration::from_millis(40))],
                index: 0,
                next: Instant::now(),
                seen: Instant::now(),
                bytes: 16,
            },
        );
        cache.clear();
        assert!(cache.entries.is_empty());
        assert!(cache.worker.is_none());
    }
    #[test]
    fn message_hover_cycles_advance_frames_and_return_texture_ownership_to_baseline() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let baseline = ctx.tex_manager().read().num_allocated();
        for _ in 0..100 {
            let mut all_ids = Vec::new();
            cache.begin_frame();
            for index in 0..12 {
                let url = format!("https://cdn.discordapp.com/emojis/{}.webp?size=32&animated=true", 100 + index);
                let ids = cache.fixture(&ctx, &url, 3);
                cache.entries.get_mut(&url).unwrap().next = Instant::now() - Duration::from_secs(1);
                let texture = cache.texture(&ctx, &url).unwrap();
                assert_eq!(texture.id(), ids[1]);
                drop(texture);
                all_ids.extend(ids);
            }
            cache.finish_frame(false);
            assert_eq!(cache.metrics()["entries"], 12);
            assert_eq!(cache.metrics()["requested"], 12);
            assert!(ctx.tex_manager().read().num_allocated() > baseline);
            cache.begin_frame(); cache.finish_frame(false);
            assert_eq!(cache.metrics()["bytes"], 0);
            assert_eq!(cache.metrics()["pending"], 0);
            assert_eq!(ctx.tex_manager().read().num_allocated(), baseline);
            assert!(all_ids.iter().all(|id| ctx.tex_manager().read().meta(*id).is_none()));
            drop(ctx.tex_manager().write().take_delta());
        }
        assert!(cache.worker.is_none(), "offline fixtures never start a network worker");
    }
}
