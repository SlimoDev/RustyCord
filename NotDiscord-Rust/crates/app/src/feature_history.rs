use super::{atomic_write, protect, read_bounded};
use notdiscord_core::models::ChatMessage;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
    sync::Mutex,
};
use zeroize::Zeroizing;

// Bounds describe retained message payloads, not measured process memory.
const CHANNEL_LIMIT: usize = 2 * 1024 * 1024;
const TOTAL_LIMIT: usize = 8 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_RECORDS: usize = 4096;

struct LoggedChange {
    snapshot: ChatMessage,
}
struct SessionHistory {
    records: VecDeque<LoggedChange>,
    channel_bytes: HashMap<String, usize>,
    bytes: usize,
    channel_limit: usize,
    total_limit: usize,
    record_limit: usize,
}
impl Default for SessionHistory {
    fn default() -> Self {
        Self { records: VecDeque::new(), channel_bytes: HashMap::new(), bytes: 0,
            channel_limit: CHANNEL_LIMIT, total_limit: TOTAL_LIMIT, record_limit: MAX_RECORDS }
    }
}
pub struct Features {
    dir: PathBuf,
    pub pins: Vec<String>,
    history: Mutex<SessionHistory>,
}
impl Features {
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&dir).map_err(|_| "No se pudo crear la carpeta de preferencias.")?;
        let mut pins = Vec::new();
        let pin_path = dir.join("pins.dpapi");
        if pin_path.exists() {
            let bytes = read_bounded(&pin_path, 2 * 1024 * 1024)
                .ok_or("Fijados locales demasiado grandes o no disponibles.")?;
            let plain = Zeroizing::new(protect(&bytes, true, false)?);
            let loaded = serde_json::from_slice::<Vec<String>>(&plain)
                .map_err(|_| "Fijados locales no válidos.")?;
            let mut seen = HashSet::new();
            pins = loaded.into_iter().filter(|s| !s.trim().is_empty() && seen.insert(s.clone()))
                .take(1000).collect();
        }
        // Legacy message-history journals are deliberately neither opened nor
        // changed. Only this application's current session populates the log.
        Ok(Self { dir, pins, history: Mutex::new(SessionHistory::default()) })
    }
    pub fn toggle_pin(&mut self, id: &str) -> Result<(), String> {
        if id.trim().is_empty() { return Err("Conversación no válida.".into()); }
        let mut updated = self.pins.clone();
        if updated.iter().any(|p| p == id) { updated.retain(|p| p != id); }
        else {
            if updated.len() >= 1000 { return Err("Límite de 1000 conversaciones fijadas.".into()); }
            updated.push(id.into());
        }
        let data = Zeroizing::new(serde_json::to_vec(&updated).map_err(|_| "Fijados no válidos.")?);
        atomic_write(&self.dir.join("pins.dpapi"), &protect(&data, true, true)?)?;
        self.pins = updated;
        Ok(())
    }
    pub fn record(&self, previous: &ChatMessage, deleted: bool) -> Result<(), String> {
        let mut snapshot = previous.clone();
        // Earlier observed versions have separate session entries. Do not
        // recursively duplicate the inline expansion for every change.
        snapshot.previous_contents.clear();
        snapshot.deleted = deleted;
        snapshot.edited = previous.edited || !deleted;
        let serialized = Zeroizing::new(serde_json::to_vec(&snapshot).map_err(|_| "Historial no válido.")?);
        if serialized.len() > MAX_RECORD_BYTES {
            return Err("El mensaje supera el límite de 1 MiB del registro de sesión; se conserva el historial existente.".into());
        }
        let weight = serialized.len().saturating_add(std::mem::size_of::<LoggedChange>());
        let mut history = self.history.lock().map_err(|_| "No se pudo bloquear el historial de sesión.")?;
        if history.records.iter().rev().find(|record|record.snapshot.channel_id==snapshot.channel_id&&record.snapshot.id==snapshot.id)
            .is_some_and(|record|record.snapshot==snapshot) {return Ok(());}
        let channel_bytes = history.channel_bytes.get(&snapshot.channel_id).copied().unwrap_or(0);
        if history.records.len() >= history.record_limit
            || history.bytes.saturating_add(weight) > history.total_limit
            || channel_bytes.saturating_add(weight) > history.channel_limit {
            return Err("El historial de esta sesión alcanzó su límite (2 MiB por canal, 8 MiB o 4096 cambios en total). Se conserva lo registrado sin añadir nuevas entradas.".into());
        }
        history.channel_bytes.insert(snapshot.channel_id.clone(), channel_bytes + weight);
        history.bytes += weight;
        history.records.push_back(LoggedChange { snapshot });
        Ok(())
    }
    pub fn history(&self, channel: &str, limit: usize) -> Result<Vec<ChatMessage>, String> {
        if !(1..=1000).contains(&limit) { return Err("El límite debe estar entre 1 y 1000.".into()); }
        let history = self.history.lock().map_err(|_| "No se pudo bloquear el historial de sesión.")?;
        let mut result = history.records.iter().rev().filter(|record| record.snapshot.channel_id == channel)
            .take(limit).map(|record| record.snapshot.clone()).collect::<Vec<_>>();
        result.reverse();
        Ok(result)
    }
    /// Restore only deletion markers inside the loaded interval. The separate
    /// History view still exposes all retained observations; a deleted row
    /// outside this window must not fabricate a pagination/scroll frontier.
    pub fn restore_session(&self,channel:&str,messages:Vec<ChatMessage>)->Vec<ChatMessage> {
        let history=self.history.lock().unwrap_or_else(|error|error.into_inner());
        let order=|a:&ChatMessage,b:&ChatMessage|a.timestamp.cmp(&b.timestamp).then(a.id.len().cmp(&b.id.len())).then(a.id.cmp(&b.id));
        let bounds=messages.iter().filter(|message|message.channel_id==channel).min_by(|a,b|order(a,b))
            .zip(messages.iter().filter(|message|message.channel_id==channel).max_by(|a,b|order(a,b)));
        let in_window=|snapshot:&ChatMessage|bounds.is_some_and(|(first,last)|!order(snapshot,first).is_lt()&&!order(snapshot,last).is_gt());
        let visible=messages.iter().filter(|message|message.channel_id==channel).map(|message|message.id.as_str()).collect::<HashSet<_>>();
        let reply_ids=messages.iter().filter(|message|message.channel_id==channel)
            .filter_map(|message|message.reply_to.as_deref()).collect::<HashSet<_>>();
        let mut known:HashMap<&str,(Option<&ChatMessage>,Vec<&str>)>=HashMap::new();
        for record in history.records.iter().filter(|record|record.snapshot.channel_id==channel) {
            let snapshot=&record.snapshot;
            if !visible.contains(snapshot.id.as_str())&&!reply_ids.contains(snapshot.id.as_str())
                && !(snapshot.deleted&&in_window(snapshot)) {continue;}
            let entry=known.entry(&snapshot.id).or_default();
            if snapshot.deleted {entry.0=Some(snapshot);} else {
                let content=snapshot.content.as_str();
                if entry.1.last().copied()!=Some(content) {
                    if entry.1.len()>=8 {entry.1.remove(0);}
                    entry.1.push(content);
                }
            }
        }
        let deleted=known.iter().filter_map(|(id,(snapshot,_))|snapshot.filter(|_|reply_ids.contains(*id)).map(|message|(
            (*id).to_owned(),(message.author.clone(),notdiscord_core::parsing::message_preview(message),message.mentions.clone()))))
            .collect::<HashMap<_,_>>();
        let retained_deletions=known.iter().filter_map(|(id,(snapshot,_))|snapshot.filter(|snapshot|visible.contains(*id)||in_window(snapshot)).map(|_|(*id).to_owned()))
            .collect::<HashSet<_>>();
        let mut combined:HashMap<String,ChatMessage>=messages.into_iter().filter(|message|message.channel_id==channel)
            .map(|message|(message.id.clone(),message)).collect();
        for (id,(deleted,versions)) in known {
            if let Some(snapshot)=deleted.filter(|_|retained_deletions.contains(id)) {
                let mut snapshot=snapshot.clone();snapshot.pinned=false;snapshot.reactions.clear();
                combined.insert(id.to_owned(),snapshot);
            }
            if let Some(message)=combined.get_mut(id) {
                let mut previous=versions.into_iter().map(|text|text.chars().take(4000).collect::<String>()).collect::<Vec<_>>();
                for version in &message.previous_contents {
                    if !previous.iter().any(|known|known==version) {previous.push(version.chars().take(4000).collect());}
                }
                if previous.len()>8 {previous.drain(..previous.len()-8);}
                message.previous_contents=previous;message.edited|=!message.previous_contents.is_empty();
            }
        }
        let mut result=combined.into_values().collect::<Vec<_>>();
        for message in &mut result {
            if let Some(id)=message.reply_to.as_ref()
                && message.reply_preview.as_ref().is_none_or(|preview|preview.channel_id==channel)
                && let Some((author,content,mentions))=deleted.get(id) {
                let preview=message.reply_preview.get_or_insert_with(||notdiscord_core::models::MessageReplyPreview {
                    message_id:id.clone(),channel_id:channel.into(),..Default::default()
                });
                preview.deleted=true;preview.author=Some(author.clone());preview.content=content.clone();preview.mentions=mentions.clone();preview.unavailable=false;
            }
        }
        result.sort_by(|a,b|a.timestamp.cmp(&b.timestamp).then(a.id.len().cmp(&b.id.len())).then(a.id.cmp(&b.id)));
        result
    }
    /// Session replacement and shutdown release messages and spare capacity.
    /// Conversation pins and all historical files remain untouched.
    pub fn clear_history(&self) {
        *self.history.lock().unwrap_or_else(|error| error.into_inner()) = SessionHistory::default();
    }
}

