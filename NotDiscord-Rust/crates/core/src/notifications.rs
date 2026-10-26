//! Bounded, account-session notification settings and pure delivery policy.
//!
//! A PATCH acknowledgment retires its transaction without overwriting a valid
//! Gateway update to that same field received after the PATCH began. Even an
//! event containing the existing/requested value counts; unrelated fields and
//! malformed or absent fields do not. One marker per bounded pending PATCH is
//! sufficient because only one transaction per guild may be in flight.
use crate::parsing::{items, s, valid_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct GuildNotifications {
    pub known: bool,
    pub message_notifications: Option<u8>,
    pub suppress_everyone: bool,
    pub suppress_roles: bool,
    pub suppress_everyone_known: bool,
    pub suppress_roles_known: bool,
    pub hide_muted_channels: bool,
    pub pending: bool,
    pub muted: Option<bool>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct MentionFlags {
    pub direct: bool,
    pub everyone: bool,
    pub role: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Delivery {
    pub notify: bool,
    pub mention: bool,
}

struct PendingPatch {
    token: u64,
    patch: Value,
    same_field_observed: bool,
}

fn valid_patch_field(field: &str, value: &Value) -> bool {
    match field {
        "message_notifications" => value.as_u64().is_some_and(|mode| mode <= 2),
        "suppress_everyone" | "suppress_roles" => value.is_boolean(),
        _ => false,
    }
}

#[derive(Default)]
pub struct SettingsStore {
    guilds: HashMap<String, GuildNotifications>,
    channels: HashMap<String, u8>,
    pending: HashMap<String, PendingPatch>,
    next_token: u64,
    hidden: HashSet<String>,
    pub local_account_loaded: String,
}
impl SettingsStore {
    pub fn effective_mode(&self, guild: &str, channel: &str, parents: &[&str]) -> Option<u8> {
        self.channels.get(channel).copied()
            .or_else(|| parents.iter().find_map(|parent| self.channels.get(*parent).copied()))
            .or_else(|| self.guilds.get(guild).and_then(|setting| setting.message_notifications))
    }
    pub fn get(&self, id: &str) -> GuildNotifications {
        let mut setting = self.guilds.get(id).cloned().unwrap_or_default();
        setting.pending = self.pending.contains_key(id);
        setting.hide_muted_channels = self.hidden.contains(id);
        setting
    }
    pub fn apply(&mut self, value: &Value) {
        let id = s(value, "guild_id");
        if valid_id(&id) && (self.guilds.len() < 500 || self.guilds.contains_key(&id)) {
            if let Some(pending) = self.pending.get_mut(&id) {
                let field = pending.patch.as_object().unwrap().keys().next().unwrap();
                if valid_patch_field(field, &value[field.as_str()]) {
                    pending.same_field_observed = true;
                }
            }
            let setting = self.guilds.entry(id).or_default();
            if let Some(mode) = value["message_notifications"]
                .as_u64()
                .filter(|mode| *mode <= 2)
            {
                setting.message_notifications = Some(mode as u8);
                setting.known = true;
            }
            if let Some(flag) = value["suppress_everyone"].as_bool() {
                setting.suppress_everyone = flag;
                setting.suppress_everyone_known = true;
                setting.known = true;
            }
            if let Some(flag) = value["suppress_roles"].as_bool() {
                setting.suppress_roles = flag;
                setting.suppress_roles_known = true;
                setting.known = true;
            }
            if let Some(flag) = value["muted"].as_bool() {
                setting.muted = Some(flag);
            }
        }
        let mut apply_channel = |id: String, setting: &Value| {
            if !valid_id(&id) {
                return;
            }
            if let Some(mode) = setting["message_notifications"]
                .as_u64()
                .filter(|mode| *mode <= 3)
            {
                if mode == 3 {
                    self.channels.remove(&id);
                } else if self.channels.len() < 2000 || self.channels.contains_key(&id) {
                    self.channels.insert(id, mode as u8);
                }
            }
        };
        if let Some(overrides) = value["channel_overrides"].as_object() {
            for (id, setting) in overrides.iter().take(2000) {
                apply_channel(id.clone(), setting);
            }
        } else {
            for setting in items(&value["channel_overrides"]).iter().take(2000) {
                apply_channel(s(setting, "channel_id"), setting);
            }
        }
    }
    pub fn begin_patch(&mut self, id: &str, patch: Value) -> Result<u64, String> {
        if !valid_id(id) || !patch.is_object() || patch.as_object().unwrap().len() != 1 {
            return Err("Cambio de notificación no válido.".into());
        }
        let (field, value) = patch.as_object().unwrap().iter().next().unwrap();
        if !valid_patch_field(field, value) {
            return Err("El ajuste de notificación no está permitido.".into());
        }
        if self.pending.contains_key(id) {
            return Err("Espera la confirmación del cambio anterior de este servidor.".into());
        }
        if self.pending.len() >= 500 {
            return Err("Demasiados ajustes pendientes.".into());
        }
        self.next_token = self.next_token.wrapping_add(1).max(1);
        self.pending.insert(
            id.into(),
            PendingPatch {
                token: self.next_token,
                patch,
                same_field_observed: false,
            },
        );
        Ok(self.next_token)
    }
    /// Never apply the full REST response: an older response could overwrite
    /// another field updated by a newer Gateway event or another client. Even
    /// our exact requested field must yield to a same-field event observed
    /// since begin_patch, including equal-value events and value round trips.
    pub fn finish_patch(&mut self, id: &str, token: u64, successful: bool) -> Option<bool> {
        if self
            .pending
            .get(id)
            .is_none_or(|pending| pending.token != token)
        {
            return None;
        }
        let PendingPatch {
            mut patch,
            same_field_observed,
            ..
        } = self.pending.remove(id).unwrap();
        if successful && !same_field_observed {
            patch["guild_id"] = json!(id);
            self.apply(&patch);
        }
        Some(successful)
    }
    pub fn delivery(
        &self,
        guild: &str,
        channel: &str,
        parent: Option<&str>,
        flags: MentionFlags,
    ) -> Delivery {
        if guild.is_empty() {
            return Delivery {
                notify: true,
                mention: flags.direct,
            };
        }
        let setting = self.get(guild);
        let mode = self.effective_mode(guild, channel, parent.as_slice()).unwrap_or(1);
        let mention = flags.direct
            || flags.everyone && !setting.suppress_everyone
            || flags.role && !setting.suppress_roles;
        Delivery {
            notify: mode == 0 || mode == 1 && mention,
            mention,
        }
    }
    pub fn set_hidden(&mut self, id: &str, hidden: bool) -> Result<(), String> {
        if !valid_id(id) {
            return Err("Servidor no válido.".into());
        }
        if hidden {
            if self.hidden.len() >= 500 && !self.hidden.contains(id) {
                return Err("Demasiados ajustes locales.".into());
            }
            self.hidden.insert(id.into());
        } else {
            self.hidden.remove(id);
        }
        Ok(())
    }
    pub fn hidden_ids(&self) -> Vec<String> {
        let mut ids = self.hidden.iter().cloned().collect::<Vec<_>>();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effective_channel_mode_inherits_categories_and_reset_without_guessing_unknown_settings() {
        let mut store = SettingsStore::default();
        assert_eq!(store.effective_mode("50", "21", &["20", "10"]), None);
        store.apply(&json!({"guild_id":"50","message_notifications":1,"channel_overrides":[
            {"channel_id":"10","message_notifications":2},{"channel_id":"20","message_notifications":0}]}));
        assert_eq!(store.effective_mode("50", "21", &["20", "10"]), Some(0));
        assert_eq!(store.effective_mode("50", "22", &["10"]), Some(2));
        store.apply(&json!({"guild_id":"50","channel_overrides":[{"channel_id":"21","message_notifications":2}]}));
        assert_eq!(store.effective_mode("50", "21", &["20", "10"]), Some(2));
        store.apply(&json!({"guild_id":"50","channel_overrides":[{"channel_id":"21","message_notifications":3}]}));
        assert_eq!(store.effective_mode("50", "21", &["20", "10"]), Some(0));
        assert_eq!(store.effective_mode("50", "23", &[]), Some(1));
    }
    #[test]
    fn modes_and_suppressions_keep_direct_mentions_and_never_turn_mentions_only_into_mute() {
        let mut settings = SettingsStore::default();
        settings.apply(&json!({"guild_id":"10","message_notifications":1,"suppress_everyone":true,"suppress_roles":true}));
        let indirect = MentionFlags {
            everyone: true,
            role: true,
            ..Default::default()
        };
        assert!(!settings.delivery("10", "20", None, indirect).notify);
        assert!(
            settings
                .delivery(
                    "10",
                    "20",
                    None,
                    MentionFlags {
                        direct: true,
                        ..indirect
                    }
                )
                .notify
        );
        settings.apply(&json!({"guild_id":"10","message_notifications":0}));
        assert_eq!(
            settings.delivery("10", "20", None, indirect),
            Delivery {
                notify: true,
                mention: false
            }
        );
        settings.apply(&json!({"guild_id":"10","message_notifications":2}));
        assert!(
            !settings
                .delivery(
                    "10",
                    "20",
                    None,
                    MentionFlags {
                        direct: true,
                        ..indirect
                    }
                )
                .notify
        );
        assert!(
            !settings.get("10").muted.unwrap_or(false),
            "Nothing and server mute are independent settings"
        );
    }
    #[test]
    fn channel_inheritance_and_partial_updates_preserve_unrelated_fields() {
        let mut settings = SettingsStore::default();
        settings.apply(&json!({"guild_id":"10","message_notifications":1,"suppress_everyone":true,"channel_overrides":[{"channel_id":"30","message_notifications":0}]}));
        assert!(
            settings
                .delivery("10", "20", Some("30"), MentionFlags::default())
                .notify
        );
        settings.apply(
            &json!({"guild_id":"10","channel_overrides":{"20":{"message_notifications":2}}}),
        );
        settings.apply(&json!({"guild_id":"10","channel_overrides":[]}));
        settings.apply(&json!({"guild_id":"10","channel_overrides":{"20":{"muted":false}}}));
        assert!(
            !settings
                .delivery(
                    "10",
                    "20",
                    Some("30"),
                    MentionFlags {
                        direct: true,
                        ..Default::default()
                    }
                )
                .notify
        );
        settings.apply(&json!({"guild_id":"10","channel_overrides":[{"channel_id":"20","message_notifications":3}]}));
        assert!(
            settings
                .delivery("10", "20", Some("30"), MentionFlags::default())
                .notify
        );
        assert!(settings.get("10").suppress_everyone);
    }
    #[test]
    fn serialized_minimal_patches_commit_on_success_and_rollback_on_failure() {
        let mut settings = SettingsStore::default();
        settings.apply(&json!({"guild_id":"10","message_notifications":1,"suppress_roles":true}));
        let first = settings
            .begin_patch("10", json!({"message_notifications":0}))
            .unwrap();
        assert!(settings.get("10").pending);
        assert_eq!(settings.get("10").message_notifications, Some(1));
        assert!(
            settings
                .begin_patch("10", json!({"suppress_roles":false}))
                .is_err()
        );
        settings.apply(&json!({"guild_id":"10","suppress_everyone":true}));
        assert_eq!(settings.finish_patch("10", first, true), Some(true));
        assert_eq!(
            settings.get("10").message_notifications,
            Some(0),
            "An unrelated Gateway field must not prevent the requested field from committing"
        );
        assert!(settings.get("10").suppress_everyone && settings.get("10").suppress_roles);
        let second = settings
            .begin_patch("10", json!({"suppress_roles":false}))
            .unwrap();
        assert_eq!(
            settings.finish_patch("10", first, true),
            None,
            "A stale ACK cannot finish a newer patch"
        );
        assert!(settings.get("10").pending);
        assert_eq!(settings.finish_patch("10", second, false), Some(false));
        assert!(settings.get("10").suppress_roles && !settings.get("10").pending);
        assert!(
            settings
                .begin_patch("10", json!({"message_notifications":0,"muted":false}))
                .is_err()
        );
    }
    #[test]
    fn same_field_gateway_update_wins_over_a_later_local_acknowledgment() {
        let mut settings = SettingsStore::default();
        settings.apply(&json!({"guild_id":"10","message_notifications":1}));
        let token = settings
            .begin_patch("10", json!({"message_notifications":0}))
            .unwrap();
        settings.apply(&json!({"guild_id":"10","message_notifications":2}));
        assert_eq!(settings.finish_patch("10", token, true), Some(true));
        assert_eq!(settings.get("10").message_notifications, Some(2));
        assert!(!settings.get("10").pending);

        settings.apply(&json!({"guild_id":"10","suppress_everyone":false,"suppress_roles":false}));
        let token = settings
            .begin_patch("10", json!({"suppress_everyone":true}))
            .unwrap();
        settings.apply(&json!({"guild_id":"10","suppress_everyone":false}));
        settings.finish_patch("10", token, true);
        assert!(
            !settings.get("10").suppress_everyone,
            "An equal-value event is still newer than the PATCH"
        );
        let token = settings
            .begin_patch("10", json!({"suppress_roles":true}))
            .unwrap();
        settings.apply(&json!({"guild_id":"10","suppress_roles":true}));
        settings.apply(&json!({"guild_id":"10","suppress_roles":false}));
        settings.finish_patch("10", token, true);
        assert!(
            !settings.get("10").suppress_roles,
            "A value round trip must not restore a stale requested value"
        );
    }
    #[test]
    fn gateway_echo_of_the_requested_value_finishes_without_replacing_following_events() {
        let mut settings = SettingsStore::default();
        settings.apply(&json!({"guild_id":"10","message_notifications":1}));
        let token = settings
            .begin_patch("10", json!({"message_notifications":0}))
            .unwrap();
        settings.apply(&json!({"guild_id":"10","message_notifications":0}));
        assert_eq!(settings.finish_patch("10", token, true), Some(true));
        assert_eq!(settings.get("10").message_notifications, Some(0));
        assert!(!settings.get("10").pending);

        let token = settings
            .begin_patch("10", json!({"message_notifications":0}))
            .unwrap();
        settings.apply(&json!({"guild_id":"10","message_notifications":0}));
        settings.apply(&json!({"guild_id":"10","message_notifications":2}));
        settings.apply(&json!({"guild_id":"10","message_notifications":1}));
        settings.finish_patch("10", token, true);
        assert_eq!(settings.get("10").message_notifications, Some(1));
    }
    #[test]
    fn invalid_partial_fields_do_not_mutate_settings_or_supersede_pending_patch() {
        let mut settings = SettingsStore::default();
        settings.apply(&json!({"guild_id":"10","message_notifications":1,"suppress_everyone":true,"suppress_roles":true}));
        let token = settings
            .begin_patch("10", json!({"message_notifications":0}))
            .unwrap();
        let before = settings.get("10");
        for partial in [
            json!({"guild_id":"10"}),
            json!({"guild_id":"10","message_notifications":null}),
            json!({"guild_id":"10","message_notifications":3}),
            json!({"guild_id":"10","message_notifications":-1}),
            json!({"guild_id":"10","message_notifications":"0","suppress_everyone":0,"suppress_roles":null}),
            json!({"guild_id":"not-an-id","message_notifications":2}),
        ] {
            settings.apply(&partial);
            assert_eq!(settings.get("10"), before);
        }
        settings.finish_patch("10", token, true);
        assert_eq!(settings.get("10").message_notifications, Some(0));
        assert!(settings.get("10").suppress_everyone && settings.get("10").suppress_roles);

        let token = settings
            .begin_patch("10", json!({"suppress_roles":false}))
            .unwrap();
        settings.apply(&json!({"guild_id":"10","suppress_roles":null,"suppress_everyone":false}));
        settings.finish_patch("10", token, true);
        assert!(!settings.get("10").suppress_roles && !settings.get("10").suppress_everyone);
    }
}
