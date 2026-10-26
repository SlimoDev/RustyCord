//! Import once on explicit selection. Compressed codecs never run in playback.
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use symphonia::core::io::Monitor;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_MP3, CODEC_TYPE_VORBIS, DecoderOptions},
    errors::Error as DecodeError,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::{Limit, MetadataOptions},
    probe::Hint,
};

const STORE_LIMIT: u64 = 32 * 1024 * 1024;
const IMPORT_TIMEOUT: Duration = Duration::from_secs(20);

pub struct ImportedSound {
    pub event: SoundEvent,
    pub path: PathBuf,
    pub reused: bool,
    pub source_name: String,
}
struct Pending {
    rx: mpsc::Receiver<Result<ImportedSound, String>>,
    cancel: Arc<AtomicBool>,
}
#[derive(Default)]
pub struct SoundImporter {
    pending: Option<Pending>,
}
impl SoundImporter {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn start(
        &mut self,
        event: SoundEvent,
        path: PathBuf,
        data: PathBuf,
        protected: Vec<PathBuf>,
        ctx: eframe::egui::Context,
    ) -> Result<(), String> {
        if self.busy() {
            return Err("Ya se está importando un sonido.".into());
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::Builder::new()
            .name("sound-import".into())
            .spawn(move || {
                let result = import_file(&path, event, &data, &protected, &stop);
                let _ = tx.try_send(result);
                ctx.request_repaint();
            })
            .map_err(|_| "No se pudo iniciar la importación.")?;
        self.pending = Some(Pending { rx, cancel });
        Ok(())
    }
    pub fn poll(&mut self) -> Option<Result<ImportedSound, String>> {
        let rx = &self.pending.as_ref()?.rx;
        let result = match rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("La importación se interrumpió.".into())),
        };
        if result.is_some() {
            let cancelled = self
                .pending
                .as_ref()
                .unwrap()
                .cancel
                .load(Ordering::Acquire);
            self.pending = None;
            if cancelled {
                return None;
            }
        }
        result
    }
    pub fn cancel(&mut self) {
        if let Some(p) = &self.pending {
            p.cancel.store(true, Ordering::Release);
        }
    }
}
impl Drop for SoundImporter {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn owned_name(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "wav")
        && path.file_stem().is_some_and(|s| {
            s.to_string_lossy().len() == 64
                && s.to_string_lossy().bytes().all(|c| c.is_ascii_hexdigit())
        })
}
fn owned_temporary(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        name.to_str()
            .and_then(|name| name.strip_prefix(".import-"))
            .and_then(|name| name.strip_suffix(".tmp"))
            .is_some_and(|id| id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit()))
    })
}
fn check_cancel(cancel: &AtomicBool, start: Instant) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Importación cancelada.".into())
    } else if start.elapsed() > IMPORT_TIMEOUT {
        Err("El archivo tardó demasiado en convertirse.".into())
    } else {
        Ok(())
    }
}
pub fn import_file(
    path: &Path,
    event: SoundEvent,
    data: &Path,
    protected: &[PathBuf],
    cancel: &AtomicBool,
) -> Result<ImportedSound, String> {
    let start = Instant::now();
    let source_name = path
        .file_name()
        .map(|n| {
            n.to_string_lossy()
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect::<String>()
        })
        .unwrap_or_else(|| "Audio importado".into());
    let source = local_sound_path(path)?;
    let ext = source
        .extension()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(ext.as_str(), "wav" | "mp3" | "ogg") {
        return Err("Selecciona WAV PCM, MP3 u OGG/Vorbis.".into());
    }
    let mut input = std::fs::File::open(&source).map_err(|_| "No se pudo abrir el sonido.")?;
    let meta = input
        .metadata()
        .map_err(|_| "No se pudo consultar el sonido.")?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > event.file_limit() {
        return Err(format!(
            "El archivo debe ocupar como máximo {} MiB.",
            event.file_limit() / 1024 / 1024
        ));
    }
    let mut hash = Sha256::new();
    hash.update(b"NotDiscord-PCM48-mono16-import-v1\0");
    let mut buffer = [0u8; 16 * 1024];
    let mut read = 0u64;
    loop {
        check_cancel(cancel, start)?;
        let count = input
            .read(&mut buffer)
            .map_err(|_| "No se pudo leer el sonido.")?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > event.file_limit() {
            return Err("El archivo cambió durante la lectura.".into());
        }
        hash.update(&buffer[..count]);
    }
    if read != meta.len() {
        return Err("El archivo cambió durante la lectura.".into());
    }
    input
        .rewind()
        .map_err(|_| "No se pudo volver a leer el sonido.")?;
    let root = data.join("custom-sounds-v1");
    std::fs::create_dir_all(&root).map_err(|_| "No se pudo crear la carpeta de sonidos.")?;
    use std::os::windows::fs::MetadataExt;
    if std::fs::symlink_metadata(&root)
        .map_err(|_| "No se pudo consultar la carpeta.")?
        .file_attributes()
        & 0x400
        != 0
    {
        return Err("La carpeta de sonidos no puede ser un enlace.".into());
    }
    let destination = root.join(format!("{:x}.wav", hash.finalize()));
    if destination.exists() {
        if std::fs::symlink_metadata(&destination)
            .map_err(|_| "No se pudo consultar el sonido importado.")?
            .file_attributes()
            & 0x400
            != 0
        {
            return Err("El sonido importado no puede ser un enlace.".into());
        }
        validate_event_file(&destination, event)?;
        return Ok(ImportedSound {
            event,
            path: destination,
            reused: true,
            source_name,
        });
    }
    // Only prune our own unreferenced assets; both saved and unsaved settings protect files.
    let mut keep = protected.to_vec();
    keep.push(source.clone());
    keep.extend(
        SoundPreferences::load(data)
            .events
            .values()
            .filter_map(|s| s.file.clone()),
    );
    let entries =
        std::fs::read_dir(&root).map_err(|_| "No se pudo consultar la caché de sonidos.")?;
    let mut stored = 0u64;
    let mut count = 0;
    for entry in entries {
        check_cancel(cancel, start)?;
        let entry = entry.map_err(|_| "No se pudo consultar la caché.")?;
        count += 1;
        if count > 256 {
            return Err("La carpeta de sonidos supera su límite de archivos.".into());
        }
        let p = entry.path();
        if !owned_name(&p) && !owned_temporary(&p) {
            continue;
        }
        let m = entry
            .metadata()
            .map_err(|_| "No se pudo consultar un sonido.")?;
        if !m.is_file() || m.file_attributes() & 0x400 != 0 {
            continue;
        }
        // A hard shutdown may leave a temporary asset. Count it against the
        // budget, but never delete a possible import owned by another process.
        if owned_temporary(&p) {
            stored = stored.saturating_add(m.len());
            continue;
        }
        if !keep.iter().any(|saved| saved == &p)
            && std::fs::canonicalize(&p).ok().as_ref() != Some(&source)
        {
            std::fs::remove_file(&p).map_err(|_| "No se pudo retirar un sonido sin uso.")?;
        } else {
            stored = stored.saturating_add(m.len());
        }
    }
    let reserve = (RATE * event.max_seconds() * 2 + 44) as u64;
    if stored.saturating_add(reserve) > STORE_LIMIT {
        return Err("Los sonidos guardados alcanzaron el límite local de 32 MiB. Restaura los que ya no utilices y guarda los ajustes.".into());
    }
    let temporary = root.join(format!(".import-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "No se pudo crear el WAV convertido.")?;
        output
            .write_all(&[0; 44])
            .map_err(|_| "No se pudo escribir el WAV.")?;
        let frames = if ext == "wav" {
            let mut bytes = Vec::new();
            input
                .take(event.file_limit() + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "No se pudo leer el WAV.")?;
            let clip = decode_wav_limited(&bytes, event.max_seconds(), event.file_limit())?;
            check_cancel(cancel, start)?;
            for samples in clip.samples.chunks(4096) {
                let mut block = Vec::with_capacity(samples.len() * 2);
                for s in samples {
                    block.extend(s.to_le_bytes());
                }
                output
                    .write_all(&block)
                    .map_err(|_| "No se pudo escribir el WAV.")?;
            }
            clip.samples.len()
        } else {
            decode_compressed(input, &ext, event, &mut output, cancel, start)?
        };
        if frames == 0 {
            return Err("El sonido no contiene audio.".into());
        }
        let current = std::fs::metadata(&source)
            .map_err(|_| "El archivo original cambió durante la conversión.")?;
        if current.len() != meta.len() || current.modified().ok() != meta.modified().ok() {
            return Err("El archivo original cambió durante la conversión.".into());
        }
        output
            .seek(SeekFrom::Start(0))
            .map_err(|_| "No se pudo finalizar el WAV.")?;
        output
            .write_all(&wav_header(frames))
            .map_err(|_| "No se pudo finalizar el WAV.")?;
        output
            .sync_all()
            .map_err(|_| "No se pudo guardar el WAV.")?;
        drop(output);
        check_cancel(cancel, start)?;
        std::fs::rename(&temporary, &destination)
            .map_err(|_| "No se pudo guardar el sonido convertido.")?;
        Ok(ImportedSound {
            event,
            path: destination.clone(),
            reused: false,
            source_name,
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
fn wav_header(frames: usize) -> [u8; 44] {
    let data = (frames * 2) as u32;
    let mut h = [0u8; 44];
    h[..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(data + 36).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&(RATE as u32).to_le_bytes());
    h[28..32].copy_from_slice(&(RATE as u32 * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}
struct Normalizer<'a> {
    output: &'a mut std::fs::File,
    rate: usize,
    seen: usize,
    next: usize,
    previous: i16,
    limit: usize,
    bytes: Vec<u8>,
}
impl Normalizer<'_> {
    fn sample(&mut self, value: i16) -> Result<(), String> {
        let index = self.seen;
        while self.next * self.rate <= index * RATE {
            if self.next >= self.limit {
                return Err("El audio supera la duración permitida para este evento.".into());
            }
            let position = self.next * self.rate;
            let base = position / RATE;
            let fraction = position % RATE;
            let sample = if base == index {
                value
            } else {
                (self.previous as i64
                    + (value as i64 - self.previous as i64) * fraction as i64 / RATE as i64)
                    as i16
            };
            self.bytes.extend(sample.to_le_bytes());
            self.next += 1;
            if self.bytes.len() >= 8192 {
                self.flush()?;
            }
        }
        self.previous = value;
        self.seen += 1;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), String> {
        self.output
            .write_all(&self.bytes)
            .map_err(|_| "No se pudo escribir el WAV.")?;
        self.bytes.clear();
        Ok(())
    }
    fn finish(mut self) -> Result<usize, String> {
        let frames = self.seen * RATE / self.rate;
        if frames > self.limit {
            return Err("El audio supera la duración permitida para este evento.".into());
        }
        while self.next < frames {
            self.bytes.extend(self.previous.to_le_bytes());
            self.next += 1;
        }
        self.flush()?;
        Ok(self.next)
    }
}
fn decode_compressed(
    mut input: std::fs::File,
    ext: &str,
    event: SoundEvent,
    output: &mut std::fs::File,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<usize, String> {
    let verified = preflight_compressed(&mut input, ext, event.file_limit())?;
    let stream = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(verified)),
        MediaSourceStreamOptions {
            buffer_len: 64 * 1024,
        },
    );
    let mut hint = Hint::new();
    hint.with_extension(ext);
    let metadata = MetadataOptions {
        limit_metadata_bytes: Limit::Maximum(16 * 1024),
        limit_visual_bytes: Limit::Maximum(0),
    };
    let mut probe = symphonia::core::probe::Probe::default();
    if ext == "ogg" {
        probe.register_all::<symphonia::default::formats::OggReader>();
    } else {
        probe.register_all::<symphonia::default::formats::MpaReader>();
    }
    // Do not register a standalone ID3 reader: all leading tags were checked
    // and stripped, and only the requested container may be detected.
    let probed = probe
        .format(
            &hint,
            stream,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &metadata,
        )
        .map_err(|_| "No se reconoce un MP3 u OGG/Vorbis válido.")?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or("El archivo no contiene una pista de audio.")?;
    if ![CODEC_TYPE_MP3, CODEC_TYPE_VORBIS].contains(&track.codec_params.codec) {
        return Err(
            "OGG debe contener Vorbis. Este importador no admite OGG/Opus ni otros códecs.".into(),
        );
    }
    let params = track.codec_params.clone();
    let id = track.id;
    if params.codec == CODEC_TYPE_VORBIS {
        preflight_vorbis(
            params
                .extra_data
                .as_deref()
                .ok_or("OGG sin cabecera Vorbis.")?,
        )?;
    }
    let rate = params
        .sample_rate
        .ok_or("No se pudo determinar la frecuencia de audio.")? as usize;
    let channels = params
        .channels
        .ok_or("No se pudo determinar los canales.")?
        .count();
    if !(8000..=96000).contains(&rate) || !(1..=2).contains(&channels) {
        return Err("Usa audio mono/estéreo entre 8 y 96 kHz.".into());
    }
    if params
        .n_frames
        .is_some_and(|n| n > rate as u64 * event.max_seconds() as u64 + rate as u64 / 4)
    {
        return Err("El audio supera la duración permitida.".into());
    }
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|_| "No se pudo iniciar el decodificador de audio.")?;
    let mut normalize = Normalizer {
        output,
        rate,
        seen: 0,
        next: 0,
        previous: 0,
        limit: RATE * event.max_seconds(),
        bytes: Vec::with_capacity(8192),
    };
    let mut packets = 0usize;
    loop {
        check_cancel(cancel, start)?;
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(_) => return Err("El archivo de audio está dañado o cambia de formato.".into()),
        };
        packets += 1;
        if packets > 32768 || packet.buf().len() > 256 * 1024 {
            return Err("El archivo supera los límites del decodificador.".into());
        }
        if packet.track_id() != id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|_| "No se pudo decodificar el audio; el archivo puede estar dañado.")?;
        if decoded.spec().rate as usize != rate
            || decoded.spec().channels.count() != channels
            || decoded.capacity() > 16384
        {
            return Err("El archivo cambia de formato o supera el límite por bloque.".into());
        }
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        samples.copy_interleaved_ref(decoded);
        for frame in samples.samples().chunks_exact(channels) {
            let value = frame.iter().copied().sum::<f32>() / channels as f32;
            if !value.is_finite() {
                return Err("El audio contiene muestras no válidas.".into());
            }
            normalize.sample((value.clamp(-1., 1.) * 32767.).round() as i16)?;
        }
    }
    normalize.finish()
}

// The codec's metadata options are advisory in 0.5.5. Validate container and
// declared tag sizes before its probe can allocate from those declarations.
fn preflight_compressed(
    input: &mut std::fs::File,
    ext: &str,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    (&mut *input)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "No se pudo comprobar el audio.")?;
    if bytes.len() > limit as usize {
        return Err("El archivo cambió o supera el límite de tamaño.".into());
    }
    if ext == "ogg" {
        preflight_ogg(&bytes)?;
    } else {
        let start = preflight_mp3(&bytes)?;
        bytes.drain(..start);
    }
    Ok(bytes)
}
fn preflight_mp3(bytes: &[u8]) -> Result<usize, String> {
    let bad = || {
        "Los metadatos MP3 están dañados o superan el límite. Exporta el sonido sin portada ni metadatos comprimidos.".to_owned()
    };
    fn sync(b: &[u8]) -> Option<usize> {
        if b.len() != 4 || b.iter().any(|v| v & 0x80 != 0) {
            return None;
        }
        Some(b.iter().fold(0usize, |n, v| (n << 7) | *v as usize))
    }
    // APE is unnecessary for short notification sounds; do not invoke its
    // metadata reader on an arbitrary unbounded advertised item count.
    if bytes.starts_with(b"APETAGEX")
        || bytes.len() >= 32 && &bytes[bytes.len() - 32..bytes.len() - 24] == b"APETAGEX"
        || bytes.len() >= 160 && &bytes[bytes.len() - 160..bytes.len() - 152] == b"APETAGEX"
    {
        return Err(bad());
    }
    let mut offset = 0;
    let mut tags = 0;
    while bytes.get(offset..offset + 3) == Some(b"ID3") {
        tags += 1;
        if tags > 4 {
            return Err(bad());
        }
        let header = bytes.get(offset..offset + 10).ok_or_else(bad)?;
        let version = header[3];
        if ![2, 3, 4].contains(&version)
            || header[5] & 0x80 != 0
            || version == 2 && header[5] & 0x40 != 0
        {
            return Err(bad());
        }
        let size = sync(&header[6..10]).ok_or_else(bad)?;
        if size > 64 * 1024 {
            return Err(bad());
        }
        let start = offset + 10;
        let end = start
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(bad)?;
        let mut at = start;
        if version >= 3 && header[5] & 0x40 != 0 {
            let b = bytes
                .get(at..at + 4)
                .filter(|_| at + 4 <= end)
                .ok_or_else(bad)?;
            let size = if version == 4 {
                sync(b).ok_or_else(bad)?
            } else {
                u32::from_be_bytes(b.try_into().unwrap()) as usize + 4
            };
            if size < 6 {
                return Err(bad());
            }
            at = at.checked_add(size).filter(|n| *n <= end).ok_or_else(bad)?;
        }
        while at < end {
            if bytes[at..end].iter().all(|v| *v == 0) {
                break;
            }
            let length = if version == 2 { 6 } else { 10 };
            let frame = bytes
                .get(at..at + length)
                .filter(|_| at + length <= end)
                .ok_or_else(bad)?;
            let id_length = if version == 2 { 3 } else { 4 };
            if !frame[..id_length]
                .iter()
                .all(|v| v.is_ascii_uppercase() || v.is_ascii_digit())
            {
                return Err(bad());
            }
            let size = if version == 2 {
                ((frame[3] as usize) << 16) | ((frame[4] as usize) << 8) | frame[5] as usize
            } else if version == 3 {
                if frame[9] & 0xc0 != 0 {
                    return Err(bad());
                }
                u32::from_be_bytes(frame[4..8].try_into().unwrap()) as usize
            } else {
                if frame[9] & 0x0c != 0 {
                    return Err(bad());
                }
                sync(&frame[4..8]).ok_or_else(bad)?
            };
            at = at
                .checked_add(length)
                .and_then(|n| n.checked_add(size))
                .filter(|n| *n <= end)
                .ok_or_else(bad)?;
        }
        offset = end;
        if version == 4 && header[5] & 0x10 != 0 {
            offset = offset
                .checked_add(10)
                .filter(|n| *n <= bytes.len())
                .ok_or_else(bad)?;
        }
    }
    let frame = bytes.get(offset..offset + 4).ok_or_else(bad)?;
    if frame[0] != 0xff
        || frame[1] & 0xe0 != 0xe0
        || frame[1] & 6 != 2
        || ((frame[1] >> 3) & 3) == 1
        || frame[2] >> 4 == 15
        || ((frame[2] >> 2) & 3) == 3
    {
        return Err(bad());
    }
    Ok(offset)
}
fn preflight_ogg(bytes: &[u8]) -> Result<(), String> {
    let bad = || {
        "Los metadatos OGG están dañados o superan el límite. Exporta el sonido sin portada y con etiquetas pequeñas.".to_owned()
    };
    let mut streams: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut headers: BTreeMap<u32, (usize, [u8; 30])> = BTreeMap::new();
    let mut at = 0;
    while at < bytes.len() {
        let h = bytes.get(at..at + 27).ok_or_else(bad)?;
        if &h[..4] != b"OggS" || h[4] != 0 || h[5] & 0xf8 != 0 {
            return Err(bad());
        }
        let serial = u32::from_le_bytes(h[14..18].try_into().unwrap());
        let count = h[26] as usize;
        let lacing = bytes.get(at + 27..at + 27 + count).ok_or_else(bad)?;
        let body_length = lacing.iter().map(|n| *n as usize).sum::<usize>();
        let start = at + 27 + count;
        let end = start
            .checked_add(body_length)
            .filter(|n| *n <= bytes.len())
            .ok_or_else(bad)?;
        let mut crc = symphonia::core::checksum::Crc32::new(0);
        crc.process_buf_bytes(&bytes[at..at + 22]);
        crc.process_buf_bytes(&[0; 4]);
        crc.process_buf_bytes(&bytes[at + 26..end]);
        if crc.crc() != u32::from_le_bytes(h[22..26].try_into().unwrap()) {
            return Err(bad());
        }
        if !streams.contains_key(&serial) && streams.len() >= 4 {
            return Err(bad());
        }
        let packet = streams.entry(serial).or_default();
        if (h[5] & 1 == 0) != packet.is_empty() {
            return Err(bad());
        }
        let mut cursor = start;
        for length in lacing {
            let length = *length as usize;
            if packet.len() + length > 256 * 1024 {
                return Err(bad());
            }
            packet.extend_from_slice(&bytes[cursor..cursor + length]);
            cursor += length;
            if length < 255 {
                let state = headers.entry(serial).or_insert((0, [0; 30]));
                if state.0 == 0 {
                    if packet.starts_with(b"OpusHead") {
                        return Err("OGG debe contener Vorbis. OGG/Opus no es compatible con este importador.".into());
                    }
                    if packet.len() != 30
                        || !packet.starts_with(b"\x01vorbis")
                        || packet[7..11] != [0; 4]
                        || ![1, 2].contains(&packet[11])
                        || !(8000..=96000)
                            .contains(&u32::from_le_bytes(packet[12..16].try_into().unwrap()))
                        || packet[29] != 1
                    {
                        return Err(bad());
                    }
                    state.1.copy_from_slice(packet);
                } else if state.0 == 1 {
                    if !packet.starts_with(b"\x03vorbis") {
                        return Err(bad());
                    }
                    preflight_comments(&packet[7..])?;
                } else if state.0 == 2 {
                    if !packet.starts_with(b"\x05vorbis") {
                        return Err(bad());
                    }
                    let mut extra = state.1.to_vec();
                    extra.extend_from_slice(packet);
                    preflight_vorbis(&extra)?;
                } else if packet.first().is_some_and(|n| n & 1 != 0) {
                    // Reject chained header updates rather than letting a tag
                    // reader discover metadata we have not validated.
                    return Err(bad());
                }
                state.0 += 1;
                packet.clear();
            }
        }
        at = end;
    }
    if streams.is_empty()
        || streams.values().any(|p| !p.is_empty())
        || headers.values().any(|(count, _)| *count < 3)
    {
        return Err(bad());
    }
    Ok(())
}
fn preflight_comments(bytes: &[u8]) -> Result<(), String> {
    let bad = || {
        "Las etiquetas OGG superan el presupuesto o contienen una portada. Exporta el sonido sin portada.".to_owned()
    };
    fn number(bytes: &[u8], at: &mut usize) -> Option<usize> {
        let value =
            u32::from_le_bytes(bytes.get(*at..at.checked_add(4)?)?.try_into().ok()?) as usize;
        *at += 4;
        Some(value)
    }
    let mut at = 0;
    let vendor = number(bytes, &mut at).ok_or_else(bad)?;
    if vendor > 4096 {
        return Err(bad());
    }
    at = at
        .checked_add(vendor)
        .filter(|n| *n <= bytes.len())
        .ok_or_else(bad)?;
    let count = number(bytes, &mut at).ok_or_else(bad)?;
    if count > 64 {
        return Err(bad());
    }
    let mut total = vendor;
    for _ in 0..count {
        let length = number(bytes, &mut at).ok_or_else(bad)?;
        total = total.checked_add(length).ok_or_else(bad)?;
        if length > 4096 || total > 16 * 1024 {
            return Err(bad());
        }
        let end = at
            .checked_add(length)
            .filter(|n| *n <= bytes.len())
            .ok_or_else(bad)?;
        let comment = &bytes[at..end];
        if comment
            .get(..23)
            .is_some_and(|p| p.eq_ignore_ascii_case(b"METADATA_BLOCK_PICTURE="))
        {
            return Err(bad());
        }
        at = end;
    }
    Ok(())
}

