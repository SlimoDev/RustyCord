//! Coalesced automatic acknowledgements retain only validated visible watermarks.
use super::*;

pub(super) fn queue_member_subscription<F>(
    accepted: &mut Option<(String, String, i32, i32)>,
    desired: (String, String, i32, i32),
    ready: bool,
    enqueue: F,
) where
    F: FnOnce(GatewayCommand) -> bool,
{
    if !ready || accepted.as_ref() == Some(&desired) || (desired.0.is_empty() && accepted.is_none())
    {
        return;
    }
    if enqueue(GatewayCommand::SubscribeGuild {
        guild_id: desired.0.clone(),
        channel_id: desired.1.clone(),
        range_start: desired.2,
        range_end: desired.3,
    }) {
        *accepted = Some(desired);
    }
}

pub(super) async fn load_sidebar<F, R>(mut seed: Value, mut get: F) -> Reply
where
    F: FnMut(&'static str) -> R,
    R: std::future::Future<Output = Reply>,
{
    for (key, path, limit) in [
        ("guilds", "users/@me/guilds?limit=200", 500),
        ("dms", "users/@me/channels", 2000),
        ("relationships", "users/@me/relationships", 3000),
    ] {
        if !component_complete(key, &seed[key]) {
            seed[key] = get(path).await?;
        }
        if !component_complete(key, &seed[key])
            || seed[key]
                .as_array()
                .is_none_or(|values| values.len() > limit)
        {
            return Err(
                "El catálogo recibido está incompleto o supera el límite de esta vista.".into(),
            );
        }
    }
    Ok(seed)
}

pub(super) fn component_complete(key: &str, value: &Value) -> bool {
    let Some(values) = value.as_array() else {
        return false;
    };
    values.iter().all(|value| match key {
        "guilds" => {
            parsing::valid_id(&s(value, "id"))
                && !s(value, "name").is_empty()
                && (b(value, "owner")
                    || value["permissions"].as_u64().is_some()
                    || value["permissions"]
                        .as_str()
                        .is_some_and(|v| v.parse::<u64>().is_ok()))
        }
        "dms" => {
            parsing::valid_id(&s(value, "id"))
                && matches!(i(value, "type"), 1 | 3)
                && value["recipients"].as_array().is_some_and(|users| {
                    users.len() <= 100
                        && (i(value, "type") == 3 || !users.is_empty())
                        && users.iter().all(complete_person)
                })
        }
        "relationships" => matches!(i(value, "type"), 1..=5) && complete_person(&value["user"]),
        _ => false,
    })
}
fn complete_person(value: &Value) -> bool {
    parsing::valid_id(&s(value, "id")) && value.get("username").is_some_and(Value::is_string)
}
pub(super) fn invalidate_partial_component(seed: &mut Value, kind: &str, value: &Value) -> bool {
    let key = if kind.starts_with("GUILD_") {
        "guilds"
    } else if kind.starts_with("RELATIONSHIP_") {
        "relationships"
    } else {
        "dms"
    };
    let Some(values) = seed[key].as_array() else {
        return false;
    };
    let id = s(value, "id");
    let unknown_update = kind.ends_with("_UPDATE")
        && !values.iter().any(|existing| {
            if key == "relationships" {
                s(&existing["user"], "id") == id || s(existing, "id") == id
            } else {
                s(existing, "id") == id
            }
        });
    if unknown_update || !component_complete(key, &seed[key]) {
        seed[key] = Value::Null;
        true
    } else {
        false
    }
}

pub(super) fn catalog_delta(kind: &str, value: &Value) -> Option<Value> {
    if kind == "GUILD_DELETE" && b(value, "unavailable") {
        return None;
    }
    let fields: &[&str] = match kind {
        "GUILD_CREATE" | "GUILD_UPDATE" | "GUILD_DELETE" => {
            &["id", "name", "icon", "permissions", "owner"]
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "CHANNEL_DELETE"
            if s(value, "guild_id").is_empty() =>
        {
            &[
                "id",
                "type",
                "name",
                "icon",
                "owner_id",
                "last_message_id",
                "recipients",
            ]
        }
        "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" | "RELATIONSHIP_REMOVE" => {
            &["id", "user", "type", "since"]
        }
        _ => return None,
    };
    let mut result = serde_json::Map::new();
    for key in fields {
        let Some(field) = value.get(*key).or_else(|| value["properties"].get(*key)) else {
            continue;
        };
        if *key == "user" {
            let mut user = serde_json::Map::new();
            for key in [
                "id",
                "username",
                "global_name",
                "avatar",
                "discriminator",
                "bot",
            ] {
                if let Some(field) = field.get(key) {
                    user.insert(key.into(), field.clone());
                }
            }
            result.insert((*key).into(), Value::Object(user));
        } else if *key == "recipients" {
            let users = field.as_array()?;
            if users.len() > 100 {
                return None;
            }
            result.insert(
                (*key).into(),
                Value::Array(
                    users
                        .iter()
                        .map(|user| {
                            let mut result = serde_json::Map::new();
                            for key in [
                                "id",
                                "username",
                                "global_name",
                                "avatar",
                                "discriminator",
                                "bot",
                            ] {
                                if let Some(field) = user.get(key) {
                                    result.insert(key.into(), field.clone());
                                }
                            }
                            Value::Object(result)
                        })
                        .collect(),
                ),
            );
        } else {
            result.insert((*key).into(), field.clone());
        }
    }
    if kind.starts_with("RELATIONSHIP_") && s(&Value::Object(result.clone()), "id").is_empty() {
        result.insert("id".into(), json!(s(&value["user"], "id")));
    }
    let result = Value::Object(result);
    (parsing::valid_id(&s(&result, "id")) && result.to_string().len() <= 64 * 1024)
        .then_some(result)
}
pub(super) fn overlay_sidebar(seed: &mut Value, kind: &str, value: &Value) {
    let component = if kind.starts_with("GUILD_") {
        "guilds"
    } else if kind.starts_with("RELATIONSHIP_") {
        "relationships"
    } else {
        "dms"
    };
    let Some(values) = seed[component].as_array_mut() else {
        return;
    };
    let id = s(value, "id");
    let index = values.iter().position(|existing| {
        if component == "relationships" {
            s(&existing["user"], "id") == id || s(existing, "id") == id
        } else {
            s(existing, "id") == id
        }
    });
    if kind.ends_with("_DELETE") || kind == "RELATIONSHIP_REMOVE" {
        if let Some(index) = index {
            values.remove(index);
        }
        return;
    }
    if let Some(index) = index {
        if let (Some(previous), Some(update)) = (values[index].as_object_mut(), value.as_object()) {
            for (key, value) in update {
                previous.insert(key.clone(), value.clone());
            }
        }
    } else if !kind.ends_with("_UPDATE") && values.len() < 3000 {
        values.push(value.clone());
    }
}
#[derive(Default)]
pub(super) struct AckBatch {
    pending: HashMap<String, (String, Instant)>,
    in_flight: HashSet<String>,
}
impl AckBatch {
    pub fn advance(&mut self, channel: &str, last: &str, now: Instant) {
        if self.pending.len() >= 128 && !self.pending.contains_key(channel) {
            return;
        }
        let entry = self
            .pending
            .entry(channel.into())
            .or_insert_with(|| (last.into(), now));
        if super::incoming::snowflake_greater(last, &entry.0) {
            entry.0 = last.into();
        }
    }
    pub fn ready(&mut self, now: Instant) -> Vec<(String, String)> {
        let channels = self
            .pending
            .iter()
            .filter(|(channel, (_, at))| {
                !self.in_flight.contains(*channel)
                    && now.saturating_duration_since(*at) >= Duration::from_millis(750)
            })
            .take(16usize.saturating_sub(self.in_flight.len()))
            .map(|(channel, _)| channel.clone())
            .collect::<Vec<_>>();
        channels
            .into_iter()
            .filter_map(|channel| {
                let (last, _) = self.pending.remove(&channel)?;
                self.in_flight.insert(channel.clone());
                Some((channel, last))
            })
            .collect()
    }
    pub fn complete(&mut self, channel: &str) {
        self.in_flight.remove(channel);
    }
    pub fn clear(&mut self) {
        self.pending.clear();
        self.in_flight.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visible_ack_bursts_coalesce_to_latest_and_never_run_twice_per_channel() {
        let mut batch = AckBatch::default();
        let now = Instant::now();
        batch.advance("20", "100", now);
        batch.advance("20", "102", now + Duration::from_millis(100));
        batch.advance("20", "101", now + Duration::from_millis(200));
        assert!(batch.ready(now + Duration::from_millis(500)).is_empty());
        assert_eq!(
            batch.ready(now + Duration::from_millis(750)),
            vec![("20".into(), "102".into())]
        );
        batch.advance("20", "103", now);
        assert!(batch.ready(now + Duration::from_secs(1)).is_empty());
        batch.complete("20");
        assert_eq!(
            batch.ready(now + Duration::from_secs(1)),
            vec![("20".into(), "103".into())]
        );
    }
}
