//! Bounded local event audio. No network, account or capture device is used.
//! A lazy worker submits at most one complete effect and one complete ring.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
#[cfg(test)]
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{CreateEventW, ResetEvent, WaitForMultipleObjects},
};
pub mod import;
mod playback;
mod clip_output;

pub const RATE: usize = 48_000;
pub const CHUNK: usize = 1024;
pub const CACHE_LIMIT: usize = 4 * 1024 * 1024;
pub const FILE_LIMIT: u64 = 2 * 1024 * 1024;
const SAMPLE_LIMIT: usize = RATE * 5;
pub const RING_SECONDS: usize = 60;
pub const RING_FILE_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundEvent {
    IncomingRing,
    OutgoingRing,
    Message,
    Mention,
    VoiceConnected,
    VoiceDisconnected,
    UserJoined,
    UserLeft,
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    StreamStarted,
    StreamEnded,
    StreamViewerJoined,
    StreamViewerLeft,
    PttActivate,
    PttDeactivate,
    UserMove,
}
impl SoundEvent {
    pub const ALL: [Self; 19] = [
        Self::IncomingRing,
        Self::OutgoingRing,
        Self::Message,
        Self::Mention,
        Self::VoiceConnected,
        Self::VoiceDisconnected,
        Self::UserJoined,
        Self::UserLeft,
        Self::Mute,
        Self::Unmute,
        Self::Deafen,
        Self::Undeafen,
        Self::StreamStarted,
        Self::StreamEnded,
        Self::StreamViewerJoined,
        Self::StreamViewerLeft,
        Self::PttActivate,
        Self::PttDeactivate,
        Self::UserMove,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::IncomingRing => "Llamada entrante",
            Self::OutgoingRing => "Llamada saliente",
            Self::Message => "Mensaje / Ping",
            Self::Mention => "Mención directa o de rol",
            Self::VoiceConnected => "Conexión propia a voz",
            Self::VoiceDisconnected => "Desconexión propia de voz",
            Self::UserJoined => "Entra otro participante",
            Self::UserLeft => "Sale otro participante",
            Self::Mute => "Silenciar micrófono",
            Self::Unmute => "Activar micrófono",
            Self::Deafen => "Ensordecer",
            Self::Undeafen => "Quitar ensordecimiento",
            Self::StreamStarted => "Inicia transmisión",
            Self::StreamEnded => "Finaliza transmisión",
            Self::StreamViewerJoined => "Entra un espectador en mi transmisión",
            Self::StreamViewerLeft => "Sale un espectador de mi transmisión",
            Self::PttActivate => "Pulsar para hablar: activar",
            Self::PttDeactivate => "Pulsar para hablar: desactivar",
            Self::UserMove => "Movimiento entre canales",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.into())).ok()
    }
    pub fn is_ring(self) -> bool {
        matches!(self, Self::IncomingRing | Self::OutgoingRing)
    }
    pub fn max_seconds(self) -> usize {
        if self.is_ring() { RING_SECONDS } else { 5 }
    }
    pub fn file_limit(self) -> u64 {
        if self.is_ring() {
            RING_FILE_LIMIT
        } else {
            FILE_LIMIT
        }
    }
}
/// The actor owns invitation lifetimes and the sticky "first peer accepted"
/// decision. Local transport readiness never decides whether recipients ring.
/// Requiring pending recipients also rejects stale/incomplete snapshot flags.
pub fn outgoing_ring_token(state:&notdiscord_core::models::AppSnapshot)->Option<(String,SoundEvent)> {
    use notdiscord_core::parsing::valid_id;
    let owner=state.current_user.as_ref().filter(|person|valid_id(&person.id))?;
    if !state.signed_in || !state.outgoing_call_ringing || !state.call_guild_id.is_empty()
        || !valid_id(&state.call_channel_id)
        || !state.call_pending_invitees.iter().any(|person|valid_id(&person.id)&&person.id!=owner.id)
        || state.call_participants.iter().any(|participant|valid_id(&participant.user.id)&&participant.user.id!=owner.id) {
        return None;
    }
    Some((format!("out:{}:{}:{}:{}",owner.id,state.notification_generation,state.call_channel_id,state.call_instance_id),SoundEvent::OutgoingRing))
}
#[cfg(test)]
mod invitation_ring_tests {
    use super::*;
    use notdiscord_core::models::{AppSnapshot,CallParticipant,Person};
    fn private_call()->AppSnapshot {
        let owner=Person{id:"100".into(),..Default::default()};
        AppSnapshot {signed_in:true,current_user:Some(owner.clone()),notification_generation:7,
            call_channel_id:"200".into(),call_instance_id:4,outgoing_call_ringing:true,
            call_participants:vec![CallParticipant{user:owner,..Default::default()}],
            call_pending_invitees:vec![Person{id:"300".into(),..Default::default()},Person{id:"400".into(),..Default::default()}],
            ..Default::default()}
    }
    #[test]
    fn local_connection_and_duplicate_invitees_keep_one_stable_ring_token() {
        let mut state=private_call();let before=outgoing_ring_token(&state).unwrap();
        assert_eq!(before.1,SoundEvent::OutgoingRing);
        state.voice_connecting=true;assert_eq!(outgoing_ring_token(&state),Some(before.clone()));
        state.voice_connecting=false;state.voice_connected=true;state.voice_encryption_ready=true;
        state.call_pending_invitees.push(state.call_pending_invitees[0].clone());
        assert_eq!(outgoing_ring_token(&state),Some(before));
        // Identity is authoritative: an incomplete local participant flag may
        // not be mistaken for the first recipient joining the call.
        assert!(!state.call_participants[0].is_self);
    }
    #[test]
    fn first_peer_stops_audio_while_other_pending_tiles_remain_and_old_round_never_restarts() {
        let mut state=private_call();assert!(outgoing_ring_token(&state).is_some());
        state.call_pending_invitees.retain(|person|person.id!="300");
        state.call_participants.push(CallParticipant{user:Person{id:"300".into(),..Default::default()},..Default::default()});
        // Also guard a snapshot where participant metadata precedes the actor's
        // sticky flag. The pending recipient remains available for its tile.
        assert!(outgoing_ring_token(&state).is_none());assert_eq!(state.call_pending_invitees[0].id,"400");
        state.outgoing_call_ringing=false;
        state.call_participants.retain(|person|person.user.id=="100");
        assert!(outgoing_ring_token(&state).is_none(),"Peer departure must not restart the old invitation round");
        state.outgoing_call_ringing=true;
        assert!(outgoing_ring_token(&state).is_some(),"Only an actor-confirmed explicit new round may ring again");
    }
    #[test]
    fn expiration_cancellation_and_unconfirmed_invitees_cannot_leave_a_ring_active() {
        let mut state=private_call();state.voice_connected=true;
        state.call_pending_invitees.clear();assert!(outgoing_ring_token(&state).is_none());
        state.call_pending_invitees=vec![state.current_user.clone().unwrap(),Person{id:"invalid".into(),..Default::default()}];
        assert!(outgoing_ring_token(&state).is_none());
        state=private_call();state.outgoing_call_ringing=false;assert!(outgoing_ring_token(&state).is_none());
        state=private_call();state.call_channel_id.clear();assert!(outgoing_ring_token(&state).is_none());
        state=private_call();state.call_guild_id="500".into();assert!(outgoing_ring_token(&state).is_none(),"Server voice has no outgoing private ring");
        state=private_call();state.signed_in=false;assert!(outgoing_ring_token(&state).is_none());
    }
    #[test]
    fn ring_tokens_are_scoped_to_the_account_session_and_call_instance() {
        let mut state=private_call();let initial=outgoing_ring_token(&state).unwrap();
        state.notification_generation+=1;assert_ne!(outgoing_ring_token(&state).unwrap(),initial);
        state.notification_generation-=1;state.call_instance_id+=1;assert_ne!(outgoing_ring_token(&state).unwrap(),initial);
        state.call_instance_id-=1;state.current_user.as_mut().unwrap().id="101".into();state.call_participants.clear();
        assert_ne!(outgoing_ring_token(&state).unwrap(),initial);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SoundSetting {
    pub enabled: bool,
    pub volume: f32,
    pub file: Option<PathBuf>,
    pub source_name: String,
}
impl Default for SoundSetting {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.65,
            file: None,
            source_name: String::new(),
        }
    }
}
#[derive(Clone, Default, Serialize)]
pub struct SoundPreferences {
    pub events: BTreeMap<SoundEvent, SoundSetting>,
}
impl<'de> Deserialize<'de> for SoundPreferences {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Saved {
            events: BTreeMap<String, SoundSetting>,
        }
        let saved = Saved::deserialize(deserializer)?;
        // Removed/unknown events cannot invalidate settings for the remaining events.
        Ok(Self {
            events: saved
                .events
                .into_iter()
                .filter_map(|(name, setting)| {
                    SoundEvent::parse(&name).map(|event| (event, setting))
                })
                .collect(),
        })
    }
}
impl SoundPreferences {
    pub fn setting(&self, event: SoundEvent) -> SoundSetting {
        self.events.get(&event).cloned().unwrap_or_default()
    }
    pub fn load(dir: &Path) -> Self {
        let mut prefs: Self = crate::storage::read_json(&dir.join("sound-settings.json"), 32768);
        prefs.normalize();
        prefs
    }
    fn normalize(&mut self) {
        for setting in self.events.values_mut() {
            setting.source_name = setting
                .source_name
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect();
            setting.volume = if setting.volume.is_finite() {
                setting.volume.clamp(0., 1.)
            } else {
                0.65
            };
            if setting
                .file
                .as_ref()
                .is_some_and(|p| p.as_os_str().len() > 2048)
            {
                setting.file = None;
            }
        }
    }
    pub fn save(&mut self, dir: &Path) -> Result<(), String> {
        self.normalize();
        crate::storage::save_json(&dir.join("sound-settings.json"), self)
    }
}

