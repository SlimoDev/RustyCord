use crate::models::{ChannelAccess, GuildRole, NavItem};
use crate::parsing::{b, i, items, s};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct GuildAccess {
    pub guild_id: String,
    pub user_id: String,
    pub base_permissions: u64,
    pub roles: Vec<GuildRole>,
    pub timed_out: bool,
    role_ids: HashSet<String>,
    everyone_views: bool,
}
pub fn read_bits(item: &Value, key: &str) -> u64 {
    item.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
fn member_list_hash(input: &[u8]) -> u32 {
    // The original ChannelMemberStore hashes sorted allow:/deny: VIEW_CHANNEL
    // rules with v3. The 32-bit, seed-zero variant is corroborated by the
    // Litecord-derived lazy guild documentation; no target code is executed.
    let mut hash=0u32;
    let mut chunks=input.chunks_exact(4);
    for chunk in &mut chunks {
        let mut block=u32::from_le_bytes([chunk[0],chunk[1],chunk[2],chunk[3]]);
        block=block.wrapping_mul(0xcc9e2d51).rotate_left(15).wrapping_mul(0x1b873593);
        hash=(hash^block).rotate_left(13).wrapping_mul(5).wrapping_add(0xe6546b64);
    }
    let mut tail=0u32;
    for (index,byte) in chunks.remainder().iter().enumerate(){tail|=(*byte as u32)<<(index*8);}
    if !chunks.remainder().is_empty(){hash^=tail.wrapping_mul(0xcc9e2d51).rotate_left(15).wrapping_mul(0x1b873593);}
    hash^=input.len() as u32;
    hash^=hash>>16;hash=hash.wrapping_mul(0x85ebca6b);
    hash^=hash>>13;hash=hash.wrapping_mul(0xc2b2ae35);
    hash^(hash>>16)
}
impl GuildAccess {
    pub fn has_role(&self, id: &str) -> bool {
        !id.is_empty() && self.role_ids.contains(id)
    }
    pub fn from_values(guild: &Value, member: &Value, user_id: &str) -> Self {
        Self::from_values_at(guild, member, user_id, chrono::Utc::now())
    }
    pub fn from_values_at(
        guild: &Value,
        member: &Value,
        user_id: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        let guild_id = s(guild, "id");
        if guild_id.is_empty() || user_id.is_empty() {
            return Self {
                guild_id,
                user_id: user_id.into(),
                ..Self::default()
            };
        }
        let role_ids: HashSet<_> = items(&member["roles"])
            .iter()
            .filter_map(Value::as_str)
            .take(500)
            .map(String::from)
            .collect();
        let mut permissions = 0;
        if s(guild, "owner_id") == user_id {
            permissions = u64::MAX;
        } else {
            for r in items(&guild["roles"]).iter().take(1000) {
                if s(r, "id") == guild_id || role_ids.contains(&s(r, "id")) {
                    permissions |= read_bits(r, "permissions")
                }
            }
        }
        if permissions & 8 != 0 {
            permissions = u64::MAX;
        }
        let mut roles: Vec<_> = items(&guild["roles"])
            .iter()
            .take(1000)
            .map(|r| GuildRole {
                id: s(r, "id"),
                name: s(r, "name"),
                position: i(r, "position"),
                color: i(r, "color"),
                hoist: b(r, "hoist"),
            })
            .collect();
        roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.id.cmp(&b.id)));
        let timed_out =
            chrono::DateTime::parse_from_rfc3339(&s(member, "communication_disabled_until"))
                .is_ok_and(|v| v > now)
                && permissions != u64::MAX;
        let everyone_views = items(&guild["roles"])
            .iter()
            .any(|r| s(r, "id") == guild_id && read_bits(r, "permissions") & 1024 != 0);
        Self {
            guild_id,
            user_id: user_id.into(),
            base_permissions: permissions,
            roles,
            timed_out,
            role_ids,
            everyone_views,
        }
    }
    pub fn channel_permissions(&self, channel: &Value) -> u64 {
        if self.guild_id.is_empty() || self.user_id.is_empty() {
            return 0;
        }
        if self.base_permissions == u64::MAX {
            return u64::MAX;
        }
        let mut p = self.base_permissions;
        let (mut role_allow, mut role_deny, mut own_allow, mut own_deny) = (0, 0, 0, 0);
        for o in items(&channel["permission_overwrites"]).iter().take(1000) {
            let id = s(o, "id");
            let Some(kind) = o.get("type").and_then(Value::as_i64) else {
                continue;
            };
            let allow = read_bits(o, "allow");
            let deny = read_bits(o, "deny");
            if kind == 0 && id == self.guild_id {
                p = (p & !deny) | allow
            } else if kind == 0 && self.role_ids.contains(&id) {
                role_allow |= allow;
                role_deny |= deny
            } else if kind == 1 && id == self.user_id {
                own_allow |= allow;
                own_deny |= deny
            }
        }
        p = (((p & !role_deny) | role_allow) & !own_deny) | own_allow;
        if self.timed_out {
            p & ((1 << 10) | (1 << 16))
        } else {
            p
        }
    }
    pub fn evaluate(&self, channel: &Value, parent: Option<&Value>) -> ChannelAccess {
        let kind = i(channel, "type");
        let thread = matches!(kind, 10 | 11 | 12);
        let effective = if thread {
            match parent {
                Some(v) if v.is_object() => v,
                _ => return ChannelAccess::default(),
            }
        } else {
            channel
        };
        if self.base_permissions != u64::MAX
            && effective
                .get("permission_overwrites")
                .is_some_and(|v| !v.is_array())
        {
            return ChannelAccess::default();
        }
        let bits = self.channel_permissions(effective);
        let mut view = bits & (1 << 10) != 0;
        let admin = self.base_permissions == u64::MAX;
        let manage_threads = bits & (1 << 34) != 0;
        if kind == 12
            && !admin
            && !manage_threads
            && s(channel, "owner_id") != self.user_id
            && !channel.get("member").is_some_and(Value::is_object)
        {
            view = false
        }
        let mut send = view && bits & (1u64 << if thread { 38 } else { 11 }) != 0;
        if thread
            && (b(&channel["thread_metadata"], "archived")
                || b(&channel["thread_metadata"], "locked"))
            && !manage_threads
            && !admin
        {
            send = false
        }
        let history = view && bits & (1 << 16) != 0;
        let reason = if !view {
            "No tienes permiso para ver este canal."
        } else if self.timed_out {
            "No puedes escribir ni reaccionar mientras se aplica la restricción temporal del servidor."
        } else if !send {
            "No tienes permiso para enviar mensajes en este canal."
        } else {
            ""
        };
        ChannelAccess {
            known: true,
            can_view: view,
            can_read_history: history,
            can_send: send,
            can_attach: send && bits & (1 << 15) != 0,
            can_react: history && bits & (1 << 6) != 0,
            can_view_members: view,
            reason: reason.into(),
            can_embed: send && bits & (1 << 14) != 0,
            can_mention_everyone: send && bits & (1 << 17) != 0,
        }
    }
    pub fn confirmed_member_list_id(&self, channel: &Value) -> String {
        let Some(kind) = channel.get("type").and_then(Value::as_i64) else {
            return String::new();
        };
        let explicit = s(channel, "member_list_id");
        if !explicit.is_empty() {
            return explicit;
        }
        if !matches!(kind, 0 | 2 | 4 | 5 | 13 | 15 | 16)
            || channel
                .get("permission_overwrites")
                .is_some_and(|v| !v.is_array())
        {
            return String::new();
        }
        let overwrites=items(&channel["permission_overwrites"]);
        if overwrites.len()>1000{return String::new();}
        let mut seen=HashSet::new();let mut rules=Vec::new();let mut any_deny=false;
        for overwrite in overwrites {
            let id=s(overwrite,"id");
            if !matches!(overwrite.get("type").and_then(Value::as_i64),Some(0|1))
                ||!crate::parsing::valid_id(&id)||!seen.insert(id.clone()) {return String::new();}
            let bits=|key:&str|match overwrite.get(key) {
                None=>Some(0),Some(Value::String(value)) if !value.is_empty()&&value.bytes().all(|byte|byte.is_ascii_digit())=>value.parse::<u64>().ok(),
                _=>None,
            };
            let (Some(allow),Some(deny))=(bits("allow"),bits("deny"))else{return String::new();};
            any_deny|=deny&1024!=0;
            if allow&1024!=0 {rules.push(format!("allow:{id}"));}
            else if deny&1024!=0 {rules.push(format!("deny:{id}"));}
        }
        if self.everyone_views&&!any_deny {return "everyone".into();}
        // A missing overwrite array cannot establish a private list binding.
        if !channel["permission_overwrites"].is_array(){return String::new();}
        rules.sort();
        member_list_hash(rules.join(",").as_bytes()).to_string()
    }
    pub fn decorate(&self, channel: &Value, parent: Option<&Value>) -> NavItem {
        let mut n = crate::parsing::channel(channel);
        let bits = self.channel_permissions(if matches!(i(channel, "type"), 10 | 11 | 12) {
            parent.unwrap_or(&Value::Null)
        } else {
            channel
        });
        n.can_invite = bits & 1025 == 1025;
        n.can_manage = bits & 1024 != 0 && bits & 16 != 0;
        n.can_pin = bits & 1024 != 0 && bits & (1u64 << 51) != 0;
        n.access = Some(self.evaluate(channel, parent));
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn base(bits: u64) -> Value {
        json!({"id":"10","owner_id":"99","roles":[{"id":"10","permissions":bits.to_string()}]})
    }
    #[test]
    fn missing_data_never_grants() {
        let a = GuildAccess::from_values(&Value::Null, &Value::Null, "");
        assert!(!a.evaluate(&json!({"type":0}), None).can_view)
    }
    #[test]
    fn everyone_mentions_use_effective_permissions_and_never_default_to_private_chat_grants() {
        let bits = (1 << 10) | (1 << 11) | (1 << 17);
        let guild = base(bits);
        let acl = GuildAccess::from_values(&guild, &json!({}), "1");
        assert!(acl.evaluate(&json!({"type":0}), None).can_mention_everyone);
        assert!(!acl.evaluate(&json!({"type":0,"permission_overwrites":[{"id":"1","type":1,"deny":(1u64 << 17).to_string(),"allow":"0"}]}), None).can_mention_everyone);
        assert!(!ChannelAccess::private().can_mention_everyone);
        assert!(!ChannelAccess::default().can_mention_everyone);
        let timeout = GuildAccess::from_values_at(&guild, &json!({"communication_disabled_until":"2030-01-01T00:00:00Z"}), "1", chrono::DateTime::from_timestamp(0, 0).unwrap());
        assert!(!timeout.evaluate(&json!({"type":0}), None).can_mention_everyone);
    }
    #[test]
    fn member_overwrite_wins_roles() {
        let g = json!({"id":"10","roles":[{"id":"10","permissions":"1024"},{"id":"20","permissions":"2048"}]});
        let m = json!({"roles":["20"]});
        let a = GuildAccess::from_values(&g, &m, "1");
        let c = json!({"type":0,"permission_overwrites":[{"id":"20","type":0,"deny":"2048","allow":"0"},{"id":"1","type":1,"allow":"2048","deny":"0"}]});
        assert!(a.evaluate(&c, None).can_send)
    }
    #[test]
    fn owner_admin_bypass() {
        let a = GuildAccess::from_values(&base(8), &json!({}), "1");
        assert_eq!(
            a.channel_permissions(
                &json!({"permission_overwrites":[{"id":"1","type":1,"deny":"1024"}]})
            ),
            u64::MAX
        )
    }
    #[test]
    fn timeout_denies_mutation() {
        let a = GuildAccess::from_values_at(
            &base(1024 | 2048 | 65536 | 64 | 32768),
            &json!({"communication_disabled_until":"2030-01-01T00:00:00Z"}),
            "1",
            chrono::DateTime::from_timestamp(0, 0).unwrap(),
        );
        let p = a.evaluate(&json!({"type":0}), None);
        assert!(p.can_view && p.can_read_history);
        assert!(!p.can_send && !p.can_react && !p.can_attach)
    }
    #[test]
    fn thread_private_and_parent() {
        let a = GuildAccess::from_values(&base(1024 | (1u64 << 38) | 65536), &json!({}), "1");
        let t = json!({"type":12});
        assert!(!a.evaluate(&t, None).known);
        assert!(!a.evaluate(&t, Some(&json!({}))).can_view);
        assert!(
            a.evaluate(&json!({"type":12,"member":{}}), Some(&json!({})))
                .can_send
        )
    }
    #[test]
    fn malformed_acl_denies() {
        let a = GuildAccess::from_values(&base(1024 | 2048), &json!({}), "1");
        assert!(!a.evaluate(&json!({"permission_overwrites":{}}), None).known)
    }
    #[test]
    fn limited_member_list_scope() {
        let a = GuildAccess::from_values(&base(1024), &json!({}), "1");
        assert_eq!(a.confirmed_member_list_id(&json!({"type":0})), "everyone");
        assert_eq!(
            a.confirmed_member_list_id(
                &json!({"type":0,"permission_overwrites":[{"deny":"1024"}]})
            ),
            ""
        )
    }
    #[test]
    fn public_member_list_uses_channel_everyone_grant_without_accepting_restricted_lists() {
        let a = GuildAccess::from_values(&base(0), &json!({}), "1");
        let mut channel = json!({"type":0,"permission_overwrites":[{"id":"10","type":0,"allow":"1024","deny":"0"}]});
        assert!(a.evaluate(&channel, None).can_view);
        assert_eq!(a.confirmed_member_list_id(&channel), "everyone");
        channel["permission_overwrites"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"20","type":0,"allow":"0","deny":"1024"}));
        assert_eq!(a.confirmed_member_list_id(&channel), "");
        assert_eq!(
            a.confirmed_member_list_id(&json!({"type":0,"permission_overwrites":{}})),
            ""
        );
    }
    #[test]
    fn malformed_scope_types_never_infer_an_everyone_member_list() {
        let guild = json!({"id":"10","roles":[{"id":"10","permissions":"0"},{"id":"30","permissions":"1024"}]});
        let a = GuildAccess::from_values(&guild, &json!({"roles":["30"]}), "1");
        for invalid in [Value::Null, json!("0"), json!(false), json!(-1), json!(2)] {
            let channel = json!({"type":0,"permission_overwrites":[{"id":"10","type":invalid,"allow":"1024","deny":"0"}]});
            assert!(a.evaluate(&channel, None).can_view, "Own role still grants access");
            assert!(a.confirmed_member_list_id(&channel).is_empty(), "Malformed overwrite type must not become type zero");
        }
        let public = GuildAccess::from_values(&base(1024), &json!({}), "1");
        for invalid in [Value::Null, json!("0"), json!(false)] {
            assert!(public.confirmed_member_list_id(&json!({"type":invalid})).is_empty());
        }
        assert!(public.confirmed_member_list_id(&json!({"type":0,"permission_overwrites":[{"id":"10","allow":"1024"}]})).is_empty());
    }
}
