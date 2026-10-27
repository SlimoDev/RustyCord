use super::{read_json_checked, save_json};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}
const MAX_HISTORY_BYTES:u64=1024*1024;
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
pub struct PickerValue {
    pub value: String,
    pub name: String,
    pub image_url: String,
    pub animation_url: String,
    pub width: i32,
    pub height: i32,
}
impl PickerValue {
    pub fn descriptor(&self) -> notdiscord_core::parsing::MediaDescriptor {
        notdiscord_core::parsing::media_descriptor(&notdiscord_core::models::ChatAttachment {
            url:self.value.clone(),preview_url:(!self.image_url.is_empty()).then(||self.image_url.clone()),
            animation_url:(!self.animation_url.is_empty()).then(||self.animation_url.clone()),
            width:self.width,height:self.height,animated:true,..Default::default()
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all="PascalCase")]
pub struct RemoteRemoval { pub url:String, pub confirmed:bool }
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
pub struct PickerValues {
    pub emoji_recent: Vec<PickerValue>,
    pub gif_favorites: Vec<PickerValue>,
    pub gif_recent: Vec<PickerValue>,
    pub gif_remote_removals: Vec<RemoteRemoval>,
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
    load_error:Option<String>,
}
impl PickerHistory {
    pub fn new(dir: &Path, account: &str) -> Self {
        let scope = format!("{:X}", Sha256::digest(account.as_bytes()));
        let path = dir.join("pickers").join(format!("{}.json", &scope[..24]));
        let (raw,mut load_error)=match read_json_checked::<serde_json::Value>(&path,MAX_HISTORY_BYTES){
            Ok(value)=>(value,None),Err(error)=>(serde_json::Value::Null,Some(error)),
        };
        if !raw.is_null()&&!raw.is_object(){load_error=Some("El historial tiene una estructura no compatible".into());}
        let removals=match raw.get("GifRemoteRemovals").filter(|value|!value.is_null()){
            None=>Vec::new(),
            Some(value)=>match serde_json::from_value::<Vec<RemoteRemoval>>(value.clone()){
                Ok(values)if values.len()<=100&&values.iter().all(|entry|entry.url.len()<=4096&&crate::pickers::safe_link(&entry.url))=>values,
                _=>{load_error=Some("El estado de conciliación de favoritos no es válido; no se modificará".into());Vec::new()},
            },
        };
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
            gif_remote_removals: removals,
        };
        Self::clean(&mut data.emoji_recent, 10, false);
        Self::clean(&mut data.gif_favorites, 100, true);
        Self::clean(&mut data.gif_recent, 32, true);
        data.gif_remote_removals.retain(|value|crate::pickers::safe_link(&value.url));
        data.gif_remote_removals.truncate(100);
        let removed=&data.gif_remote_removals;
        data.gif_favorites.retain(|value|!removed.iter().any(|entry|entry.confirmed&&value.descriptor().matches_url(&entry.url)));
        Self { path, data, load_error }
    }
    pub fn load_error(&self)->Option<&str>{self.load_error.as_deref()}
    pub fn valid(value: &PickerValue, gif: bool) -> bool {
        let length = utf16_len(&value.value);
        length > 0
            && length <= 4096
            && utf16_len(&value.name) <= 160
            && utf16_len(&value.image_url) <= 4096
            && utf16_len(&value.animation_url) <= 4096
            && !value.value.chars().any(char::is_control)
            && (if gif {
                crate::pickers::safe_link(&value.value)
            } else {
                length <= 160
            })
            && (value.image_url.is_empty() || crate::pickers::safe_link(&value.image_url))
            && (value.animation_url.is_empty() || crate::pickers::safe_link(&value.animation_url))
    }
    fn clean(values: &mut Vec<PickerValue>, limit: usize, gif: bool) {
        let mut known = HashSet::new();
        values.retain(|v| Self::valid(v, gif) && known.insert(v.descriptor().logical_id));
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
        self.commit(updated)
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
        self.commit(updated)?;
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
                ..Default::default()
            });
        }
        result.imported = imported.len();
        if !imported.is_empty() {
            let mut updated = self.data.clone();
            imported.extend(updated.gif_favorites);
            updated.gif_favorites = imported;
            self.commit(updated)?;
        }
        Ok(result)
    }
    fn commit(&mut self, candidate:PickerValues) -> Result<(),String> {
        if let Some(error)=&self.load_error{return Err(format!("No se modificó el historial porque no se pudo leer su estado anterior: {error}"));}
        if serde_json::to_vec(&candidate).map_err(|_|"Historial no válido")?.len()as u64>MAX_HISTORY_BYTES {
            return Err("El historial de GIFs supera1MiB. El estado confirmado se conserva.".into());
        }
        save_json(&self.path,&candidate)?;self.data=candidate;Ok(())
    }
    pub fn set_favorite(&mut self,value:PickerValue,saved:bool)->Result<(),String>{
        if !Self::valid(&value,true){return Err("Enlace GIF no válido".into());}
        let mut candidate=self.data.clone();
        let descriptor=value.descriptor();
        candidate.gif_favorites.retain(|old|!old.descriptor().same_medium(&descriptor));
        if saved {candidate.gif_favorites.insert(0,value);candidate.gif_favorites.truncate(100);}
        self.commit(candidate)
    }
    pub fn import_values(&mut self,values:Vec<PickerValue>)->Result<(),String>{
        let mut candidate=self.data.clone();
        for value in values.into_iter().take(100){
            if !Self::valid(&value,true){continue;}
            if candidate.gif_remote_removals.iter().any(|entry|entry.confirmed&&value.descriptor().matches_url(&entry.url)){continue;}
            if candidate.gif_favorites.len()<100&&!candidate.gif_favorites.iter().any(|old|old.descriptor().same_medium(&value.descriptor())){candidate.gif_favorites.push(value);}
        }
        self.commit(candidate)
    }
    /// Persist intent before the remote write. Only an authoritative read can
    /// confirm it; a crash/timeout never turns into an automatic repeat PATCH.
    pub fn prepare_remote_removal(&mut self,url:&str)->Result<(),String>{
        if !crate::pickers::safe_link(url){return Err("Enlace GIF no válido".into());}
        let mut candidate=self.data.clone();
        if let Some(existing)=candidate.gif_remote_removals.iter_mut().find(|entry|entry.url==url){existing.confirmed=false;}
        else {if candidate.gif_remote_removals.len()>=100{return Err("Hay demasiadas conciliaciones de favoritos pendientes".into());}candidate.gif_remote_removals.push(RemoteRemoval{url:url.into(),confirmed:false});}
        self.commit(candidate)
    }
    pub fn reconcile_remote(&mut self,encoded:&str)->Result<(),String>{
        let mut candidate=self.data.clone();
        for removal in &mut candidate.gif_remote_removals {
            removal.confirmed=!crate::pickers::favorites_wire::contains(encoded,&removal.url)?;
        }
        let observed_removals=candidate.gif_remote_removals.clone();
        candidate.gif_favorites.retain(|value|!candidate.gif_remote_removals.iter().any(|entry|entry.confirmed&&value.descriptor().matches_url(&entry.url)));
        // Once removal and local cleanup are in the same durable candidate,
        // no stale UI snapshot can resurrect it: the controller is sole writer.
        candidate.gif_remote_removals.retain(|entry|!entry.confirmed);
        match self.commit(candidate){Ok(())=>Ok(()),Err(error)=>{
            // Remote absence has been observed even if disk cleanup fails.
            // Keep that separate fact in memory so the effective UI does not
            // reintroduce the stale local copy. The durable prepared intent
            // causes another read/reconciliation next time, never a blind PATCH.
            self.data.gif_remote_removals=observed_removals;Err(error)
        }}
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
    #[test]
    fn corrupt_removal_intent_is_read_only_and_never_replaced_by_empty_history(){
        let temp=Temp::new();let original=PickerHistory::new(&temp.0,"1");
        let bytes=br#"{"GifRemoteRemovals":[{"Url":"https://tenor.com/view/synthetic","Confirmed":"invalid"}]}"#;
        fs::create_dir_all(original.path.parent().unwrap()).unwrap();fs::write(&original.path,bytes).unwrap();
        let mut loaded=PickerHistory::new(&temp.0,"1");assert!(loaded.load_error().is_some());
        assert!(loaded.prepare_remote_removal("https://tenor.com/view/new").is_err());
        assert_eq!(fs::read(&original.path).unwrap(),bytes);
    }
}
