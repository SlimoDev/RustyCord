//! Minimal READY sidebar projection. An absent/partial component remains null
//! so the controller can reconcile it through its existing authenticated GET.
use crate::parsing::{i, items, limited, s, valid_id};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

fn person(value: &Value) -> Option<Value> {
    let id = s(value, "id");
    if !valid_id(&id) || !value.get("username").is_some_and(Value::is_string) {
        return None;
    }
    Some(json!({"id":id,"username":limited(&s(value,"username"),128),
        "global_name":limited(&s(value,"global_name"),128),
        "avatar":limited(&s(value,"avatar"),128),"discriminator":limited(&s(value,"discriminator"),8),
        "bot":value["bot"].as_bool().unwrap_or(false)}))
}
fn user(value: &Value, users: &HashMap<String, Value>) -> Option<Value> {
    if value.is_object() {
        person(value)
    } else {
        value.as_str().and_then(|id| users.get(id)).cloned()
    }
}
fn projected_list<F>(source: &Value, max: usize, mut project: F) -> Value
where
    F: FnMut(&Value) -> Option<Value>,
{
    let Some(source) = source.as_array().filter(|source| source.len() <= max) else {
        return Value::Null;
    };
    let mut ids = HashSet::new();
    let result = source
        .iter()
        .map(|item| {
            let value = project(item)?;
            ids.insert(s(&value, "id")).then_some(value)
        })
        .collect::<Option<Vec<_>>>();
    result.map(Value::Array).unwrap_or(Value::Null)
}

pub fn ready_catalog(data: &Value) -> Value {
    let me = s(&data["user"], "id");
    let users = items(&data["users"])
        .iter()
        .take(6000)
        .filter_map(person)
        .map(|value| (s(&value, "id"), value))
        .collect::<HashMap<_, _>>();
    let guilds = projected_list(&data["guilds"], 500, |value| {
        let id = s(value, "id");
        let properties = value
            .get("properties")
            .filter(|v| v.is_object())
            .unwrap_or(value);
        let name = s(properties, "name");
        if !valid_id(&id) || name.is_empty() {
            return None;
        }
        let permissions = value
            .get("permissions")
            .or_else(|| properties.get("permissions"));
        let owner = valid_id(&me) && s(properties, "owner_id") == me;
        // Missing effective permissions must not silently remove the existing
        // guild context-menu capabilities. Reconcile that component by GET.
        if !owner
            && permissions.is_none_or(|value| {
                value.as_u64().is_none()
                    && value
                        .as_str()
                        .is_none_or(|value| value.parse::<u64>().is_err())
            })
        {
            return None;
        }
        Some(
            json!({"id":id,"name":limited(&name,160),"icon":limited(&s(properties,"icon"),128),"permissions":permissions.cloned().unwrap_or(json!("0")),"owner":owner}),
        )
    });
    let dms = projected_list(&data["private_channels"], 2000, |value| {
        let id = s(value, "id");
        let kind = i(value, "type");
        if !valid_id(&id) || !matches!(kind, 1 | 3) {
            return None;
        }
        let source = value
            .get("recipients")
            .or_else(|| value.get("recipient_ids"))?
            .as_array()?;
        if source.is_empty() || source.len() > 100 {
            return None;
        }
        let recipients = source
            .iter()
            .map(|value| user(value, &users))
            .collect::<Option<Vec<_>>>()?;
        Some(
            json!({"id":id,"type":kind,"name":limited(&s(value,"name"),160),
            "icon":limited(&s(value,"icon"),128),"owner_id":s(value,"owner_id"),
            "last_message_id":s(value,"last_message_id"),"recipients":recipients}),
        )
    });
    let relationships = projected_list(&data["relationships"], 3000, |value| {
        let person = value
            .get("user")
            .and_then(|v| user(v, &users))
            .or_else(|| users.get(&s(value, "user_id")).cloned())?;
        let kind = i(value, "type");
        if !(1..=5).contains(&kind) {
            return None;
        }
        Some(
            json!({"id":s(&person,"id"),"type":kind,"user":person,"since":limited(&s(value,"since"),100)}),
        )
    });
    json!({"guilds":guilds,"dms":dms,"relationships":relationships})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_ready_projects_only_sidebar_fields_and_resolves_user_ids() {
        let value = ready_catalog(
            &json!({"token":"private-token","session_id":"private-session",
          "users":[{"id":"42","username":"fixture","email":"private-email","avatar":"hash"}],
          "guilds":[{"id":"10","permissions":"0","properties":{"name":"Fixture"},"members":[{"token":"private-member"}]}],
          "private_channels":[{"id":"20","type":1,"recipient_ids":["42"]}],
          "relationships":[{"type":1,"user_id":"42"}]}),
        );
        assert_eq!(value["dms"][0]["recipients"][0]["username"], "fixture");
        assert_eq!(value["guilds"][0]["name"], "Fixture");
        assert_eq!(value["relationships"][0]["user"]["id"], "42");
        assert!(!value.to_string().contains("private-"));
        assert_eq!(value["guilds"][0]["permissions"], "0");
    }
    #[test]
    fn partial_or_excessive_components_require_fallback_without_truncation() {
        let value = ready_catalog(
            &json!({"guilds":[],"private_channels":[{"id":"20","type":1,"recipient_ids":["42"]}]}),
        );
        assert_eq!(value["guilds"], json!([]));
        assert!(value["dms"].is_null());
        assert!(value["relationships"].is_null());
        let oversized = ready_catalog(
            &json!({"guilds":(0..501).map(|n|json!({"id":(n+1).to_string(),"name":"fixture"})).collect::<Vec<_>>() }),
        );
        assert!(oversized["guilds"].is_null());
        let duplicate = ready_catalog(
            &json!({"guilds":[{"id":"10","name":"Fixture"},{"id":"10","name":"Duplicate"}]}),
        );
        assert!(duplicate["guilds"].is_null());
        assert!(
            ready_catalog(&json!({"guilds":[{"id":"10","name":"Missing permissions"}]}))["guilds"]
                .is_null()
        );
        assert!(ready_catalog(&json!({"guilds":[{"id":"10","name":"Invalid permissions","permissions":"not-numeric"}]}))["guilds"].is_null());
    }
}