#[derive(Debug)]
pub struct Clip {
    samples: Box<[i16]>,
    stream: Option<StreamSource>,
}
#[derive(Debug)]
struct StreamSource {
    path: PathBuf,
    frames: usize,
    length: u64,
    modified: Option<std::time::SystemTime>,
}
impl Clip {
    pub fn bytes(&self) -> usize {
        self.samples.len() * 2
    }
    fn frames(&self) -> usize {
        self.stream
            .as_ref()
            .map_or(self.samples.len(), |s| s.frames)
    }
}
/// PCM WAV only: 8/16-bit, mono/stereo, 8..96 kHz, at most five seconds.
/// RIFF/chunk/frame bounds are checked before allocation. Output is 48k mono PCM16.
pub fn decode_wav(bytes: &[u8]) -> Result<Clip, String> {
    decode_wav_limited(bytes, 5, FILE_LIMIT)
}
fn decode_wav_limited(bytes: &[u8], seconds: usize, file_limit: u64) -> Result<Clip, String> {
    let bad = || {
        format!(
            "Usa WAV PCM de 8/16 bits, mono/estéreo, de hasta {seconds} segundos y {} MiB.",
            file_limit / 1024 / 1024
        )
    };
    if bytes.len() > file_limit as usize
        || bytes.len() < 12
        || &bytes[..4] != b"RIFF"
        || &bytes[8..12] != b"WAVE"
    {
        return Err(bad());
    }
    let u16at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let end = (u32at(bytes, 4) as usize)
        .checked_add(8)
        .filter(|e| *e <= bytes.len())
        .ok_or_else(bad)?;
    let (mut format, mut data) = (None, None);
    let mut at = 12usize;
    while at.checked_add(8).is_some_and(|e| e <= end) {
        let len = u32at(bytes, at + 4) as usize;
        let start = at + 8;
        let next = start
            .checked_add(len)
            .filter(|e| *e <= end)
            .ok_or_else(bad)?;
        match &bytes[at..at + 4] {
            b"fmt " if len >= 16 => format = Some(&bytes[start..next]),
            b"data" => data = Some(&bytes[start..next]),
            _ => {}
        }
        at = next.checked_add(len % 2).ok_or_else(bad)?;
    }
    let f = format.ok_or_else(bad)?;
    let d = data.ok_or_else(bad)?;
    let (channels, rate, align, bits) = (
        u16at(f, 2) as usize,
        u32at(f, 4) as usize,
        u16at(f, 12) as usize,
        u16at(f, 14) as usize,
    );
    if u16at(f, 0) != 1
        || !(1..=2).contains(&channels)
        || !(8000..=96000).contains(&rate)
        || ![8, 16].contains(&bits)
        || align != channels * bits / 8
        || d.len() % align != 0
    {
        return Err(bad());
    }
    let frames = d.len() / align;
    if frames == 0 || frames > rate * seconds {
        return Err(bad());
    }
    let count = frames * RATE / rate;
    if count > RATE * seconds {
        return Err(bad());
    }
    let sample = |frame: usize| -> i32 {
        let mut total = 0i32;
        for channel in 0..channels {
            let offset = frame * align + channel * bits / 8;
            total += if bits == 8 {
                (d[offset] as i32 - 128) * 256
            } else {
                i16::from_le_bytes([d[offset], d[offset + 1]]) as i32
            };
        }
        total / channels as i32
    };
    let mut samples = Vec::with_capacity(count);
    for i in 0..count {
        let pos = i * rate;
        let base = (pos / RATE).min(frames - 1);
        let fraction = (pos % RATE) as i64;
        let a = sample(base) as i64;
        let b = sample((base + 1).min(frames - 1)) as i64;
        samples.push((a + (b - a) * fraction / RATE as i64) as i16);
    }
    Ok(Clip {
        samples: samples.into_boxed_slice(),
        stream: None,
    })
}
pub fn validate_file(path: &Path) -> Result<(), String> {
    read_clip(path, SoundEvent::Message).map(|_| ())
}
pub fn validate_event_file(path: &Path, event: SoundEvent) -> Result<(), String> {
    read_clip(path, event).map(|_| ())
}
fn local_sound_path(path: &Path) -> Result<PathBuf, String> {
    use std::path::{Component, Prefix};
    let local = |p: &Path| matches!(p.components().next(),Some(Component::Prefix(prefix)) if matches!(prefix.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)));
    if !local(path) {
        return Err("Selecciona un archivo WAV en un disco local.".into());
    }
    let drive = path
        .components()
        .find_map(|component| match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(d) | Prefix::VerbatimDisk(d) => Some(d),
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    let root = [drive as u16, ':' as u16, '\\' as u16, 0];
    unsafe extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    if !matches!(unsafe { GetDriveTypeW(root.as_ptr()) }, 2 | 3 | 5 | 6) {
        return Err("El reproductor admite archivos de discos locales; las rutas de red no son compatibles.".into());
    }
    let resolved = std::fs::canonicalize(path)
        .map_err(|_| "El archivo de sonido ya no está disponible.".to_owned())?;
    if !local(&resolved) {
        return Err("Selecciona un archivo WAV en un disco local.".into());
    }
    Ok(resolved)
}
fn read_clip(path: &Path, event: SoundEvent) -> Result<Clip, String> {
    use std::io::Read;
    let path = local_sound_path(path)?;
    let mut file =
        std::fs::File::open(&path).map_err(|_| "No se pudo abrir el sonido.".to_owned())?;
    let metadata = file
        .metadata()
        .map_err(|_| "No se pudo consultar el sonido.")?;
    if !metadata.is_file() {
        return Err("Selecciona un archivo WAV normal.".into());
    }
    if metadata.len() > event.file_limit() {
        return Err(format!(
            "El sonido supera {} MiB.",
            event.file_limit() / 1024 / 1024
        ));
    }
    // Imported long rings are canonical PCM. Keep only an open buffered cursor
    // during playback; neither the full file nor its PCM enters the LRU.
    if event.is_ring() && metadata.len() > (SAMPLE_LIMIT * 2 + 44) as u64 {
        let mut header = [0u8; 44];
        file.read_exact(&mut header)
            .map_err(|_| "WAV incompleto.")?;
        if let Some(frames) = canonical_frames(&header, metadata.len(), event.max_seconds()) {
            return Ok(Clip {
                samples: Box::new([]),
                stream: Some(StreamSource {
                    path,
                    frames,
                    length: metadata.len(),
                    modified: metadata.modified().ok(),
                }),
            });
        }
        use std::io::Seek;
        file.rewind().map_err(|_| "No se pudo leer el WAV.")?;
    }
    let mut bytes = Vec::new();
    file.take(event.file_limit() + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "No se pudo leer el sonido.")?;
    let clip = decode_wav_limited(&bytes, event.max_seconds(), event.file_limit())?;
    if clip.samples.len() > SAMPLE_LIMIT {
        return Err("Vuelve a seleccionar el timbre para convertirlo al formato de reproducción por bloques.".into());
    }
    Ok(clip)
}
fn canonical_frames(h: &[u8; 44], length: u64, seconds: usize) -> Option<usize> {
    let u16at = |i| u16::from_le_bytes([h[i], h[i + 1]]);
    let u32at = |i| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
    let bytes = u32at(40) as usize;
    (h[..4] == *b"RIFF"
        && h[8..20] == *b"WAVEfmt \x10\0\0\0"
        && h[36..40] == *b"data"
        && u16at(20) == 1
        && u16at(22) == 1
        && u32at(24) == RATE as u32
        && u32at(28) == (RATE * 2) as u32
        && u16at(32) == 2
        && u16at(34) == 16
        && bytes > 0
        && bytes % 2 == 0
        && bytes / 2 <= RATE * seconds
        && length == bytes as u64 + 44
        && u32at(4) as u64 + 8 == length)
        .then_some(bytes / 2)
}
fn default_clip(event: SoundEvent) -> Clip {
    let ring = event.is_ring();
    let count = if ring { RATE * 8 / 10 } else { RATE * 13 / 100 };
    let index = SoundEvent::ALL
        .iter()
        .position(|e| *e == event)
        .unwrap_or(0);
    let frequency = 320. + index as f32 * 31.;
    let samples = (0..count)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let envelope = (i as f32 / 180.).min(1.) * ((count - i) as f32 / 600.).min(1.);
            let note = if ring && i > count / 2 { 1.25 } else { 1. };
            ((t * frequency * note * std::f32::consts::TAU).sin() * 5000. * envelope) as i16
        })
        .collect::<Vec<_>>();
    Clip {
        samples: samples.into_boxed_slice(),
        stream: None,
    }
}