// Retained solely for existing offline legacy-file integrity tests. Production
// session history has no journal parser, repair path or filesystem writer.
#[cfg(test)]
pub(super) fn validate_tail(file: &mut std::fs::File, repair: bool) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(0)).map_err(|_| "Historial no válido.")?;
    let len = file.metadata().map_err(|_| "Historial no válido.")?.len();
    loop {
        let start = file.stream_position().map_err(|_| "Historial no válido.")?;
        if start == len { return Ok(()); }
        if len - start < 4 {
            if repair { file.set_len(start).map_err(|_| "No se pudo reparar cola.")?; }
            return Ok(());
        }
        let mut bytes = [0u8; 4];
        file.read_exact(&mut bytes).map_err(|_| "Historial no válido.")?;
        let size = i32::from_le_bytes(bytes);
        if size <= 0 || size > MAX_RECORD_BYTES as i32 + 65536 {
            return Err("Registro de historial dañado; se conserva para recuperación.".into());
        }
        if len - start - 4 < size as u64 {
            if repair { file.set_len(start).map_err(|_| "No se pudo reparar cola.")?; }
            return Ok(());
        }
        file.seek(SeekFrom::Current(size as i64)).map_err(|_| "Historial no válido.")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("notdiscord-rust-history-fixture-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap(); Self(path)
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    fn message(id: &str, channel: &str) -> ChatMessage {
        ChatMessage { id: id.into(), channel_id: channel.into(), content: "Synthetic test text".into(), ..ChatMessage::default() }
    }
    #[test]
    fn known_edits_and_deleted_rich_payloads_restore_after_a_fresh_server_window_without_duplicates() {
        let temp=Temp::new();let features=Features::new(temp.0.clone()).unwrap();
        let mut original=message("10","20");original.author.id="30".into();original.reply_to=Some("8".into());
        original.attachments.push(notdiscord_core::models::ChatAttachment{id:"40".into(),url:"https://cdn.discordapp.com/attachments/1/2/known.png".into(),..Default::default()});
        features.record(&original,false).unwrap();
        let mut edited=original.clone();edited.content="Known edited text".into();edited.edited=true;
        features.record(&edited,true).unwrap();features.record(&edited,true).unwrap();
        let mut reply=message("11","20");reply.reply_to=Some("10".into());reply.reply_preview=Some(notdiscord_core::models::MessageReplyPreview{message_id:"10".into(),channel_id:"20".into(),deleted:true,..Default::default()});
        let restored=features.restore_session("20",vec![original.clone(),reply.clone(),reply]);
        assert_eq!(restored.len(),2);let deleted=&restored[0];assert!(deleted.deleted&&deleted.edited&&!deleted.pinned);
        assert_eq!(deleted.content,"Known edited text");assert_eq!(deleted.previous_contents,vec!["Synthetic test text"]);
        assert_eq!(deleted.author.id,"30");assert_eq!(deleted.reply_to.as_deref(),Some("8"));assert_eq!(deleted.attachments,edited.attachments);
        let preview=restored[1].reply_preview.as_ref().unwrap();assert!(preview.deleted);assert_eq!(preview.author.as_ref().unwrap().id,"30");
        assert!(features.restore_session("21",vec![]).is_empty());assert_eq!(features.history("20",100).unwrap().len(),2);
        assert!(fs::read_dir(&temp.0).unwrap().next().is_none());
    }
    #[test]
    fn old_versions_survive_reconstruction_focus_refresh_and_session_log_limits_without_inventing_a_current_row() {
        let temp=Temp::new();let features=Features::new(temp.0.clone()).unwrap();
        for version in 0..12 {let mut old=message("1","20");old.content=format!("observed {version}");features.record(&old,false).unwrap();}
        assert!(features.restore_session("20",vec![]).is_empty(),"old versions alone cannot fabricate the current message");
        let mut fresh=message("1","20");fresh.content="current received".into();
        for _ in 0..3 {let restored=features.restore_session("20",vec![fresh.clone()]);assert_eq!(restored[0].previous_contents.len(),8);assert_eq!(restored[0].previous_contents[0],"observed 4");assert_eq!(restored[0].previous_contents[7],"observed 11");}
        {let mut history=features.history.lock().unwrap();history.record_limit=history.records.len();}
        assert!(features.record(&message("2","20"),true).is_err());
        assert_eq!(features.restore_session("20",vec![fresh.clone()])[0].previous_contents.len(),8);
        features.clear_history();assert!(features.restore_session("20",vec![fresh])[0].previous_contents.is_empty());
    }
    #[test]
    fn session_log_has_no_journal_or_policy_files_and_is_empty_after_reopening() {
        let temp = Temp::new();
        let features = Features::new(temp.0.clone()).unwrap();
        features.record(&message("1", "20"), false).unwrap();
        features.record(&message("2", "20"), true).unwrap();
        assert_eq!(features.history("20", 100).unwrap().len(), 2);
        assert!(fs::read_dir(&temp.0).unwrap().next().is_none());
        assert!(Features::new(temp.0.clone()).unwrap().history("20", 100).unwrap().is_empty());
    }
    #[test]
    fn legacy_files_are_never_imported_repaired_or_modified() {
        let temp = Temp::new();
        let legacy = temp.0.join("message-history"); fs::create_dir(&legacy).unwrap();
        let file = legacy.join("old-encrypted-record.ndj");
        let bytes = b"Synthetic legacy bytes including an incomplete frame";
        fs::write(&file, bytes).unwrap();
        let features = Features::new(temp.0.clone()).unwrap();
        assert!(features.history("20", 100).unwrap().is_empty());
        features.record(&message("1", "20"), false).unwrap();
        features.clear_history();
        assert!(features.history("20", 100).unwrap().is_empty());
        assert_eq!(fs::read(file).unwrap(), bytes);
        assert_eq!(fs::read_dir(legacy).unwrap().count(), 1);
    }
    #[test]
    fn changes_retain_order_channel_identity_and_metadata_without_recursive_versions() {
        let temp = Temp::new(); let features = Features::new(temp.0.clone()).unwrap();
        for n in 0..5 {
            let mut value = message(&n.to_string(), "20");
            value.author.id = "30".into(); value.previous_contents.push("Do not duplicate".into());
            features.record(&value, n == 4).unwrap();
            features.record(&message(&n.to_string(), "21"), false).unwrap();
        }
        let history = features.history("20", 2).unwrap();
        assert_eq!(history.iter().map(|message| message.id.as_str()).collect::<Vec<_>>(), ["3", "4"]);
        assert!(history[0].edited && !history[0].deleted);
        assert!(history[1].deleted && !history[1].edited);
        assert_eq!(history[1].author.id, "30");
        assert!(history.iter().all(|message| message.previous_contents.is_empty()));
        assert!(features.history("20", 0).is_err()); assert!(features.history("20", 1001).is_err());
        assert!(features.history("unknown", 10).unwrap().is_empty());
        features.clear_history();
        assert!(features.history("20", 100).unwrap().is_empty());
        let history = features.history.lock().unwrap();
        assert_eq!((history.bytes, history.records.capacity(), history.channel_bytes.capacity()), (0, 0, 0));
    }
    #[test]
    fn bounded_session_log_stops_without_discarding_existing_changes() {
        for kind in ["records", "channel", "total"] {
            let temp = Temp::new(); let features = Features::new(temp.0.clone()).unwrap();
            features.record(&message("1", "20"), false).unwrap();
            {
                let mut history = features.history.lock().unwrap();
                match kind { "records" => history.record_limit = 1, "channel" => history.channel_limit = history.bytes,
                    _ => history.total_limit = history.bytes }
            }
            assert!(features.record(&message("2", "20"), true).is_err());
            let retained = features.history("20", 10).unwrap();
            assert_eq!(retained.len(), 1); assert_eq!(retained[0].id, "1");
        }
    }
    #[test]
    fn oversized_record_keeps_session_history() {
        let temp = Temp::new(); let features = Features::new(temp.0.clone()).unwrap();
        features.record(&message("1", "2"), false).unwrap();
        let mut large = message("3", "2"); large.content = "x".repeat(MAX_RECORD_BYTES + 1);
        assert!(features.record(&large, false).is_err());
        assert_eq!(features.history("2", 10).unwrap().len(), 1);
    }
    #[test]
    fn conversation_pins_remain_persistent_and_deduplicated() {
        let temp = Temp::new(); let raw = Zeroizing::new(br#"["1","2","1",""]"#.to_vec());
        atomic_write(&temp.0.join("pins.dpapi"), &protect(&raw, true, true).unwrap()).unwrap();
        let mut features = Features::new(temp.0.clone()).unwrap(); assert_eq!(features.pins, ["1", "2"]);
        features.record(&message("1", "20"), false).unwrap(); features.clear_history();
        assert_eq!(features.pins, ["1", "2"]);
        features.toggle_pin("1").unwrap();
        assert_eq!(Features::new(temp.0.clone()).unwrap().pins, ["2"]);
    }
}
