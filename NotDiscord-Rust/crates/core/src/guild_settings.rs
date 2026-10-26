//! Small, typed guild editor. Every write is checked against a newly read ACL.
use crate::{
    access::{GuildAccess, read_bits},
    models::Person,
    parsing::{b, i, items, s, valid_id},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MANAGE_GUILD: u64 = 1 << 5;
pub const MANAGE_CHANNELS: u64 = 1 << 4;
pub const MANAGE_ROLES: u64 = 1 << 28;
pub const CHANGE_NICKNAME: u64 = 1 << 26;
pub const MANAGE_NICKNAMES: u64 = 1 << 27;

/// Guild nicknames never replace the cached global identity.
pub fn visible_name<'a>(person: &'a Person, nickname: &'a str) -> &'a str {
    if !nickname.trim().is_empty() {
        nickname
    } else if !person.display_name.trim().is_empty() {
        &person.display_name
    } else if !person.username.trim().is_empty() {
        &person.username
    } else if !person.name.trim().is_empty() {
        &person.name
    } else {
        "Usuario"
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct GuildSettingsRole {
    pub id: String,
    pub name: String,
    pub position: i32,
    pub color: u32,
    pub permissions: u64,
    pub hoist: bool,
    pub mentionable: bool,
    pub managed: bool,
    pub editable: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct PermissionOverwrite {
    pub id: String,
    pub kind: i32,
    pub allow: u64,
    pub deny: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct GuildSettingsChannel {
    pub id: String,
    pub name: String,
    pub kind: i32,
    pub parent_id: String,
    pub topic: String,
    pub nsfw: bool,
    pub bitrate: i32,
    pub user_limit: i32,
    pub slowmode: i32,
    pub editable: bool,
    pub permissions_editable: bool,
    pub can_invite: bool,
    pub permission_overwrites: Vec<PermissionOverwrite>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct GuildSettings {
    pub guild_id: String,
    pub name: String,
    pub description: String,
    pub owner_id: String,
    pub verification_level: i32,
    pub explicit_content_filter: i32,
    pub default_message_notifications: i32,
    pub own_nickname: String,
    pub can_manage_guild: bool,
    pub can_manage_roles: bool,
    pub can_manage_channels: bool,
    pub can_change_nickname: bool,
    pub can_manage_nicknames: bool,
    pub effective_permissions: u64,
    pub roles: Vec<GuildSettingsRole>,
    pub channels: Vec<GuildSettingsChannel>,
    pub loading: bool,
    pub saving: bool,
    pub error: String,
    pub note: String,
    pub revision: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct GuildVoiceMember {
    pub guild_id: String,
    pub channel_id: String,
    pub user: Person,
    pub nickname: String,
    pub muted: bool,
    pub deafened: bool,
    pub self_mute: bool,
    pub server_mute: bool,
    pub self_deaf: bool,
    pub server_deaf: bool,
    pub self_video: bool,
    pub screen_sharing: bool,
}

fn confirmed(guild: &Value, member: &Value, me: &str) -> Result<GuildAccess, String> {
    if !valid_id(&s(guild, "id"))
        || s(&member["user"], "id") != me
        || !guild["roles"].is_array()
        || !member["roles"].is_array()
    {
        return Err(
            "No se pudieron confirmar el servidor, la membresía y los permisos actuales.".into(),
        );
    }
    Ok(GuildAccess::from_values(guild, member, me))
}
fn highest(guild: &Value, member: &Value) -> i32 {
    items(&guild["roles"])
        .iter()
        .filter(|role| {
            items(&member["roles"])
                .iter()
                .any(|id| id.as_str() == Some(s(role, "id").as_str()))
        })
        .map(|role| i(role, "position"))
        .max()
        .unwrap_or(0)
}
fn role_editable(guild: &Value, member: &Value, me: &str, acl: &GuildAccess, role: &Value) -> bool {
    acl.base_permissions & MANAGE_ROLES != 0
        && !b(role, "managed")
        && (s(guild, "owner_id") == me || i(role, "position") < highest(guild, member))
}
pub fn snapshot(
    guild: &Value,
    member: &Value,
    channels: &Value,
    me: &str,
) -> Result<GuildSettings, String> {
    let acl = confirmed(guild, member, me)?;
    if !channels.is_array() || items(channels).len() > 2000 || items(&guild["roles"]).len() > 1000 {
        return Err("La configuración recibida supera los límites del editor.".into());
    }
    let mut roles: Vec<_> = items(&guild["roles"])
        .iter()
        .map(|role| GuildSettingsRole {
            id: s(role, "id"),
            name: s(role, "name"),
            position: i(role, "position"),
            color: i(role, "color").max(0) as u32,
            permissions: read_bits(role, "permissions"),
            hoist: b(role, "hoist"),
            mentionable: b(role, "mentionable"),
            managed: b(role, "managed"),
            editable: role_editable(guild, member, me, &acl, role),
        })
        .collect();
    roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.id.cmp(&b.id)));
    let channels = crate::parsing::ordered_guild_channels(items(channels))
        .into_iter()
        .filter_map(|channel| {
            let bits = acl.channel_permissions(channel);
            if bits & (1 << 10) == 0 {
                return None;
            }
            Some(GuildSettingsChannel {
                id: s(channel, "id"),
                name: s(channel, "name"),
                kind: i(channel, "type"),
                parent_id: s(channel, "parent_id"),
                topic: s(channel, "topic"),
                nsfw: b(channel, "nsfw"),
                bitrate: i(channel, "bitrate"),
                user_limit: i(channel, "user_limit"),
                slowmode: i(channel, "rate_limit_per_user"),
                editable: bits & MANAGE_CHANNELS != 0,
                permissions_editable: bits & MANAGE_ROLES != 0,
                can_invite: bits & 1025 == 1025 && matches!(i(channel, "type"), 0 | 2 | 5),
                permission_overwrites: items(&channel["permission_overwrites"])
                    .iter()
                    .take(1000)
                    .map(|value| PermissionOverwrite {
                        id: s(value, "id"),
                        kind: i(value, "type"),
                        allow: read_bits(value, "allow"),
                        deny: read_bits(value, "deny"),
                    })
                    .collect(),
            })
        })
        .collect();
    Ok(GuildSettings {
        guild_id: s(guild, "id"),
        name: s(guild, "name"),
        description: s(guild, "description"),
        owner_id: s(guild, "owner_id"),
        verification_level: i(guild, "verification_level"),
        explicit_content_filter: i(guild, "explicit_content_filter"),
        default_message_notifications: i(guild, "default_message_notifications"),
        own_nickname: s(member, "nick"),
        can_manage_guild: acl.base_permissions & MANAGE_GUILD != 0,
        can_manage_roles: acl.base_permissions & MANAGE_ROLES != 0,
        can_manage_channels: acl.base_permissions & MANAGE_CHANNELS != 0,
        can_change_nickname: acl.base_permissions & CHANGE_NICKNAME != 0,
        can_manage_nicknames: acl.base_permissions & MANAGE_NICKNAMES != 0,
        effective_permissions: acl.base_permissions,
        roles,
        channels,
        ..Default::default()
    })
}

fn text(body: &Value, key: &str, min: usize, max: usize) -> Result<(), String> {
    if let Some(value) = body.get(key) {
        let Some(value) = value.as_str() else {
            return Err(format!("El campo {key} debe ser texto."));
        };
        if !(min..=max).contains(&value.chars().count())
            || value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(format!(
                "El campo {key} admite entre {min} y {max} caracteres."
            ));
        }
    }
    Ok(())
}
fn integer(body: &Value, key: &str, min: i64, max: i64) -> Result<(), String> {
    if let Some(value) = body.get(key)
        && value
            .as_i64()
            .is_none_or(|value| !(min..=max).contains(&value))
    {
        return Err(format!("El campo {key} está fuera de su intervalo."));
    }
    Ok(())
}
fn fields(body: &Value, allowed: &[&str]) -> Result<(), String> {
    let Some(map) = body.as_object() else {
        return Err("La modificación debe ser un objeto.".into());
    };
    if map.is_empty() || map.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("La modificación contiene campos no disponibles en este editor.".into());
    }
    Ok(())
}
fn require(bits: u64, permission: u64) -> Result<(), String> {
    if bits & permission != 0 {
        Ok(())
    } else {
        Err("Ya no tienes permiso para esta modificación.".into())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct GuildWrite {
    pub method: &'static str,
    pub path: String,
    pub body: Option<Value>,
}

/// Validate an explicit editor intent using fresh responses, never UI booleans.
pub fn prepare_write(
    guild: &Value,
    member: &Value,
    channels: &Value,
    me: &str,
    kind: &str,
    target: &str,
    body: &Value,
    target_member: Option<&Value>,
) -> Result<GuildWrite, String> {
    let acl = confirmed(guild, member, me)?;
    let guild_id = s(guild, "id");
    let path = |suffix: &str| format!("guilds/{guild_id}{suffix}");
    let mut method = "PATCH";
    let endpoint = match kind {
        "general" => {
            require(acl.base_permissions, MANAGE_GUILD)?;
            fields(
                body,
                &[
                    "name",
                    "description",
                    "verification_level",
                    "explicit_content_filter",
                    "default_message_notifications",
                ],
            )?;
            text(body, "name", 2, 100)?;
            text(body, "description", 0, 120)?;
            integer(body, "verification_level", 0, 4)?;
            integer(body, "explicit_content_filter", 0, 2)?;
            integer(body, "default_message_notifications", 0, 1)?;
            path("")
        }
        "role" | "role-create" | "role-delete" => {
            require(acl.base_permissions, MANAGE_ROLES)?;
            if kind != "role-create" {
                if !valid_id(target) {
                    return Err("Rol no válido.".into());
                }
                let role = items(&guild["roles"])
                    .iter()
                    .find(|role| s(role, "id") == target)
                    .ok_or("El rol ya no existe.")?;
                if !role_editable(guild, member, me, &acl, role) {
                    return Err("No puedes modificar roles iguales o superiores al tuyo, ni roles de una integración.".into());
                }
                if kind == "role-delete" && target == guild_id {
                    return Err("El rol @everyone no se puede eliminar.".into());
                }
            }
            if kind == "role-delete" {
                method = "DELETE";
                path(&format!("/roles/{target}"))
            } else {
                fields(
                    body,
                    &["name", "color", "hoist", "mentionable", "permissions"],
                )?;
                text(body, "name", 1, 100)?;
                integer(body, "color", 0, 0xffffff)?;
                for key in ["hoist", "mentionable"] {
                    if body.get(key).is_some_and(|value| !value.is_boolean()) {
                        return Err("La opción del rol no es válida.".into());
                    }
                }
                if let Some(value) = body.get("permissions") {
                    let bits = value
                        .as_str()
                        .and_then(|v| v.parse::<u64>().ok())
                        .ok_or("Permisos no válidos.")?;
                    let previous = items(&guild["roles"])
                        .iter()
                        .find(|role| s(role, "id") == target)
                        .map(|role| read_bits(role, "permissions"))
                        .unwrap_or(0);
                    if (bits & !previous) & !acl.base_permissions != 0 {
                        return Err("No puedes conceder permisos que no tienes.".into());
                    }
                }
                if kind == "role-create" {
                    method = "POST";
                    path("/roles")
                } else {
                    path(&format!("/roles/{target}"))
                }
            }
        }
        "channel" | "channel-create" | "channel-delete" => {
            if kind != "channel-create" && !valid_id(target) {
                return Err("Canal no válido.".into());
            }
            if kind == "channel-create" {
                require(acl.base_permissions, MANAGE_CHANNELS)?;
                method = "POST";
            }
            let channel = items(channels)
                .iter()
                .find(|channel| s(channel, "id") == target);
            if kind != "channel-create" {
                let channel = channel.ok_or("El canal ya no existe.")?;
                let bits = acl.channel_permissions(channel);
                require(bits, MANAGE_CHANNELS)?;
                require(bits, 1 << 10)?;
            }
            if kind == "channel-delete" {
                method = "DELETE";
                format!("channels/{target}")
            } else {
                fields(
                    body,
                    &[
                        "name",
                        "topic",
                        "nsfw",
                        "bitrate",
                        "user_limit",
                        "rate_limit_per_user",
                        "parent_id",
                        "type",
                    ],
                )?;
                text(body, "name", 1, 100)?;
                text(body, "topic", 0, 1024)?;
                integer(body, "user_limit", 0, 99)?;
                integer(body, "rate_limit_per_user", 0, 21600)?;
                if let Some(nsfw) = body.get("nsfw")
                    && !nsfw.is_boolean()
                {
                    return Err("La opción de contenido del canal no es válida.".into());
                }
                if let Some(bitrate) = body.get("bitrate") {
                    let maximum = match i(guild, "premium_tier") {
                        3 => 384000,
                        2 => 256000,
                        _ => 96000,
                    };
                    if bitrate
                        .as_i64()
                        .is_none_or(|v| !(8000..=maximum).contains(&v))
                    {
                        return Err(
                            "La tasa de audio supera el límite confirmado del servidor.".into()
                        );
                    }
                }
                if let Some(parent) = body.get("parent_id")
                    && !parent.is_null()
                {
                    let parent = parent.as_str().ok_or("Categoría no válida.")?;
                    let category = items(channels)
                        .iter()
                        .find(|channel| s(channel, "id") == parent && i(channel, "type") == 4)
                        .ok_or("La categoría ya no existe.")?;
                    require(acl.channel_permissions(category), MANAGE_CHANNELS)?;
                }
                if kind == "channel-create" {
                    if !matches!(body.get("type").and_then(Value::as_i64), Some(0 | 2 | 4)) {
                        return Err("Tipo de canal no disponible.".into());
                    }
                    path("/channels")
                } else {
                    if body.get("type").is_some() {
                        return Err("No se cambia el tipo de un canal existente.".into());
                    }
                    format!("channels/{target}")
                }
            }
        }
        "overwrite" | "overwrite-delete" => {
            let channel_id = s(body, "channel_id");
            let channel = items(channels)
                .iter()
                .find(|channel| s(channel, "id") == channel_id)
                .ok_or("El canal ya no existe.")?;
            let bits = acl.channel_permissions(channel);
            require(bits, MANAGE_ROLES)?;
            require(bits, 1 << 10)?;
            if !valid_id(target) {
                return Err("Destinatario de permisos no válido.".into());
            }
            if !items(&guild["roles"]).iter().any(|role|s(role,"id")==target){return Err("El rol ya no existe.".into());}
            if kind == "overwrite-delete" {
                let prior=items(&channel["permission_overwrites"]).iter().find(|value|s(value,"id")==target && i(value,"type")==0).ok_or("La sobrescritura ya no existe.")?;
                if (read_bits(prior,"allow")|read_bits(prior,"deny")) & !bits !=0 {return Err("No puedes eliminar una sobrescritura que cambia permisos que no tienes en este canal.".into());}
                method = "DELETE";
            } else {
                fields(body, &["channel_id", "type", "allow", "deny"])?;
                if !matches!(body.get("type").and_then(Value::as_i64), Some(0)) {
                    return Err("El editor modifica sobrescrituras de roles existentes.".into());
                }
                if !items(&guild["roles"])
                    .iter()
                    .any(|role| s(role, "id") == target)
                {
                    return Err("El rol ya no existe.".into());
                }
                let allow = body["allow"]
                    .as_str()
                    .and_then(|v| v.parse::<u64>().ok())
                    .ok_or("Permisos no válidos.")?;
                let deny = body["deny"]
                    .as_str()
                    .and_then(|v| v.parse::<u64>().ok())
                    .ok_or("Permisos no válidos.")?;
                if allow & deny != 0 {
                    return Err("Un permiso no se puede permitir y denegar a la vez.".into());
                }
                let prior = items(&channel["permission_overwrites"])
                    .iter()
                    .find(|v| s(v, "id") == target);
                let prior_allow = prior.map(|v| read_bits(v, "allow")).unwrap_or(0);
                let prior_deny = prior.map(|v| read_bits(v, "deny")).unwrap_or(0);
                if ((allow ^ prior_allow) | (deny ^ prior_deny)) & !bits != 0 {
                    return Err("No puedes modificar permisos que no tienes en este canal.".into());
                }
                method = "PUT";
            }
            format!("channels/{channel_id}/permissions/{target}")
        }
        "nickname" => {
            fields(body, &["nick"])?;
            text(body, "nick", 0, 32)?;
            let target_member = if target == me {member} else {
                target_member.ok_or("No se pudo confirmar la membresía de esa persona.")?
            };
            if s(&target_member["user"], "id") != target
                || !crate::member_moderation::capabilities(guild, member, target_member, me, chrono::Utc::now())?.can_nickname {
                return Err("No tienes permiso o la jerarquía impide cambiar este apodo.".into());
            }
            if target == me {
                require(acl.base_permissions, CHANGE_NICKNAME)?;
                path("/members/@me")
            } else {
                require(acl.base_permissions, MANAGE_NICKNAMES)?;
                if !valid_id(target)
                    || s(&target_member["user"], "id") != target
                    || target == s(guild, "owner_id")
                    || s(guild, "owner_id") != me
                        && highest(guild, target_member) >= highest(guild, member)
                {
                    return Err("No puedes cambiar el apodo de una persona con un rol igual o superior al tuyo.".into());
                }
                path(&format!("/members/{target}"))
            }
        }
        _ => return Err("Esta acción de servidor no está disponible.".into()),
    };
    let body = if method == "DELETE" {
        None
    } else if kind == "overwrite" {
        Some(json!({"type":0,"allow":body["allow"],"deny":body["deny"]}))
    } else if kind == "nickname" && body["nick"] == "" {
        Some(json!({"nick":null}))
    } else {
        Some(body.clone())
    };
    Ok(GuildWrite {
        method,
        path: endpoint,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, Value, Value) {
        (
            json!({"id":"10","owner_id":"9","name":"Synthetic","roles":[{"id":"10","position":0,"permissions":"1024"},{"id":"20","position":4,"permissions":((MANAGE_GUILD|MANAGE_CHANNELS|MANAGE_ROLES|CHANGE_NICKNAME|MANAGE_NICKNAMES|1024)).to_string()},{"id":"21","position":2,"permissions":"1024"},{"id":"22","position":5,"permissions":"8"},{"id":"23","position":1,"permissions":"1024","managed":true}]}),
            json!({"user":{"id":"1"},"roles":["20"]}),
            json!([{"id":"30","type":0,"name":"chat","permission_overwrites":[]}]),
        )
    }
    #[test]
    fn editor_rechecks_role_hierarchy_managed_roles_and_escalation() {
        let (g, m, c) = fixture();
        assert!(
            prepare_write(
                &g,
                &m,
                &c,
                "1",
                "role",
                "21",
                &json!({"name":"Renamed"}),
                None
            )
            .is_ok()
        );
        for role in ["20", "22", "23"] {
            assert!(
                prepare_write(
                    &g,
                    &m,
                    &c,
                    "1",
                    "role",
                    role,
                    &json!({"name":"Denied"}),
                    None
                )
                .is_err()
            );
        }
        assert!(
            prepare_write(
                &g,
                &m,
                &c,
                "1",
                "role",
                "21",
                &json!({"permissions":"8"}),
                None
            )
            .is_err()
        );
        let revoked = json!({"user":{"id":"1"},"roles":[]});
        assert!(
            prepare_write(
                &g,
                &revoked,
                &c,
                "1",
                "general",
                "",
                &json!({"name":"Denied"}),
                None
            )
            .is_err()
        );
    }
    #[test]
    fn nicknames_require_confirmed_membership_and_hierarchy() {
        let (g, m, c) = fixture();
        let low = json!({"user":{"id":"2"},"roles":["21"]});
        let high = json!({"user":{"id":"2"},"roles":["20"]});
        assert_eq!(
            prepare_write(&g, &m, &c, "1", "nickname", "1", &json!({"nick":""}), None)
                .unwrap()
                .body,
            Some(json!({"nick":null}))
        );
        assert!(
            prepare_write(
                &g,
                &m,
                &c,
                "1",
                "nickname",
                "2",
                &json!({"nick":"Local"}),
                Some(&low)
            )
            .is_ok()
        );
        assert!(
            prepare_write(
                &g,
                &m,
                &c,
                "1",
                "nickname",
                "2",
                &json!({"nick":"Denied"}),
                Some(&high)
            )
            .is_err()
        );
        assert!(
            prepare_write(
                &g,
                &m,
                &c,
                "1",
                "nickname",
                "9",
                &json!({"nick":"Owner"}),
                Some(&json!({"user":{"id":"9"},"roles":[]}))
            )
            .is_err()
        );
    }
    #[test]
    fn channel_overrides_and_editor_fields_fail_closed() {
        let (g, m, mut c) = fixture();
        c[0]["permission_overwrites"] =
            json!([{"id":"1","type":1,"allow":"0","deny":MANAGE_CHANNELS.to_string()}]);
        assert!(
            prepare_write(
                &g,
                &m,
                &c,
                "1",
                "channel",
                "30",
                &json!({"name":"Denied"}),
                None
            )
            .is_err()
        );
        assert!(
            prepare_write(
                &g,
                &m,
                &c,
                "1",
                "general",
                "",
                &json!({"owner_id":"1"}),
                None
            )
            .is_err()
        );
        assert!(
            prepare_write(
                &g,
                &m,
                &c,
                "1",
                "overwrite",
                "21",
                &json!({"channel_id":"30","type":0,"allow":"8","deny":"0"}),
                None
            )
            .is_err()
        );
    }
    #[test]
    fn names_prioritize_server_then_global_then_username_without_mutating_identity() {
        let p = Person {
            name: "Old".into(),
            display_name: "Global".into(),
            username: "Login".into(),
            ..Default::default()
        };
        assert_eq!(visible_name(&p, "Server"), "Server");
        assert_eq!(visible_name(&p, ""), "Global");
        let mut p = p;
        p.display_name.clear();
        assert_eq!(visible_name(&p, ""), "Login");
        assert_eq!(p.name, "Old");
    }
}
