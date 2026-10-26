//! Guild moderation policy. UI capabilities are hints; every write uses fresh members and roles.
use crate::{
    access::{GuildAccess, read_bits},
    guild_settings::{CHANGE_NICKNAME, MANAGE_NICKNAMES},
    parsing::{b, items, s, valid_id},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const KICK_MEMBERS: u64 = 1 << 1;
pub const BAN_MEMBERS: u64 = 1 << 2;
pub const MODERATE_MEMBERS: u64 = 1 << 40;
pub const MAX_TIMEOUT_SECONDS: u64 = 28 * 24 * 60 * 60;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct MemberModeration {
    pub actor_id: String,
    pub guild_id: String,
    pub user_id: String,
    pub revision: u64,
    pub loading: bool,
    pub saving: bool,
    pub error: String,
    pub note: String,
    pub nickname: String,
    pub timed_out: bool,
    pub can_timeout: bool,
    pub can_kick: bool,
    pub can_ban: bool,
    pub can_nickname: bool,
}

fn confirmed(guild: &Value, member: &Value, user: &str) -> Result<GuildAccess, String> {
    let roles = guild["roles"].as_array().ok_or("No se pudieron confirmar los roles del servidor.")?;
    let member_roles = member["roles"].as_array().ok_or("No se pudieron confirmar los roles del miembro.")?;
    if !valid_id(&s(guild, "id")) || !valid_id(&s(guild, "owner_id"))
        || !valid_id(user) || s(&member["user"], "id") != user
        || roles.len() > 1000 || member_roles.len() > 500
        || !roles.iter().any(|role| s(role, "id") == s(guild, "id"))
        || roles.iter().any(|role| !valid_id(&s(role,"id")) || role["position"].as_i64().is_none_or(|value|value<0)
            || role.get("permissions").and_then(Value::as_str).filter(|v|!v.is_empty()&&v.bytes().all(|b|b.is_ascii_digit())).and_then(|v| v.parse::<u64>().ok()).is_none())
        || member["user"].get("bot").is_some_and(|value|!value.is_boolean())
        || member.get("communication_disabled_until").is_some_and(|value|!value.is_null()
            && value.as_str().is_none_or(|value|DateTime::parse_from_rfc3339(value).is_err()))
        || member_roles.iter().any(|id| id.as_str().is_none_or(|id| !valid_id(id)
            || !roles.iter().any(|role| s(role,"id") == id)))
    {
        return Err("No se pudieron confirmar la membresía y todos sus roles actuales.".into());
    }
    Ok(GuildAccess::from_values(guild, member, user))
}

fn highest(guild: &Value, member: &Value) -> i64 {
    items(&guild["roles"]).iter()
        .filter(|role| items(&member["roles"]).iter().any(|id| id.as_str() == Some(s(role,"id").as_str())))
        .filter_map(|role| role["position"].as_i64()).max().unwrap_or(0)
}

/// Equal positions are deliberately non-manageable: incomplete ordering must not grant moderation.
pub fn capabilities(guild: &Value, actor: &Value, target: &Value, me: &str, now: DateTime<Utc>) -> Result<MemberModeration, String> {
    let acl = confirmed(guild, actor, me)?;
    let user = s(&target["user"], "id");
    let target_acl = confirmed(guild, target, &user)?;
    let owner = s(guild, "owner_id");
    let is_self = me == user;
    let below = !is_self && user != owner && (me == owner || highest(guild, target) < highest(guild, actor));
    let actor_timed_out = actor.get("communication_disabled_until").and_then(Value::as_str)
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok()).is_some_and(|v| v > now)
        && acl.base_permissions != u64::MAX;
    let has = |permission| !actor_timed_out && acl.base_permissions & permission != 0;
    let target_admin = items(&guild["roles"]).iter().any(|role|
        (s(role,"id") == s(guild,"id") || target_acl.has_role(&s(role,"id"))) && read_bits(role,"permissions") & 8 != 0);
    let timed_out = target.get("communication_disabled_until").and_then(Value::as_str)
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok()).is_some_and(|v| v > now);
    Ok(MemberModeration {
        actor_id: me.into(), guild_id: s(guild,"id"), user_id: user,
        nickname: s(target,"nick"), timed_out,
        can_kick: below && has(KICK_MEMBERS),
        can_ban: below && has(BAN_MEMBERS),
        // Discord's timeout is for people; bots are excluded conservatively by this client.
        can_timeout: below && has(MODERATE_MEMBERS) && !target_admin && !b(&target["user"],"bot"),
        can_nickname: if is_self {has(CHANGE_NICKNAME)} else {below && has(MANAGE_NICKNAMES)},
        ..Default::default()
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModerationWrite {
    pub method: &'static str,
    pub path: String,
    pub body: Option<Value>,
    pub reason: Option<String>,
}

/// Validate form bounds before any read or write. Revision is checked by the actor.
pub fn validate_intent(kind: &str, body: &Value) -> Result<(), String> {
    let map = body.as_object().ok_or("La modificación no es válida.")?;
    let allowed: &[&str] = match kind {
        "timeout" => &["revision","reason","duration_seconds"],
        "untimeout" | "kick" => &["revision","reason"],
        "ban" => &["revision","reason","delete_message_seconds"],
        "nickname" => &["revision","nick"],
        _ => return Err("Esta acción de miembro no está disponible.".into()),
    };
    if map.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("La modificación contiene campos no disponibles.".into());
    }
    if let Some(reason) = body.get("reason") {
        let reason = reason.as_str().ok_or("El motivo debe ser texto.")?;
        if reason.chars().count() > 512 || reason.chars().any(char::is_control) {
            return Err("El motivo admite hasta 512 caracteres sin caracteres de control.".into());
        }
    }
    if kind == "nickname" {
        let nick = body.get("nick").and_then(Value::as_str).ok_or("El apodo debe ser texto.")?;
        if nick.chars().count() > 32 || nick.chars().any(char::is_control) {
            return Err("El apodo admite hasta 32 caracteres sin caracteres de control.".into());
        }
    }
    if kind == "timeout" && body.get("duration_seconds").and_then(Value::as_u64)
        .is_none_or(|value| !(1..=MAX_TIMEOUT_SECONDS).contains(&value)) {
        return Err("La duración debe estar entre 1 segundo y 28 días.".into());
    }
    if let Some(seconds) = body.get("delete_message_seconds")
        && seconds.as_u64().is_none_or(|value| value > 7 * 24 * 60 * 60) {
        return Err("El borrado de mensajes debe estar entre 0 y 7 días.".into());
    }
    Ok(())
}

pub fn prepare_write(guild: &Value, actor: &Value, target: &Value, me: &str, kind: &str, body: &Value, now: DateTime<Utc>) -> Result<ModerationWrite, String> {
    validate_intent(kind, body)?;
    let policy = capabilities(guild, actor, target, me, now)?;
    let allowed = match kind {
        "timeout" | "untimeout" => policy.can_timeout,
        "kick" => policy.can_kick,
        "ban" => policy.can_ban,
        "nickname" => policy.can_nickname,
        _ => false,
    };
    if !allowed {return Err("Ya no tienes permiso o la jerarquía de roles impide esta acción.".into());}
    let base = format!("guilds/{}", policy.guild_id);
    let member = format!("{base}/members/{}", policy.user_id);
    let (method, path, payload) = match kind {
        "kick" => ("DELETE", member, None),
        "ban" => ("PUT", format!("{base}/bans/{}",policy.user_id), Some(json!({"delete_message_seconds":body["delete_message_seconds"].as_u64().unwrap_or(0)}))),
        "nickname" => ("PATCH", if me == policy.user_id {format!("{base}/members/@me")} else {member},
            Some(json!({"nick":body["nick"].as_str().filter(|v| !v.is_empty())}))),
        "timeout" => ("PATCH", member, Some(json!({"communication_disabled_until":
            (now + Duration::seconds(body["duration_seconds"].as_u64().unwrap() as i64)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}))),
        "untimeout" => ("PATCH", member, Some(json!({"communication_disabled_until":null}))),
        _ => unreachable!(),
    };
    Ok(ModerationWrite{method,path,body:payload,reason:body.get("reason").and_then(Value::as_str).filter(|v|!v.trim().is_empty()).map(str::to_owned)})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value,Value,Value) {
        let permissions=KICK_MEMBERS|BAN_MEMBERS|MODERATE_MEMBERS|CHANGE_NICKNAME|MANAGE_NICKNAMES;
        (json!({"id":"10","owner_id":"9","roles":[{"id":"10","position":0,"permissions":"0"},{"id":"20","position":5,"permissions":permissions.to_string()},{"id":"21","position":2,"permissions":"0"},{"id":"22","position":6,"permissions":"8"},{"id":"23","position":1,"permissions":"8"}]}),
        json!({"user":{"id":"1"},"roles":["20"]}),json!({"user":{"id":"2"},"roles":["21"],"nick":"Original"}))
    }
    fn policy(g:&Value,a:&Value,t:&Value)->MemberModeration {capabilities(g,a,t,"1",Utc::now()).unwrap()}
    #[test]
    fn permission_matrix_hierarchy_owner_admin_self_and_bot() {
        let (mut g,mut a,mut t)=fixture();
        let p=policy(&g,&a,&t);assert!(p.can_timeout&&p.can_kick&&p.can_ban&&p.can_nickname);
        let p=policy(&g,&a,&a);assert!(!p.can_timeout&&!p.can_kick&&!p.can_ban&&p.can_nickname);
        t["user"]["id"]=json!("9");let p=policy(&g,&a,&t);assert!(!p.can_timeout&&!p.can_kick&&!p.can_ban&&!p.can_nickname);
        t["user"]["id"]=json!("2");t["roles"]=json!(["20"]);let p=policy(&g,&a,&t);assert!(!p.can_kick&&!p.can_ban&&!p.can_nickname);
        a["roles"]=json!(["23"]);t["roles"]=json!(["21"]);let p=policy(&g,&a,&t);assert!(!p.can_kick&&!p.can_ban,"Administrator never bypasses hierarchy");
        a["roles"]=json!(["20"]);t["roles"]=json!(["23"]);let p=policy(&g,&a,&t);assert!(!p.can_timeout&&p.can_kick&&p.can_ban);
        t["roles"]=json!(["21"]);t["user"]["bot"]=json!(true);let p=policy(&g,&a,&t);assert!(!p.can_timeout&&p.can_kick&&p.can_ban&&p.can_nickname);
        g["owner_id"]=json!("1");t["roles"]=json!(["22"]);let p=policy(&g,&a,&t);assert!(p.can_kick&&p.can_ban&&p.can_nickname&&!p.can_timeout);
    }
    #[test]
    fn missing_members_roles_and_timed_out_actor_never_grant_capabilities() {
        let (g,mut a,mut t)=fixture();
        for unknown in [json!(null),json!(["missing"]),json!([12])] {t["roles"]=unknown;assert!(capabilities(&g,&a,&t,"1",Utc::now()).is_err());}
        t["roles"]=json!(["21"]);t["user"]["id"]=json!("3");assert!(capabilities(&g,&a,&t,"invalid",Utc::now()).is_err());
        a["communication_disabled_until"]=json!((Utc::now()+Duration::hours(1)).to_rfc3339());let p=policy(&g,&a,&t);assert!(!p.can_kick&&!p.can_ban&&!p.can_timeout&&!p.can_nickname);
        a["roles"]=json!([]);a["communication_disabled_until"]=Value::Null;let p=policy(&g,&a,&t);assert!(!p.can_kick&&!p.can_ban&&!p.can_timeout&&!p.can_nickname);
    }
    #[test]
    fn requests_are_bounded_and_use_documented_fields_and_routes() {
        let (g,a,t)=fixture();let now=Utc::now();
        let write=prepare_write(&g,&a,&t,"1","timeout",&json!({"duration_seconds":600,"reason":"Razón ñ"}),now).unwrap();
        assert_eq!(write.path,"guilds/10/members/2");assert_eq!(write.method,"PATCH");assert_eq!(write.reason.as_deref(),Some("Razón ñ"));
        let until=DateTime::parse_from_rfc3339(write.body.as_ref().unwrap()["communication_disabled_until"].as_str().unwrap()).unwrap();assert!((until.with_timezone(&Utc)-now).num_seconds()>=599);
        assert_eq!(prepare_write(&g,&a,&a,"1","nickname",&json!({"nick":""}),now).unwrap().body,Some(json!({"nick":null})));
        assert_eq!(prepare_write(&g,&a,&a,"1","nickname",&json!({"nick":"own"}),now).unwrap().path,"guilds/10/members/@me");
        assert_eq!(prepare_write(&g,&a,&t,"1","kick",&json!({}),now).unwrap().body,None);
        assert_eq!(prepare_write(&g,&a,&t,"1","ban",&json!({}),now).unwrap().body,Some(json!({"delete_message_seconds":0})));
        for body in [json!({"duration_seconds":0}),json!({"duration_seconds":MAX_TIMEOUT_SECONDS+1}),json!({"duration_seconds":1,"roles":[]})] {assert!(prepare_write(&g,&a,&t,"1","timeout",&body,now).is_err());}
        assert!(validate_intent("ban",&json!({"delete_message_seconds":604801})).is_err());
        assert!(validate_intent("kick",&json!({"reason":"x".repeat(513)})).is_err());
        assert!(validate_intent("nickname",&json!({"nick":"x".repeat(33)})).is_err());
    }
    #[test]
    fn individual_permission_bits_do_not_grant_other_actions_or_self_manage_nickname() {
        let (mut g,a,t)=fixture();
        for permission in [KICK_MEMBERS,BAN_MEMBERS,MODERATE_MEMBERS,CHANGE_NICKNAME,MANAGE_NICKNAMES] {
            g["roles"][1]["permissions"]=json!(permission.to_string());
            let p=policy(&g,&a,&t);
            assert_eq!(p.can_kick,permission==KICK_MEMBERS);
            assert_eq!(p.can_ban,permission==BAN_MEMBERS);
            assert_eq!(p.can_timeout,permission==MODERATE_MEMBERS);
            assert_eq!(p.can_nickname,permission==MANAGE_NICKNAMES);
            assert_eq!(policy(&g,&a,&a).can_nickname,permission==CHANGE_NICKNAME);
        }
    }
}