#[derive(Clone, PartialEq)]
struct CacheKey {
    event: Option<SoundEvent>,
    file: Option<PathBuf>,
    stamp: Option<(u64, Option<std::time::SystemTime>)>,
}
#[derive(Default)]
pub struct SoundBank {
    entries: VecDeque<(CacheKey, Arc<Clip>)>,
    weight: usize,
    pub decodes: u64,
}
impl SoundBank {
    pub fn bytes(&self) -> usize {
        self.weight
    }
    pub fn load(&mut self, event: SoundEvent, setting: &SoundSetting) -> Result<Arc<Clip>, String> {
        let file = setting
            .file
            .as_ref()
            .map(|path| local_sound_path(path))
            .transpose()?;
        let stamp = file
            .as_ref()
            .map(|p| {
                std::fs::metadata(p)
                    .map(|m| (m.len(), m.modified().ok()))
                    .map_err(|_| "El archivo de sonido ya no está disponible.".to_owned())
            })
            .transpose()?;
        let key = CacheKey {
            event: setting.file.is_none().then_some(event),
            file,
            stamp,
        };
        if let Some(index) = self.entries.iter().position(|(k, _)| *k == key) {
            let pair = self.entries.remove(index).unwrap();
            let clip = pair.1.clone();
            self.entries.push_back(pair);
            return Ok(clip);
        }
        let clip = Arc::new(if let Some(path) = &setting.file {
            read_clip(path, event)?
        } else {
            default_clip(event)
        });
        self.decodes += 1;
        if clip.stream.is_some() {
            return Ok(clip);
        }
        while self.weight + clip.bytes() > CACHE_LIMIT {
            if let Some((_, old)) = self.entries.pop_front() {
                self.weight -= old.bytes();
            } else {
                break;
            }
        }
        self.weight += clip.bytes();
        self.entries.push_back((key, clip.clone()));
        Ok(clip)
    }
}
struct Playing {
    clip: Arc<Clip>,
    cursor: usize,
    volume: f32,
    reader: Option<std::io::BufReader<std::fs::File>>,
}
impl Playing {
    fn new(clip: Arc<Clip>, volume: f32) -> Result<Self, String> {
        use std::io::{Seek, SeekFrom};
        let reader = if let Some(source) = &clip.stream {
            let mut file = std::fs::File::open(&source.path)
                .map_err(|_| "El timbre importado ya no está disponible.")?;
            let meta = file
                .metadata()
                .map_err(|_| "No se pudo consultar el timbre.")?;
            if meta.len() != source.length || meta.modified().ok() != source.modified {
                return Err("El timbre cambió. Vuelve a seleccionarlo.".into());
            }
            file.seek(SeekFrom::Start(44))
                .map_err(|_| "No se pudo abrir el timbre.")?;
            Some(std::io::BufReader::with_capacity(8192, file))
        } else {
            None
        };
        Ok(Self {
            clip,
            cursor: 0,
            volume: volume.clamp(0., 1.),
            reader,
        })
    }
    fn next_sample(&mut self) -> Result<i16, String> {
        use std::io::Read;
        let value = if let Some(reader) = &mut self.reader {
            let mut bytes = [0; 2];
            reader
                .read_exact(&mut bytes)
                .map_err(|_| "No se pudo seguir leyendo el timbre.")?;
            i16::from_le_bytes(bytes)
        } else {
            self.clip.samples[self.cursor]
        };
        self.cursor += 1;
        Ok(value)
    }
    fn rewind(&mut self) -> Result<(), String> {
        use std::io::{Seek, SeekFrom};
        if let Some(reader) = &mut self.reader {
            reader
                .seek(SeekFrom::Start(44))
                .map_err(|_| "No se pudo repetir el timbre.")?;
        }
        self.cursor = 0;
        Ok(())
    }
}
// Prepare a complete, bounded PCM clip on the loader thread. The native output
// owns this allocation until playback completes, so imported files never depend
// on a producer meeting a deadline for each individual output block.
fn prepare_clip(
    clip: Arc<Clip>,
    volume: f32,
    default_ring_cadence: bool,
) -> Result<Box<[i16]>, String> {
    use std::io::Read;
    let frames = clip.frames();
    if frames == 0 || frames > RATE * RING_SECONDS {
        return Err("El sonido supera la duración permitida o no contiene audio.".into());
    }
    if !volume.is_finite() {
        return Err("El volumen del sonido no es válido.".into());
    }
    let volume = volume.clamp(0., 1.);
    let padding = if default_ring_cadence { RATE * 2 } else { 0 };
    let mut samples = Vec::with_capacity(frames + padding);
    let mut playing = Playing::new(clip, volume)?;
    if let Some(reader) = &mut playing.reader {
        let mut bytes = [0u8; 8192];
        let mut remaining = frames;
        while remaining > 0 {
            let count = remaining.min(bytes.len() / 2);
            reader
                .read_exact(&mut bytes[..count * 2])
                .map_err(|_| "No se pudo leer por completo el timbre importado.")?;
            samples.extend(bytes[..count * 2].chunks_exact(2).map(|pair| {
                let sample = i16::from_le_bytes([pair[0], pair[1]]);
                (sample as f32 * volume) as i16
            }));
            remaining -= count;
        }
        let source = playing.clip.stream.as_ref().unwrap();
        let metadata = reader
            .get_ref()
            .metadata()
            .map_err(|_| "No se pudo comprobar el timbre importado.")?;
        if metadata.len() != source.length || metadata.modified().ok() != source.modified {
            return Err("El timbre cambió mientras se cargaba. Vuelve a seleccionarlo.".into());
        }
    } else {
        samples.extend(
            playing.clip.samples.iter().map(|sample| (*sample as f32 * volume) as i16),
        );
    }
    // The synthesized ring is a short note followed by silence. Imported
    // ringtones already carry their own timing and loop without added silence.
    samples.resize(frames + padding, 0);
    Ok(samples.into_boxed_slice())
}
#[derive(Default)]
pub struct SoundMixer {
    effect: Option<Playing>,
    ring: Option<Playing>,
    ring_gap: usize,
    error: Option<String>,
}
impl SoundMixer {
    pub fn play(&mut self, clip: Arc<Clip>, volume: f32) {
        self.effect = match Playing::new(clip, volume) {
            Ok(p) => Some(p),
            Err(e) => {
                self.error = Some(e);
                None
            }
        };
    }
    pub fn ring(&mut self, clip: Option<Arc<Clip>>, volume: f32) {
        self.ring = clip.and_then(|clip| match Playing::new(clip, volume) {
            Ok(p) => Some(p),
            Err(e) => {
                self.error = Some(e);
                None
            }
        });
        self.ring_gap = 0;
    }
    pub fn active(&self) -> bool {
        self.effect.is_some() || self.ring.is_some()
    }
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
    pub fn render(&mut self, out: &mut [i16]) {
        for sample in out {
            let mut value = 0f32;
            let mut done = false;
            if let Some(effect) = &mut self.effect {
                match effect.next_sample() {
                    Ok(sample) => value += sample as f32 * effect.volume,
                    Err(e) => {
                        self.error = Some(e);
                        done = true;
                    }
                }
                done |= effect.cursor == effect.clip.frames();
            }
            if done {
                self.effect = None;
            }
            let mut ring_failed = false;
            if let Some(ring) = &mut self.ring {
                if self.ring_gap > 0 {
                    self.ring_gap -= 1;
                } else {
                    match ring.next_sample() {
                        Ok(sample) => value += sample as f32 * ring.volume,
                        Err(e) => {
                            self.error = Some(e);
                            ring_failed = true;
                        }
                    }
                    if ring.cursor == ring.clip.frames() {
                        if let Err(e) = ring.rewind() {
                            self.error = Some(e);
                            ring_failed = true;
                        }
                        self.ring_gap = RATE * 2;
                    }
                }
            }
            if ring_failed {
                self.ring = None;
            }
            *sample = value.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }
    }
}

