//! Read-only shared mappings avoid epaint cloning every owned system font.
//! A fixed allowlist and process-lifetime slots keep borrowed font bytes valid.
use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    sync::OnceLock,
};

const PATHS: [&str; 9] = [
    r"C:\Windows\Fonts\segoeui.ttf",
    r"C:\Windows\Fonts\seguisym.ttf",
    r"C:\Windows\Fonts\seguiemj.ttf",
    r"C:\Windows\Fonts\YuGothM.ttc",
    r"C:\Windows\Fonts\YuGothR.ttc",
    r"C:\Windows\Fonts\msgothic.ttc",
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\msjh.ttc",
    r"C:\Windows\Fonts\malgun.ttf",
];
struct FontMapping {
    _file: File,
    bytes: memmap2::Mmap,
}
static MAPPINGS: [OnceLock<Option<FontMapping>>; PATHS.len()] =
    [const { OnceLock::new() }; PATHS.len()];

pub(super) fn load(path: &str) -> Option<eframe::egui::FontData> {
    let index = PATHS.iter().position(|candidate| *candidate == path)?;
    let mapping = MAPPINGS[index].get_or_init(|| {
        // Keep the handle open without WRITE/DELETE sharing. This prevents a
        // font update from mutating or replacing bytes while FontRef borrows them.
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(path)
            .ok()?;
        let len = file.metadata().ok()?.len();
        if !(1..=64 * 1024 * 1024).contains(&len) {
            return None;
        }
        // SAFETY: read-only mapping of an allowlisted Windows system font. The
        // retained handle denies writers/deletion; the OnceLock outlives all UIs.
        let bytes = unsafe { memmap2::Mmap::map(&file) }.ok()?;
        Some(FontMapping { _file: file, bytes })
    });
    if let Some(mapping) = mapping {
        Some(eframe::egui::FontData::from_static(&mapping.bytes))
    } else {
        // A sharing conflict (e.g. an OS font update) must not remove glyphs.
        std::fs::read(path)
            .ok()
            .map(eframe::egui::FontData::from_owned)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_font_bytes_are_borrowed_and_reused_without_loading_other_scripts() {
        let font = load(PATHS[0]).unwrap();
        assert!(matches!(font.font, std::borrow::Cow::Borrowed(_)));
        let again = load(PATHS[0]).unwrap();
        assert_eq!(font.font.as_ptr(), again.font.as_ptr());
        assert!(load(r"C:\untrusted-font.ttf").is_none());
    }
}
