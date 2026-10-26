use super::{read_json, save_json};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
pub struct PickerValue {
    pub value: String,
    pub name: String,
    pub image_url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
pub struct PickerValues {
    pub emoji_recent: Vec<PickerValue>,
    pub gif_favorites: Vec<PickerValue>,
    pub gif_recent: Vec<PickerValue>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub struct GifImportResult {
    pub imported: usize,
    pub existing: usize,
    pub rejected: usize,
}
pub struct PickerHistory {
    path: PathBuf,
    pub data: PickerValues,
}
impl PickerHistory {
    pub fn new(dir: &Path, account: &str) -> Self {
        let scope = format!("{:X}", Sha256::digest(account.as_bytes()));
        let path = dir.join("pickers").join(format!("{}.json", &scope[..24]));
        let raw: serde_json::Value = read_json(&path, 256 * 1024);
        let entries = |key: &str| {
            notdiscord_core::parsing::items(&raw[key])
                .iter()
                .filter_map(|v| serde_json::from_value::<PickerValue>(v.clone()).ok())
                .collect::<Vec<_>>()
        };
        let mut data = PickerValues {
            emoji_recent: entries("EmojiRecent"),
            gif_favorites: entries("GifFavorites"),
            gif_recent: entries("GifRecent"),
        };
        Self::clean(&mut data.emoji_recent, 10, false);
        Self::clean(&mut data.gif_favorites, 100, true);
        Self::clean(&mut data.gif_recent, 32, true);
        Self { path, data }
    }
    pub fn valid(value: &PickerValue, gif: bool) -> bool {
        let length = utf16_len(&value.value);
        length > 0
            && length <= 4096
            && utf16_len(&value.name) <= 160
            && utf16_len(&value.image_url) <= 4096
            && !value.value.chars().any(char::is_control)
            && (if gif {
                crate::pickers::safe_link(&value.value)
            } else {
                length <= 160
            })
            && (value.image_url.is_empty() || crate::pickers::safe_link(&value.image_url))
    }
    fn clean(values: &mut Vec<PickerValue>, limit: usize, gif: bool) {
        let mut known = HashSet::new();
        values.retain(|v| Self::valid(v, gif) && known.insert(v.value.clone()));
        values.truncate(limit)
    }
    pub fn recent(&mut self, value: PickerValue, gif: bool) -> Result<(), String> {
        if !Self::valid(&value, gif) {
            return Ok(());
        }
        let mut updated = self.data.clone();
        let list = if gif {
            &mut updated.gif_recent
        } else {
            &mut updated.emoji_recent
        };
        list.retain(|v| v.value != value.value);
        list.insert(0, value);
        Self::clean(list, if gif { 32 } else { 10 }, gif);
        save_json(&self.path, &updated)?;
        self.data = updated;
        Ok(())
    }
    pub fn favorite(&mut self, value: PickerValue) -> Result<bool, String> {
        if !Self::valid(&value, true) {
            return Err("Enlace GIF no válido.".into());
        }
        let mut updated = self.data.clone();
        let old = updated.gif_favorites.len();
        updated.gif_favorites.retain(|v| v.value != value.value);
        let added = old == updated.gif_favorites.len();
        if added {
            updated.gif_favorites.insert(0, value)
        }
        Self::clean(&mut updated.gif_favorites, 100, true);
        save_json(&self.path, &updated)?;
        self.data = updated;
        Ok(added)
    }
    pub fn import_gif_links(&mut self, text: &str) -> Result<GifImportResult, String> {
        if utf16_len(text) > 256 * 1024 {
            return Err("La lista debe ocupar como máximo 256 KiB.".into());
        }
        let mut result = GifImportResult::default();
        let mut imported = Vec::new();
        let mut known: HashSet<_> = self
            .data
            .gif_favorites
            .iter()
            .map(|v| v.value.clone())
            .collect();
        for line in text.split(['\r', '\n']).filter(|v| !v.is_empty()).take(500) {
            let link = crate::pickers::normalized(line);
            if link.is_empty()
                || !(notdiscord_core::parsing::direct_gif(&link) || crate::pickers::is_page(&link))
            {
                result.rejected += 1;
                continue;
            }
            if !known.insert(link.clone()) {
                result.existing += 1;
                continue;
            }
            if imported.len() + self.data.gif_favorites.len() >= 100 {
                result.rejected += 1;
                continue;
            }
            imported.push(PickerValue {
                value: link.clone(),
                name: "GIF importado".into(),
                image_url: link,
            });
        }
        result.imported = imported.len();
        if !imported.is_empty() {
            let mut updated = self.data.clone();
            imported.extend(updated.gif_favorites);
            updated.gif_favorites = imported;
            save_json(&self.path, &updated)?;
            self.data = updated
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "notdiscord-rust-picker-fixture-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn emoji_recent_is_ten_and_newest_first() {
        let temp = Temp::new();
        let mut p = PickerHistory::new(&temp.0, "1");
        for n in 0..12 {
            p.recent(
                PickerValue {
                    value: format!("emoji{n}"),
                    ..PickerValue::default()
                },
                false,
            )
            .unwrap()
        }
        assert_eq!(p.data.emoji_recent.len(), 10);
        assert_eq!(p.data.emoji_recent[0].value, "emoji11");
        p.recent(
            PickerValue {
                value: "x".repeat(161),
                ..PickerValue::default()
            },
            false,
        )
        .unwrap();
        assert_eq!(p.data.emoji_recent[0].value, "emoji11");
    }
    #[test]
    fn gif_import_validation_and_existing() {
        let temp = Temp::new();
        let mut p = PickerHistory::new(&temp.0, "1");
        let result=p.import_gif_links("https://tenor.com/view/a\nhttps://tenor.com/view/a\nhttps://example.com/x.gif\nhttps://media.tenor.com/b.gif").unwrap();
        assert_eq!(
            result,
            GifImportResult {
                imported: 2,
                existing: 1,
                rejected: 1
            }
        );
        assert_eq!(PickerHistory::new(&temp.0, "1").data.gif_favorites.len(), 2);
    }
    #[test]
    fn invalid_entry_does_not_drop_others() {
        let temp = Temp::new();
        let p = PickerHistory::new(&temp.0, "1");
        save_json(&p.path,&serde_json::json!({"EmojiRecent":[null,{"Value":"😀","Name":"Smile","ImageUrl":""}],"GifFavorites":null})).unwrap();
        let p = PickerHistory::new(&temp.0, "1");
        assert_eq!(p.data.emoji_recent.len(), 1);
    }
    #[test]
    fn image_urls_and_name_length_validated() {
        assert!(!PickerHistory::valid(
            &PickerValue {
                value: "😀".into(),
                name: "x".repeat(161),
                ..PickerValue::default()
            },
            false
        ));
        assert!(!PickerHistory::valid(
            &PickerValue {
                value: "😀".into(),
                image_url: "https://evil.test/a.png".into(),
                ..PickerValue::default()
            },
            false
        ));
    }
}