#[derive(Clone, PartialEq)]
struct Ring {
    token: String,
    event: SoundEvent,
    setting: SoundSetting,
}
enum Command {
    Play(u64, SoundEvent, SoundSetting),
    ClearCache,
}
struct Worker {
    tx: mpsc::SyncSender<Command>,
    ring: Arc<Mutex<Option<Ring>>>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
    generation: Arc<std::sync::atomic::AtomicU64>,
    effect_generation: Arc<std::sync::atomic::AtomicU64>,
    wake_producer: Arc<playback::Wake>,
    thread: Option<std::thread::JoinHandle<()>>,
}
#[derive(Default)]
pub struct SoundEngine {
    worker: Option<Worker>,
    errors: Option<mpsc::Receiver<String>>,
    pub last_error: Option<String>,
    #[cfg(test)]
    pub(crate) observed: Vec<SoundEvent>,
}
impl SoundEngine {
    fn worker(&mut self) -> Option<&Worker> {
        if self.worker.is_none() {
            let (tx, rx) = mpsc::sync_channel(16);
            let (error_tx, errors) = mpsc::sync_channel(4);
            let ring = Arc::new(Mutex::new(None));
            let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let effect_generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let wake_producer = Arc::new(playback::Wake::default());
            let timeline = generation.clone();
            let effects = effect_generation.clone();
            let wake = wake_producer.clone();
            let (r, s) = (ring.clone(), shutdown.clone());
            let thread = match std::thread::Builder::new()
                .name("event-audio".into())
                .spawn(move || playback::run(rx, r, s, timeline, effects, wake, error_tx))
            {
                Ok(thread) => thread,
                Err(_) => {
                    self.last_error = Some("No se pudo iniciar el reproductor de sonidos.".into());
                    return None;
                }
            };
            self.errors = Some(errors);
            self.worker = Some(Worker {
                tx,
                ring,
                shutdown,
                generation,
                effect_generation,
                wake_producer,
                thread: Some(thread),
            });
        }
        self.worker.as_ref()
    }
    pub fn play(&mut self, event: SoundEvent, prefs: &SoundPreferences) {
        let setting = prefs.setting(event);
        if !setting.enabled || setting.volume <= 0. {
            return;
        }
        #[cfg(test)]
        {
            self.observed.push(event);
            return;
        }
        #[cfg(not(test))]
        if let Some(worker) = self.worker() {
            let generation = worker.effect_generation.load(std::sync::atomic::Ordering::Acquire);
            if worker.tx.try_send(Command::Play(generation, event, setting)).is_ok() {
                worker.wake_producer.notify();
            }
        }
    }
    pub fn set_ring(&mut self, token: Option<(String, SoundEvent)>, prefs: &SoundPreferences) {
        if cfg!(test) {
            return;
        }
        let desired = token.and_then(|(token, event)| {
            let setting = prefs.setting(event);
            (setting.enabled && setting.volume > 0.).then_some(Ring {
                token,
                event,
                setting,
            })
        });
        if desired.is_none() && self.worker.is_none() {
            return;
        }
        if let Some(worker) = self.worker() {
            if let Ok(mut ring) = worker.ring.lock() {
                if *ring != desired {
                    *ring = desired;
                    worker.generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    worker.wake_producer.notify();
                }
            }
        }
    }
    pub fn stop_ring(&mut self) {
        if let Some(worker) = &self.worker {
            if let Ok(mut ring) = worker.ring.lock() {
                if ring.take().is_some() {
                    worker.generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    worker.wake_producer.notify();
                }
            }
        }
    }
    pub fn stop_preview(&mut self) {
        if let Some(worker) = &self.worker {
            worker.effect_generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            worker.wake_producer.notify();
        }
    }
    pub fn clear_cache(&mut self) {
        if let Some(worker) = &self.worker {
            let _ = worker.tx.try_send(Command::ClearCache);
            worker.wake_producer.notify();
        }
    }
    pub fn poll_error(&mut self) -> Option<String> {
        if let Some(rx) = &self.errors {
            if let Ok(error) = rx.try_recv() {
                self.last_error = Some(error.clone());
                return Some(error);
            }
        }
        None
    }
}
impl Drop for SoundEngine {
    fn drop(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            // Also invalidate a clip being prepared inside a native open.
            worker.generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            worker.effect_generation.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            worker
                .shutdown
                .store(true, std::sync::atomic::Ordering::Relaxed);
            worker.wake_producer.notify();
            if let Some(thread) = worker.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
// WinMM PCM output: a single lazily opened device, two stable heap buffers, no
// callbacks into Rust. Reset returns both headers before they can be dropped.
#[repr(C, packed(1))]
struct WaveFormat {
    tag: u16,
    channels: u16,
    rate: u32,
    bytes: u32,
    align: u16,
    bits: u16,
    extra: u16,
}
#[repr(C)]
struct WaveHeader {
    data: *mut i16,
    len: u32,
    recorded: u32,
    user: usize,
    flags: u32,
    loops: u32,
    next: *mut WaveHeader,
    reserved: usize,
}
type WaveOpen =
    unsafe extern "system" fn(*mut usize, u32, *const WaveFormat, usize, usize, u32) -> u32;
type WaveHeaderCall = unsafe extern "system" fn(usize, *mut WaveHeader, u32) -> u32;
type WaveHandleCall = unsafe extern "system" fn(usize) -> u32;
struct WinMm {
    _module: usize,
    open: WaveOpen,
    prepare: WaveHeaderCall,
    unprepare: WaveHeaderCall,
    write: WaveHeaderCall,
    reset: WaveHandleCall,
    #[cfg(test)] pause: WaveHandleCall,
    #[cfg(test)] restart: WaveHandleCall,
    close: WaveHandleCall,
}
static WINMM: std::sync::OnceLock<Result<WinMm, String>> = std::sync::OnceLock::new();
fn audio_apis() -> Result<&'static WinMm, String> {
    use windows::{
        Win32::System::LibraryLoader::{
            GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
        },
        core::{PCSTR, w},
    };
    WINMM
        .get_or_init(|| unsafe {
            let module = LoadLibraryExW(w!("winmm.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
                .map_err(|_| "Windows no ofrece el reproductor de eventos.".to_owned())?;
            macro_rules! symbol {
                ($name:literal,$ty:ty) => {
                    match GetProcAddress(module, PCSTR(concat!($name, "\0").as_ptr())) {
                        Some(pointer) => std::mem::transmute::<
                            unsafe extern "system" fn() -> isize,
                            $ty,
                        >(pointer),
                        None => {
                            return Err("Windows no ofrece las funciones PCM requeridas.".into())
                        }
                    }
                };
            }
            Ok(WinMm {
                _module: module.0 as usize,
                open: symbol!("waveOutOpen", WaveOpen),
                prepare: symbol!("waveOutPrepareHeader", WaveHeaderCall),
                unprepare: symbol!("waveOutUnprepareHeader", WaveHeaderCall),
                write: symbol!("waveOutWrite", WaveHeaderCall),
                reset: symbol!("waveOutReset", WaveHandleCall),
                #[cfg(test)] pause: symbol!("waveOutPause", WaveHandleCall),
                #[cfg(test)] restart: symbol!("waveOutRestart", WaveHandleCall),
                close: symbol!("waveOutClose", WaveHandleCall),
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}
#[cfg(test)]
struct WaveBuffer {
    samples: Box<[i16; CHUNK]>,
    header: Box<WaveHeader>,
    prepared: bool,
    queued: bool,
}
#[cfg(test)]
struct NativeOutput {
    apis: &'static WinMm,
    handle: usize,
    wake: Option<HANDLE>,
    buffers: Vec<WaveBuffer>,
    paused: bool,
}
static OUTPUT_QUARANTINED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
impl NativeOutput {
    fn new() -> Result<Self, String> {
        if OUTPUT_QUARANTINED.load(std::sync::atomic::Ordering::Acquire) {
            return Err("El controlador de audio no liberó sus buffers. Reinicia NotDiscord para volver a probar los sonidos.".into());
        }
        let mut handle = 0;
        let apis = audio_apis()?;
        let wake=unsafe {CreateEventW(None,true,false,windows::core::PCWSTR::null())}
            .map_err(|_|"No se pudo crear la señal del reproductor de eventos.")?;
        let format = WaveFormat {
            tag: 1,
            channels: 1,
            rate: RATE as u32,
            bytes: (RATE * 2) as u32,
            align: 2,
            bits: 16,
            extra: 0,
        };
        const CALLBACK_EVENT:u32=0x0005_0000;
        if unsafe { (apis.open)(&mut handle, u32::MAX, &format, wake.0 as usize, 0, CALLBACK_EVENT) } != 0 {
            let _=unsafe {CloseHandle(wake)};
            return Err("No se pudo abrir la salida de sonidos de Windows.".into());
        }
        let mut output = Self {
            apis,
            handle,
            wake:Some(wake),
            buffers: Vec::new(),
            paused: false,
        };
        // waveOutWrite starts playback immediately unless the device is
        // paused first. Prime the existing pair before releasing the clock.
        output.pause_for_refill()?;
        for _ in 0..2 {
            let mut samples = Box::new([0; CHUNK]);
            let mut header = Box::new(WaveHeader {
                data: samples.as_mut_ptr(),
                len: (CHUNK * 2) as u32,
                recorded: 0,
                user: 0,
                flags: 0,
                loops: 0,
                next: std::ptr::null_mut(),
                reserved: 0,
            });
            if unsafe {
                (apis.prepare)(
                    handle,
                    &mut *header,
                    std::mem::size_of::<WaveHeader>() as u32,
                )
            } != 0
            {
                return Err("No se pudo preparar el reproductor de eventos.".into());
            }
            output.buffers.push(WaveBuffer {
                samples,
                header,
                prepared: true,
                queued: false,
            });
        }
        Ok(output)
    }
    fn reset(&mut self) -> Result<(), String> {
        if unsafe { (self.apis.reset)(self.handle) } != 0 {
            return Err("Windows no pudo detener el reproductor de eventos.".into());
        }
        for buffer in &mut self.buffers {
            buffer.queued = false;
        }
        // Reset stops playback but does not establish a paused queue. Pause
        // explicitly before any new write after cancellation or a new ring.
        self.paused = false;
        self.pause_for_refill()
    }
    fn pending_buffers(&self) -> usize {
        self.buffers.iter().filter(|buffer| {
            buffer.queued
                && unsafe { std::ptr::addr_of!(buffer.header.flags).read_volatile() } & 1 == 0
        }).count()
    }
    fn is_playing(&self) -> bool {
        !self.paused
    }
    fn pause_for_refill(&mut self) -> Result<(), String> {
        if !self.paused {
            if unsafe { (self.apis.pause)(self.handle) } != 0 {
                return Err("Windows no pudo preparar la cola de sonidos.".into());
            }
            self.paused = true;
        }
        Ok(())
    }
    fn resume_if_primed(&mut self, more_pcm: bool) -> Result<(), String> {
        let pending = self.pending_buffers();
        // A short final clip can contain only one block; it must not wait for
        // a second block that its producer will never emit.
        if self.paused && (pending == self.buffers.len() || pending > 0 && !more_pcm) {
            if unsafe { (self.apis.restart)(self.handle) } != 0 {
                return Err("Windows no pudo iniciar la cola de sonidos.".into());
            }
            self.paused = false;
        }
        Ok(())
    }
    fn fill(&mut self, mixer: &mut SoundMixer) -> Result<(), String> {
        self.fill_from(|samples| {
            if !mixer.active() {
                return false;
            }
            mixer.render(&mut samples[..]);
            true
        }).map(|_|())
    }
    fn fill_from(&mut self, mut provide: impl FnMut(&mut [i16; CHUNK]) -> bool) -> Result<usize, String> {
        // Clear the manual-reset event BEFORE scanning every WHDR_DONE flag.
        // A completion during this scan must remain signaled for the next wait.
        if let Some(wake)=self.wake {
            unsafe {ResetEvent(wake)}.map_err(|_|"No se pudo reiniciar la señal del reproductor de eventos.")?;
        }
        let mut written = 0;
        for buffer in &mut self.buffers {
            let done = unsafe { std::ptr::addr_of!(buffer.header.flags).read_volatile() } & 1 != 0;
            if buffer.queued && !done {
                continue;
            }
            buffer.queued = false;
            if !provide(&mut buffer.samples) {
                continue;
            }
            if unsafe {
                (self.apis.write)(
                    self.handle,
                    &mut *buffer.header,
                    std::mem::size_of::<WaveHeader>() as u32,
                )
            } != 0
            {
                return Err("Windows no pudo reproducir el sonido de evento.".into());
            }
            buffer.queued = true;
            written += 1;
        }
        Ok(written)
    }
    fn wait(&self, ready: HANDLE)->Result<(),String> {
        let Some(wake)=self.wake else {return Ok(());};
        // PCM publication and cancellation wake this same consumer, instead
        // of waiting for an unrelated driver event or an 8 ms polling tick.
        let result=unsafe {WaitForMultipleObjects(&[wake,ready],false,1000)};
        match result {
            WAIT_TIMEOUT=>Ok(()),
            status if status==WAIT_OBJECT_0 || status.0==WAIT_OBJECT_0.0+1=>Ok(()),
            _=>Err("No se pudo esperar la salida de sonidos de Windows.".into()),
        }
    }
}
#[cfg(test)]
impl Drop for NativeOutput {
    fn drop(&mut self) {
        let _ = self.reset();
        let mut driver_owns_memory = false;
        for mut buffer in self.buffers.drain(..) {
            if buffer.prepared {
                let released = unsafe {
                    (self.apis.unprepare)(
                        self.handle,
                        &mut *buffer.header,
                        std::mem::size_of::<WaveHeader>() as u32,
                    )
                } == 0;
                if !released {
                    // A faulty driver may still own this address. Preserve at
                    // most two buffers plus its handle instead of freeing live DMA memory.
                    driver_owns_memory = true;
                    OUTPUT_QUARANTINED.store(true, std::sync::atomic::Ordering::Release);
                    std::mem::forget(buffer);
                }
            }
        }
        if !driver_owns_memory {
            if unsafe {(self.apis.close)(self.handle)}==0 {
                if let Some(wake)=self.wake.take() {let _=unsafe {CloseHandle(wake)};}
            } else {
                // The device can still signal this event until close succeeds.
                // Quarantine its handle and event together, just like live PCM.
                OUTPUT_QUARANTINED.store(true,std::sync::atomic::Ordering::Release);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn wav(seconds: usize) -> Vec<u8> {
        let data = seconds * 8000 * 2;
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend(((data + 36) as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(8000u32.to_le_bytes());
        b.extend(16000u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((data as u32).to_le_bytes());
        for _ in 0..data / 2 {
            b.extend(1000i16.to_le_bytes());
        }
        b
    }
    #[test]
    fn validates_chunks_duration_format_and_resamples() {
        let clip = decode_wav(&wav(1)).unwrap();
        assert_eq!(clip.samples.len(), RATE);
        assert!(clip.samples.iter().all(|s| *s == 1000));
        assert!(decode_wav(&wav(6)).is_err());
        for n in [0, 1, 11, 12, 19, 25, 43] {
            assert!(decode_wav(&wav(1)[..n]).is_err());
        }
        let mut bad = wav(1);
        bad[20] = 3;
        assert!(decode_wav(&bad).is_err());
        bad = wav(1);
        bad[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_wav(&bad).is_err());
    }
    #[test]
    fn repeated_events_reuse_decoding_and_mix_without_queues() {
        let mut bank = SoundBank::default();
        let clip = bank
            .load(SoundEvent::Message, &SoundSetting::default())
            .unwrap();
        for _ in 0..1000 {
            assert!(Arc::ptr_eq(
                &clip,
                &bank
                    .load(SoundEvent::Message, &SoundSetting::default())
                    .unwrap()
            ));
        }
        assert_eq!(bank.decodes, 1);
        let mut mix = SoundMixer::default();
        for _ in 0..1000 {
            mix.play(clip.clone(), 0.5);
        }
        let mut samples = vec![0; RATE];
        mix.render(&mut samples);
        assert!(!mix.active());
        assert!(samples.iter().any(|s| *s != 0));
        mix.ring(Some(clip), 1.);
        mix.render(&mut samples);
        assert!(mix.active());
        mix.ring(None, 0.);
        assert!(!mix.active());
        mix.render(&mut samples);
        assert!(samples.iter().all(|s| *s == 0));
    }
    #[test]
    fn all_events_have_independent_defaults_and_persistence() {
        let root = std::env::temp_dir().join(format!("sound-settings-{}", uuid::Uuid::new_v4()));
        let mut prefs = SoundPreferences::default();
        for (i, event) in SoundEvent::ALL.iter().enumerate() {
            prefs.events.insert(
                *event,
                SoundSetting {
                    enabled: i % 2 == 0,
                    volume: i as f32 / 20.,
                    file: None,
                    source_name: String::new(),
                },
            );
        }
        prefs.save(&root).unwrap();
        let loaded = SoundPreferences::load(&root);
        assert_eq!(prefs.events, loaded.events);
        let mut bank = SoundBank::default();
        for event in SoundEvent::ALL {
            assert!(bank.load(event, &SoundSetting::default()).unwrap().bytes() > 0);
        }
        assert!(bank.bytes() <= CACHE_LIMIT);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn idle_engine_is_lazy_and_stop_does_not_open_audio() {
        assert!(
            WINMM.get().is_none(),
            "Unit tests never load the native playback API"
        );
        let mut engine = SoundEngine::default();
        engine.stop_ring();
        engine.set_ring(None, &SoundPreferences::default());
        assert!(engine.worker.is_none());
        let mut prefs = SoundPreferences::default();
        prefs.events.insert(
            SoundEvent::Message,
            SoundSetting {
                enabled: false,
                ..Default::default()
            },
        );
        engine.play(SoundEvent::Message, &prefs);
        assert!(engine.worker.is_none());
    }
    #[test]
    fn custom_file_is_shared_between_events_and_pcm8_stereo_is_supported() {
        let root = std::env::temp_dir().join(format!("sound-shared-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("fixture.wav");
        std::fs::write(&path, wav(1)).unwrap();
        let setting = SoundSetting {
            file: Some(path),
            ..Default::default()
        };
        let mut bank = SoundBank::default();
        let first = bank.load(SoundEvent::Message, &setting).unwrap();
        for event in SoundEvent::ALL {
            assert!(Arc::ptr_eq(&first, &bank.load(event, &setting).unwrap()));
        }
        assert_eq!(bank.decodes, 1);
        let mut pcm = wav(1);
        pcm[22..24].copy_from_slice(&2u16.to_le_bytes());
        pcm[34..36].copy_from_slice(&8u16.to_le_bytes());
        for frame in pcm[44..].chunks_exact_mut(2) {
            frame[0] = 128;
            frame[1] = 192;
        }
        let decoded = decode_wav(&pcm).unwrap();
        assert!(decoded.samples.iter().all(|s| *s == 8192));
        assert_eq!(std::mem::size_of::<WaveFormat>(), 18);
        assert_eq!(std::mem::size_of::<WaveHeader>(), 48);
        assert!(local_sound_path(Path::new(r"\\server\sound.wav")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod native_output_protocol_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Clone, Debug, PartialEq)]
    enum Call {
        Write(usize),
        Reset,
        Unprepare(usize),
        Close,
        UnexpectedPrepare,
    }
    #[derive(Default)]
    struct Probe {
        // usize addresses are records only; never access freed allocations.
        addresses: Vec<(usize, usize)>,
        calls: Vec<Call>,
        errors: Vec<&'static str>,
        fail_unprepare: bool,
    }
    static PROBES: OnceLock<Mutex<HashMap<usize, Probe>>> = OnceLock::new();
    static OPEN_CALLS: AtomicUsize = AtomicUsize::new(0);
    fn probes() -> &'static Mutex<HashMap<usize, Probe>> {
        PROBES.get_or_init(|| Mutex::new(HashMap::new()))
    }
    // No assertions/panics across the system ABI: callbacks record failures and
    // the Rust test asserts after each simulated driver call has returned.
    unsafe extern "system" fn fake_open(
        _: *mut usize,
        _: u32,
        _: *const WaveFormat,
        _: usize,
        _: usize,
        _: u32,
    ) -> u32 {
        OPEN_CALLS.fetch_add(1, Ordering::Relaxed);
        5
    }
    unsafe extern "system" fn fake_prepare(handle: usize, _: *mut WaveHeader, _: u32) -> u32 {
        if let Ok(mut all) = probes().lock() {
            if let Some(probe) = all.get_mut(&handle) {
                probe.calls.push(Call::UnexpectedPrepare);
            }
        }
        5
    }
    unsafe extern "system" fn fake_write(handle: usize, header: *mut WaveHeader, size: u32) -> u32 {
        let Ok(mut all) = probes().lock() else {
            return 5;
        };
        let Some(probe) = all.get_mut(&handle) else {
            return 5;
        };
        let Some((_, samples)) = probe
            .addresses
            .iter()
            .find(|(known, _)| *known == header as usize)
            .copied()
        else {
            probe.errors.push("foreign header");
            return 5;
        };
        if size != std::mem::size_of::<WaveHeader>() as u32 {
            probe.errors.push("header ABI size");
            return 5;
        }
        // Registered headers and samples are still owned by output.fill.
        let current = unsafe { &mut *header };
        if current.data as usize != samples
            || current.len != (CHUNK * 2) as u32
            || current.flags & 2 == 0
        {
            probe.errors.push("prepared PCM buffer changed");
            return 5;
        }
        if unsafe { *current.data } != 1500 {
            probe
                .errors
                .push("PCM was not rendered at the selected volume");
        }
        current.flags &= !1; // waveOutWrite clears WHDR_DONE while queued.
        probe.calls.push(Call::Write(header as usize));
        0
    }
    unsafe extern "system" fn fake_reset(handle: usize) -> u32 {
        let Ok(mut all) = probes().lock() else {
            return 5;
        };
        let Some(probe) = all.get_mut(&handle) else {
            return 5;
        };
        probe.calls.push(Call::Reset);
        for (header, _) in &probe.addresses {
            // reset runs before any unprepare/free in NativeOutput::drop.
            unsafe {
                (*(*header as *mut WaveHeader)).flags |= 1;
            }
        }
        0
    }
    unsafe extern "system" fn fake_unprepare(
        handle: usize,
        header: *mut WaveHeader,
        size: u32,
    ) -> u32 {
        let Ok(mut all) = probes().lock() else {
            return 5;
        };
        let Some(probe) = all.get_mut(&handle) else {
            return 5;
        };
        if size != std::mem::size_of::<WaveHeader>() as u32
            || !probe
                .addresses
                .iter()
                .any(|(known, _)| *known == header as usize)
        {
            probe.errors.push("unprepare ABI/address");
            return 5;
        }
        probe.calls.push(Call::Unprepare(header as usize));
        if probe.fail_unprepare {
            return 33;
        } // WAVERR_STILLPLAYING.
        unsafe {
            (*header).flags &= !2;
        }
        0
    }
    unsafe extern "system" fn fake_close(handle: usize) -> u32 {
        let Ok(mut all) = probes().lock() else {
            return 5;
        };
        let Some(probe) = all.get_mut(&handle) else {
            return 5;
        };
        probe.calls.push(Call::Close);
        0
    }
    unsafe extern "system" fn fake_pause_restart(_: usize) -> u32 { 0 }
    static APIS: WinMm = WinMm {
        _module: 0,
        open: fake_open,
        prepare: fake_prepare,
        unprepare: fake_unprepare,
        write: fake_write,
        reset: fake_reset,
        pause: fake_pause_restart,
        restart: fake_pause_restart,
        close: fake_close,
    };

    fn prepared_output(handle: usize, fail_unprepare: bool) -> NativeOutput {
        let mut output = NativeOutput {
            apis: &APIS,
            handle,
            wake:None,
            buffers: Vec::new(),
            paused: true,
        };
        let mut probe = Probe {
            fail_unprepare,
            ..Default::default()
        };
        for _ in 0..2 {
            let mut samples = Box::new([0i16; CHUNK]);
            let mut header = Box::new(WaveHeader {
                data: samples.as_mut_ptr(),
                len: (CHUNK * 2) as u32,
                recorded: 0,
                user: 0,
                flags: 2,
                loops: 0,
                next: std::ptr::null_mut(),
                reserved: 0,
            });
            probe.addresses.push((
                &mut *header as *mut WaveHeader as usize,
                samples.as_mut_ptr() as usize,
            ));
            output.buffers.push(WaveBuffer {
                samples,
                header,
                prepared: true,
                queued: false,
            });
        }
        probes().lock().unwrap().insert(handle, probe);
        output
    }
    struct RestoreQuarantine(bool);
    impl Drop for RestoreQuarantine {
        fn drop(&mut self) {
            OUTPUT_QUARANTINED.store(self.0, Ordering::Release);
        }
    }

    #[test]
    fn native_output_fake_driver_reuses_two_buffers_cancels_and_quarantines_failed_release() {
        // One test owns both handles and the quarantine flag. Other sound unit
        // tests never initialize playback, so no actual driver or DLL is used.
        let _restore = RestoreQuarantine(OUTPUT_QUARANTINED.swap(false, Ordering::AcqRel));
        let normal = 0x4e44_0001usize;
        let failed = 0x4e44_0002usize;
        let mut output = prepared_output(normal, false);
        let addresses = probes().lock().unwrap()[&normal].addresses.clone();
        let mut mixer = SoundMixer::default();
        mixer.ring(
            Some(Arc::new(Clip {
                samples: vec![3000; CHUNK * 5].into_boxed_slice(),
                stream: None,
            })),
            0.5,
        );
        output.fill(&mut mixer).unwrap();
        assert!(output.buffers.iter().all(|buffer| buffer.queued));
        assert_eq!(
            probes().lock().unwrap()[&normal].calls,
            vec![Call::Write(addresses[0].0), Call::Write(addresses[1].0)]
        );
        output.fill(&mut mixer).unwrap();
        assert_eq!(
            probes().lock().unwrap()[&normal].calls.len(),
            2,
            "busy buffers must not be submitted twice"
        );
        output.buffers[0].header.flags |= 1; // Driver completed only buffer zero.
        output.fill(&mut mixer).unwrap();
        assert_eq!(
            probes().lock().unwrap()[&normal].calls.last(),
            Some(&Call::Write(addresses[0].0))
        );
        for (buffer, (header, samples)) in output.buffers.iter().zip(&addresses) {
            assert_eq!((&*buffer.header) as *const WaveHeader as usize, *header);
            assert_eq!(buffer.samples.as_ptr() as usize, *samples);
        }
        output.reset().unwrap();
        assert!(output.buffers.iter().all(|buffer| !buffer.queued));
        mixer.ring(None, 0.);
        output.fill(&mut mixer).unwrap();
        assert_eq!(
            probes().lock().unwrap()[&normal]
                .calls
                .iter()
                .filter(|call| matches!(call, Call::Write(_)))
                .count(),
            3
        );
        drop(output);
        let probe = probes().lock().unwrap().remove(&normal).unwrap();
        assert!(probe.errors.is_empty(), "{:?}", probe.errors);
        assert_eq!(
            probe.calls,
            vec![
                Call::Write(addresses[0].0),
                Call::Write(addresses[1].0),
                Call::Write(addresses[0].0),
                Call::Reset,
                Call::Reset,
                Call::Unprepare(addresses[0].0),
                Call::Unprepare(addresses[1].0),
                Call::Close
            ]
        );
        assert!(!OUTPUT_QUARANTINED.load(Ordering::Acquire));

        let output = prepared_output(failed, true);
        let addresses = probes().lock().unwrap()[&failed].addresses.clone();
        let retained_pcm_and_headers = output.buffers.len()
            * (CHUNK * std::mem::size_of::<i16>() + std::mem::size_of::<WaveHeader>());
        assert_eq!(output.buffers.len(), 2);
        assert_eq!(retained_pcm_and_headers, 2 * (2048 + 48));
        drop(output);
        let probe = probes().lock().unwrap().remove(&failed).unwrap();
        assert!(probe.errors.is_empty(), "{:?}", probe.errors);
        assert_eq!(
            probe.calls,
            vec![
                Call::Reset,
                Call::Unprepare(addresses[0].0),
                Call::Unprepare(addresses[1].0)
            ]
        );
        assert!(OUTPUT_QUARANTINED.load(Ordering::Acquire));
        // Failed unprepare enters the production mem::forget branch: two PCM
        // allocations plus two headers are retained, and close is not called.
        // Their recorded addresses are never dereferenced after drop, and this
        // test deliberately does not free memory a driver claims to own.
        assert_ne!(addresses[0].0, addresses[1].0);
        assert_ne!(addresses[0].1, addresses[1].1);
        assert!(
            !probe
                .calls
                .iter()
                .any(|call| matches!(call, Call::Close | Call::UnexpectedPrepare))
        );
        assert_eq!(
            OPEN_CALLS.load(Ordering::Relaxed),
            0,
            "constructing the fake output must never call waveOutOpen"
        );
        assert!(
            WINMM.get().is_none(),
            "fake protocol tests never load WinMM or open speakers"
        );
    }
}
