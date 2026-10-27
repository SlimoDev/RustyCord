//! Two account-scoped choices in the existing shared navigation preferences.
//! Capture/voice session IDs are deliberately absent from preference identity.
use eframe::egui;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::{Path, PathBuf}};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Visibility {
    private_shown: bool,
    server_shown: bool,
    #[serde(flatten)]
    extensions: BTreeMap<String,serde_json::Value>,
}
impl Default for Visibility {
    fn default() -> Self { Self { private_shown: true, server_shown: false, extensions:BTreeMap::new() } }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Document {
    #[serde(rename = "CallChatVisibility")]
    values: BTreeMap<String, Visibility>,
    #[serde(flatten)]
    extensions: BTreeMap<String,serde_json::Value>,
}
#[derive(Clone)]
struct Cached { directory: PathBuf, account: String, value: (bool,bool) }
fn key(account: &str) -> String { format!("{:x}", Sha256::digest(account.as_bytes()))[..24].to_owned() }
fn cache_id() -> egui::Id { egui::Id::new("account-call-chat-visibility-v1") }
fn load(ctx: &egui::Context, directory: &Path, account: &str) -> (bool,bool) {
    if let Some(cached) = ctx.data(|data|data.get_temp::<Cached>(cache_id()))
        .filter(|cached|cached.directory == directory && cached.account == account) { return cached.value; }
    let document: Document = crate::storage::read_json(&directory.join("navigation.json"), 4 * 1024 * 1024);
    let value = document.values.get(&key(account)).map(|value|(value.private_shown,value.server_shown)).unwrap_or((true,false));
    ctx.data_mut(|data|data.insert_temp(cache_id(),Cached {directory:directory.into(),account:account.into(),value}));
    value
}
pub(super) fn shown(ctx: &egui::Context, directory: &Path, account: &str, server: bool) -> bool {
    let value = if account.is_empty() { (true,false) } else { load(ctx,directory,account) };
    if server { value.1 } else { value.0 }
}
pub(super) fn set_shown(ctx: &egui::Context, directory: &Path, account: &str, server: bool, shown: bool) -> Result<(),String> {
    if !notdiscord_core::parsing::valid_id(account) { return Err("No se pudo identificar la cuenta para guardar la visibilidad del chat.".into()); }
    // Read before writing so changing one account never replaces another one's
    // preference. Shared storage merges this field with unrelated navigation.
    let mut document: Document = crate::storage::read_json_checked(&directory.join("navigation.json"),4 * 1024 * 1024)?;
    let identity = key(account);
    if document.values.len() >= 128 && !document.values.contains_key(&identity) {
        return Err("La configuración de chats de llamada alcanzó su límite de cuentas; no se cambió la elección anterior.".into());
    }
    let value = document.values.entry(identity).or_default();
    if server { value.server_shown = shown; } else { value.private_shown = shown; }
    let updated = (value.private_shown,value.server_shown);
    crate::storage::save_json(&directory.join("navigation.json"), &document)?;
    ctx.data_mut(|data|data.insert_temp(cache_id(),Cached {directory:directory.into(),account:account.into(),value:updated}));
    Ok(())
}