// Vorbis setup can advertise enormous codebooks even in a tiny file. Check
// dimensions/entry budgets before the codec constructs its lookup tables.
fn preflight_vorbis(extra: &[u8]) -> Result<(), String> {
    let bad = || {
        "La cabecera OGG/Vorbis está dañada o supera el presupuesto de memoria del importador."
            .to_owned()
    };
    if extra.len() < 38
        || extra.len() > 256 * 1024
        || extra[..7] != *b"\x01vorbis"
        || extra[30..37] != *b"\x05vorbis"
    {
        return Err(bad());
    }
    struct Bits<'a> {
        b: &'a [u8],
        at: usize,
    }
    impl Bits<'_> {
        fn read(&mut self, n: usize) -> Option<u32> {
            if n > 32 || self.at.checked_add(n)? > self.b.len() * 8 {
                return None;
            }
            let mut v = 0;
            for i in 0..n {
                v |= (((self.b[(self.at + i) / 8] >> ((self.at + i) % 8)) & 1) as u32) << i;
            }
            self.at += n;
            Some(v)
        }
        fn skip(&mut self, n: usize) -> Option<()> {
            self.at = self.at.checked_add(n)?;
            (self.at <= self.b.len() * 8).then_some(())
        }
    }
    let mut bits = Bits {
        b: &extra[37..],
        at: 0,
    };
    let count = bits.read(8).ok_or_else(bad)? + 1;
    if count > 128 {
        return Err(bad());
    }
    let mut total = 0usize;
    for _ in 0..count {
        if bits.read(24) != Some(0x564342) {
            return Err(bad());
        }
        let dimensions = bits.read(16).ok_or_else(bad)? as usize;
        let entries = bits.read(24).ok_or_else(bad)? as usize;
        if dimensions == 0 || dimensions > 32 || entries == 0 || entries > 65536 {
            return Err(bad());
        }
        total = total.checked_add(entries * dimensions).ok_or_else(bad)?;
        if total > 1_048_576 {
            return Err(bad());
        }
        if bits.read(1).ok_or_else(bad)? == 1 {
            let mut length = bits.read(5).ok_or_else(bad)? + 1;
            let mut done = 0usize;
            while done < entries {
                if length > 32 {
                    return Err(bad());
                }
                let width = usize::BITS - (entries - done).leading_zeros();
                let n = bits.read(width as usize).ok_or_else(bad)? as usize;
                done = done.checked_add(n).ok_or_else(bad)?;
                if done > entries {
                    return Err(bad());
                }
                length += 1;
            }
        } else {
            let sparse = bits.read(1).ok_or_else(bad)? == 1;
            for _ in 0..entries {
                if !sparse || bits.read(1).ok_or_else(bad)? == 1 {
                    bits.skip(5).ok_or_else(bad)?;
                }
            }
        }
        let lookup = bits.read(4).ok_or_else(bad)?;
        if lookup > 2 {
            return Err(bad());
        }
        if lookup > 0 {
            bits.skip(64).ok_or_else(bad)?;
            let width = bits.read(4).ok_or_else(bad)? as usize + 1;
            bits.skip(1).ok_or_else(bad)?;
            let values = if lookup == 2 {
                entries * dimensions
            } else {
                let (mut lo, mut hi) = (0usize, entries + 1);
                while lo + 1 < hi {
                    let mid = (lo + hi) / 2;
                    let fits = (mid as u128)
                        .checked_pow(dimensions as u32)
                        .is_some_and(|n| n <= entries as u128);
                    if fits { lo = mid } else { hi = mid }
                }
                lo
            };
            bits.skip(values.checked_mul(width).ok_or_else(bad)?)
                .ok_or_else(bad)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("ui7-import-{}", uuid::Uuid::new_v4()))
    }
    fn input_wav(root: &Path, seconds: usize) -> PathBuf {
        std::fs::create_dir_all(root).unwrap();
        let path = root.join("source.wav");
        let mut b = wav_header(RATE * seconds).to_vec();
        b.resize(44 + RATE * seconds * 2, 0);
        std::fs::write(&path, b).unwrap();
        path
    }
    #[test]
    fn selected_wav_is_converted_once_and_survives_original_removal() {
        let root = root();
        let source = input_wav(&root, 1);
        let stop = AtomicBool::new(false);
        let first = import_file(&source, SoundEvent::Message, &root, &[], &stop).unwrap();
        assert!(!first.reused);
        let bytes = std::fs::read(&first.path).unwrap();
        assert_eq!(bytes.len(), RATE * 2 + 44);
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 48000);
        let again = import_file(
            &source,
            SoundEvent::Mention,
            &root,
            &[first.path.clone()],
            &stop,
        )
        .unwrap();
        assert!(again.reused);
        assert_eq!(first.path, again.path);
        std::fs::remove_file(source).unwrap();
        validate_event_file(&again.path, SoundEvent::Message).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn abandoned_import_temporaries_count_towards_budget_without_being_deleted() {
        let root = root();
        let source = input_wav(&root, 1);
        let store = root.join("custom-sounds-v1");
        std::fs::create_dir_all(&store).unwrap();
        let pending = store.join(".import-0123456789abcdef0123456789abcdef.tmp");
        std::fs::File::create(&pending)
            .unwrap()
            .set_len(STORE_LIMIT)
            .unwrap();
        let error = import_file(
            &source,
            SoundEvent::Message,
            &root,
            &[],
            &AtomicBool::new(false),
        )
        .err()
        .unwrap();
        assert!(error.contains("32 MiB"), "{error}");
        assert_eq!(std::fs::metadata(&pending).unwrap().len(), STORE_LIMIT);
        assert_eq!(std::fs::read_dir(&store).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn ring_minute_is_streamed_and_short_events_keep_five_second_limit() {
        let root = root();
        let source = input_wav(&root, 60);
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(&source)
                .unwrap();
            file.seek(SeekFrom::Start(44)).unwrap();
            file.write_all(&1000i16.to_le_bytes()).unwrap();
        }
        let stop = AtomicBool::new(false);
        assert!(import_file(&source, SoundEvent::Message, &root, &[], &stop).is_err());
        let result = import_file(&source, SoundEvent::IncomingRing, &root, &[], &stop).unwrap();
        let mut bank = SoundBank::default();
        let setting = SoundSetting {
            file: Some(result.path.clone()),
            ..Default::default()
        };
        let clip = bank.load(SoundEvent::IncomingRing, &setting).unwrap();
        assert_eq!(clip.bytes(), 0);
        assert_eq!(clip.frames(), RATE * 60);
        assert_eq!(bank.bytes(), 0);
        let mut mixer = SoundMixer::default();
        mixer.ring(Some(clip), 1.);
        let mut chunk = [0; CHUNK];
        let mut marker = [0];
        mixer.render(&mut marker);
        assert_eq!(marker, [1000]);
        let mut remaining = RATE * 62 - 1;
        while remaining > 0 {
            let length = remaining.min(CHUNK);
            mixer.render(&mut chunk[..length]);
            assert!(chunk[..length].iter().all(|s| *s == 0));
            remaining -= length;
        }
        mixer.render(&mut marker);
        assert_eq!(
            marker,
            [1000],
            "A minute of audio and a two-second gap must rewind the same streaming cursor"
        );
        assert!(mixer.active());
        assert!(mixer.take_error().is_none());
        mixer.ring(None, 0.);
        assert!(!mixer.active());
        assert!(validate_event_file(&result.path, SoundEvent::Message).is_err());
        let too_long = input_wav(&root, 61);
        assert!(
            import_file(
                &too_long,
                SoundEvent::OutgoingRing,
                &root,
                &[result.path],
                &stop
            )
            .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unknown_removed_event_does_not_reset_other_preferences() {
        let p:SoundPreferences=serde_json::from_value(serde_json::json!({"events":{"activity_started":{"enabled":false},"activity_ended":{"volume":0.1},"message":{"enabled":false,"volume":0.2}}})).unwrap();
        assert_eq!(p.events.len(), 1);
        assert!(!p.setting(SoundEvent::Message).enabled);
        assert_eq!(p.setting(SoundEvent::Message).volume, 0.2);
        assert_eq!(SoundEvent::ALL.len(), 19);
        assert!(SoundEvent::parse("activity_started").is_none());
    }
    #[test]
    fn cancellation_and_invalid_compressed_inputs_leave_no_partial_assets() {
        let root = root();
        let source = input_wav(&root, 1);
        let stop = AtomicBool::new(true);
        assert!(import_file(&source, SoundEvent::Message, &root, &[], &stop).is_err());
        stop.store(false, Ordering::Release);
        let bad = root.join("bad.ogg");
        std::fs::write(&bad, b"OggS bad file").unwrap();
        assert!(import_file(&bad, SoundEvent::Message, &root, &[], &stop).is_err());
        let imports = root.join("custom-sounds-v1");
        assert!(
            std::fs::read_dir(imports).unwrap().all(|e| e
                .unwrap()
                .path()
                .extension()
                .is_none_or(|e| e != "tmp"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn vorbis_codebook_budget_is_checked_before_decoder_allocation() {
        for b in [vec![], vec![0; 38], vec![0; 256 * 1024 + 1]] {
            assert!(preflight_vorbis(&b).is_err());
        }
        let mut b = vec![0; 50];
        b[..7].copy_from_slice(b"\x01vorbis");
        b[30..37].copy_from_slice(b"\x05vorbis");
        b[37] = 255;
        assert!(preflight_vorbis(&b).is_err());
    }
    #[test]
    fn real_mp3_and_ogg_synthetic_tones_convert_and_are_reused_without_decoder() {
        for (extension, bytes) in [
            (
                "mp3",
                include_bytes!("../../tests/fixtures/audio/synthetic-0.25s.mp3").as_slice(),
            ),
            (
                "ogg",
                include_bytes!("../../tests/fixtures/audio/synthetic-0.25s.ogg").as_slice(),
            ),
        ] {
            let root = root();
            std::fs::create_dir_all(&root).unwrap();
            let source = root.join(format!("tone.{extension}"));
            std::fs::write(&source, bytes).unwrap();
            let stop = AtomicBool::new(false);
            let imported = import_file(&source, SoundEvent::Message, &root, &[], &stop).unwrap();
            let canonical = std::fs::read(&imported.path).unwrap();
            let clip = decode_wav(&canonical).unwrap();
            assert!(
                (RATE / 5..=RATE / 3).contains(&clip.frames()),
                "unexpected frames {}",
                clip.frames()
            );
            assert!(clip.samples.iter().any(|s| s.abs() > 100));
            assert_eq!(&canonical[20..24], &[1, 0, 1, 0]);
            assert_eq!(&canonical[24..28], &(RATE as u32).to_le_bytes());
            let again =
                import_file(&source, SoundEvent::Mention, &root, &[imported.path], &stop).unwrap();
            assert!(again.reused);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn compressed_minute_is_accepted_and_sixty_one_seconds_rejected() {
        let fixtures = [
            (
                "60.mp3",
                include_bytes!("../../tests/fixtures/audio/synthetic-60s.mp3").as_slice(),
                true,
            ),
            (
                "60.ogg",
                include_bytes!("../../tests/fixtures/audio/synthetic-60s.ogg").as_slice(),
                true,
            ),
            (
                "61.mp3",
                include_bytes!("../../tests/fixtures/audio/synthetic-61s.mp3").as_slice(),
                false,
            ),
            (
                "61.ogg",
                include_bytes!("../../tests/fixtures/audio/synthetic-61s.ogg").as_slice(),
                false,
            ),
        ];
        for (name, bytes, allowed) in fixtures {
            let root = root();
            std::fs::create_dir_all(&root).unwrap();
            let source = root.join(name);
            std::fs::write(&source, bytes).unwrap();
            let result = import_file(
                &source,
                SoundEvent::IncomingRing,
                &root,
                &[],
                &AtomicBool::new(false),
            );
            assert_eq!(
                result.is_ok(),
                allowed,
                "{name}: {:?}",
                result.as_ref().err()
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn unsupported_ogg_codec_is_rejected_with_original_untouched() {
        let root = root();
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("opus.ogg");
        let bytes = include_bytes!("../../tests/fixtures/audio/synthetic-opus.ogg");
        std::fs::write(&source, bytes).unwrap();
        assert!(
            import_file(
                &source,
                SoundEvent::Message,
                &root,
                &[],
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(&std::fs::read(&source).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn malformed_metadata_is_rejected_before_any_codec_or_unbounded_allocation() {
        let mut tag = b"ID3\x03\0\0\0\0\0\x14TIT2\xff\xff\xff\xff\0\0".to_vec();
        tag.resize(30, 0);
        tag.extend([0xff, 0xfb, 0x90, 0]);
        assert!(preflight_mp3(&tag).is_err());
        let mut padded = vec![0];
        padded.extend(&tag);
        assert!(preflight_mp3(&padded).is_err());
        let mut comment = vec![0, 0, 0, 0, 1, 0, 0, 0];
        comment.extend(u32::MAX.to_le_bytes());
        assert!(preflight_comments(&comment).is_err());
        let mut packet = b"\x03vorbis".to_vec();
        packet.extend(comment);
        let mut page = vec![0u8; 28];
        page[..4].copy_from_slice(b"OggS");
        page[26] = 1;
        page[27] = packet.len() as u8;
        page.extend(packet);
        assert!(preflight_ogg(&page).is_err());
        assert!(
            preflight_mp3(include_bytes!(
                "../../tests/fixtures/audio/synthetic-0.25s.ogg"
            ))
            .is_err()
        );
        assert!(
            preflight_ogg(include_bytes!(
                "../../tests/fixtures/audio/synthetic-0.25s.mp3"
            ))
            .is_err()
        );
    }
}
