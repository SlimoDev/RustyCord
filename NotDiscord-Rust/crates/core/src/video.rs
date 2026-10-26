//! Bounded raw video transport. Pixels never enter JSON snapshots or disk caches.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub const HEADER_LEN: usize = 36;
pub const MAX_STREAMS: usize = 4;
pub const MAX_WIDTH: u32 = 1280;
pub const MAX_HEIGHT: u32 = 720;
pub const MAX_FRAME_BYTES: usize = MAX_WIDTH as usize * MAX_HEIGHT as usize * 4;
pub const MAX_BUFFERED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub subscription_id: u64,
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub len: u32,
}
impl FrameHeader {
    pub fn validate(&self) -> Result<(), String> {
        if self.subscription_id == 0
            || self.sequence == 0
            || self.width == 0
            || self.height == 0
            || self.width > MAX_WIDTH
            || self.height > MAX_HEIGHT
        {
            return Err("Fotograma de vídeo fuera de los límites permitidos.".into());
        }
        // Check dimensions before multiplication, including hostile u32::MAX
        // headers; validation must never overflow before rejecting the frame.
        let expected = self.width as u64 * self.height as u64 * 4;
        if expected != self.len as u64 || expected > MAX_FRAME_BYTES as u64 {
            return Err("Fotograma de vídeo fuera de los límites permitidos.".into());
        }
        Ok(())
    }
    pub fn encode_header(&self) -> [u8; HEADER_LEN] {
        let mut bytes = [0u8; HEADER_LEN];
        bytes[0..4].copy_from_slice(b"NDVF");
        bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.subscription_id.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.sequence.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.width.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.height.to_le_bytes());
        bytes[32..36].copy_from_slice(&self.len.to_le_bytes());
        bytes
    }
}
pub fn decode_header(bytes: &[u8; HEADER_LEN]) -> Result<FrameHeader, String> {
    if &bytes[0..4] != b"NDVF" || bytes[4..8] != [1, 0, 0, 0] {
        return Err("Cabecera de vídeo no compatible.".into());
    }
    let header = FrameHeader {
        subscription_id: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        sequence: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        width: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        height: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
        len: u32::from_le_bytes(bytes[32..36].try_into().unwrap()),
    };
    header.validate()?;
    Ok(header)
}
#[derive(Debug)]
pub struct VideoFrame {
    pub subscription_id: u64,
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl VideoFrame {
    pub fn header(&self) -> Result<FrameHeader, String> {
        let header = FrameHeader {
            subscription_id: self.subscription_id,
            sequence: self.sequence,
            width: self.width,
            height: self.height,
            len: self
                .rgba
                .len()
                .try_into()
                .map_err(|_| "Fotograma demasiado grande.")?,
        };
        header.validate()?;
        Ok(header)
    }
}
#[derive(Default)]
struct Slot {
    sequence: u64,
    frame: Option<VideoFrame>,
}
#[derive(Default)]
pub struct LatestFrames {
    slots: HashMap<u64, Slot>,
    bytes: usize,
}
pub type SharedVideoFrames = Arc<Mutex<LatestFrames>>;
impl LatestFrames {
    pub fn activate(&mut self, id: u64) -> bool {
        if id == 0 || (!self.slots.contains_key(&id) && self.slots.len() >= MAX_STREAMS) {
            return false;
        }
        self.slots.entry(id).or_default();
        true
    }
    pub fn remove(&mut self, id: u64) {
        if let Some(slot) = self.slots.remove(&id) {
            self.bytes -= slot.frame.map_or(0, |frame| frame.rgba.len());
        }
    }
    pub fn clear(&mut self) {
        self.slots.clear();
        self.bytes = 0;
    }
    pub fn active_ids(&self) -> Vec<u64> {
        self.slots.keys().copied().collect()
    }
    pub fn buffered_bytes(&self) -> usize {
        self.bytes
    }
    pub fn publish(&mut self, frame: VideoFrame) -> bool {
        if frame.header().is_err() {
            return false;
        }
        let Some(slot) = self.slots.get_mut(&frame.subscription_id) else {
            return false;
        };
        if frame.sequence <= slot.sequence {
            return false;
        }
        let used = self.bytes - slot.frame.as_ref().map_or(0, |old| old.rgba.len());
        if frame.rgba.len() > MAX_BUFFERED_BYTES - used {
            return false;
        }
        self.bytes = used + frame.rgba.len();
        slot.sequence = frame.sequence;
        slot.frame = Some(frame);
        true
    }
    pub fn drain(&mut self) -> Vec<VideoFrame> {
        self.bytes = 0;
        self.slots
            .values_mut()
            .filter_map(|slot| slot.frame.take())
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frame(id: u64, sequence: u64) -> VideoFrame {
        VideoFrame {
            subscription_id: id,
            sequence,
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
        }
    }
    #[test]
    fn binary_roundtrip_and_bad_lengths_are_rejected_before_body_allocation() {
        let header = frame(8, 2).header().unwrap();
        assert_eq!(decode_header(&header.encode_header()).unwrap(), header);
        for invalid in [
            FrameHeader { len: 9, ..header },
            FrameHeader {
                width: u32::MAX,
                height: u32::MAX,
                ..header
            },
            FrameHeader {
                sequence: 0,
                ..header
            },
            FrameHeader {
                subscription_id: 0,
                ..header
            },
        ] {
            assert!(decode_header(&invalid.encode_header()).is_err());
        }
        let mut bad = header.encode_header();
        bad[6] = 1;
        assert!(decode_header(&bad).is_err());
    }
    #[test]
    fn latest_slot_drops_backlog_and_rejects_stale_and_detached_frames() {
        let mut store = LatestFrames::default();
        assert!(!store.publish(frame(1, 1)));
        assert!(store.activate(1));
        for sequence in 1..=1000 {
            assert!(store.publish(frame(1, sequence)));
        }
        assert_eq!(store.buffered_bytes(), 8);
        assert!(!store.publish(frame(1, 999)));
        let out = store.drain();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sequence, 1000);
        assert_eq!(store.buffered_bytes(), 0);
        assert!(!store.publish(frame(1, 1000)));
        store.remove(1);
        assert!(!store.publish(frame(1, 1001)));
    }
    #[test]
    fn stream_count_and_total_raw_memory_are_bounded() {
        let mut store = LatestFrames::default();
        for id in 1..=MAX_STREAMS as u64 {
            assert!(store.activate(id));
        }
        assert!(!store.activate(5));
        for id in 1..=4 {
            let _ = store.publish(VideoFrame {
                subscription_id: id,
                sequence: 1,
                width: MAX_WIDTH,
                height: MAX_HEIGHT,
                rgba: vec![0; MAX_FRAME_BYTES],
            });
        }
        assert!(store.buffered_bytes() <= MAX_BUFFERED_BYTES);
        assert_eq!(store.drain().len(), 2);
        store.clear();
        assert!(store.active_ids().is_empty());
        assert_eq!(store.buffered_bytes(), 0);
    }
}
