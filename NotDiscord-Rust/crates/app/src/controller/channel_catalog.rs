use super::*;

/// Small session-only cache. A Gateway gap or a permission/catalog event
/// invalidates it; old ACL metadata is never used as an offline fallback.
#[derive(Default)]
pub(super) struct GuildCatalogCache {
    entries: HashMap<String,(Instant,Value)>,
    revision: u64,
}
impl GuildCatalogCache {
    pub(super) fn clear(&mut self){self.entries.clear();self.revision=self.revision.wrapping_add(1);}
    pub(super) fn get(&self,guild:&str)->Option<Value>{
        let (_,value)=self.entries.get(guild).filter(|(at,_)|at.elapsed()<Duration::from_secs(30))?;
        let mut value=value.clone();value["cachedCatalogRevision"]=json!(self.revision);Some(value)
    }
    pub(super) fn current(&self,value:&Value)->bool{
        value.get("cachedCatalogRevision").is_none_or(|revision|
            revision.as_u64()==Some(self.revision) && self.entries.get(&s(&value["guild"],"id"))
                .is_some_and(|(at,_)|at.elapsed()<Duration::from_secs(30)))
    }
    pub(super) fn store(&mut self,guild:&str,me:&str,value:&Value){
        if s(&value["guild"],"id")!=guild || s(&value["member"]["user"],"id")!=me
            || !value["guild"]["roles"].is_array() || !value["member"]["roles"].is_array()
            || value["channels"].as_array().is_none_or(|channels|channels.len()>2000)
            || value.to_string().len()>2*1024*1024{return;}
        self.entries.remove(guild);
        while self.entries.len()>=4{
            let Some(oldest)=self.entries.iter().min_by_key(|(_, (at,_))|*at).map(|(id,_)|id.clone())else{break;};
            self.entries.remove(&oldest);
        }
        let mut value=value.clone();
        if let Some(object)=value.as_object_mut(){object.remove("cachedCatalogRevision");}
        self.entries.insert(guild.into(),(Instant::now(),value));
    }
    pub(super) fn observe(&mut self,kind:&str,value:&Value,me:&str){
        let relevant=kind.starts_with("CHANNEL_")||kind.starts_with("THREAD_")||kind.starts_with("GUILD_ROLE_")
            || matches!(kind,"GUILD_CREATE"|"GUILD_UPDATE"|"GUILD_DELETE")
            || matches!(kind,"GUILD_MEMBER_UPDATE"|"GUILD_MEMBER_REMOVE")&&s(&value["user"],"id")==me;
        if !relevant{return;}
        let guild=if matches!(kind,"GUILD_CREATE"|"GUILD_UPDATE"|"GUILD_DELETE"){s(value,"id")}else{s(value,"guild_id")};
        // An event without a guild cannot safely be matched against a cache
        // of another visited server. Prefer a new guarded read in that case.
        if guild.is_empty(){self.clear();}
        else{self.entries.remove(&guild);self.revision=self.revision.wrapping_add(1);}
    }
}

/// Gateway changes received while the three REST guild reads are in flight.
/// Replaying them before computing ACL avoids dropping new channels and also
/// avoids restoring permissions which a newer role/channel event revoked.
#[derive(Default)]
pub(super) struct GuildLoad {
    guild: String,
    events: Vec<(String, Value)>,
    bytes: usize,
    overflow: bool,
}
fn fields(value: &Value, names: &[&str]) -> Value {
    Value::Object(names.iter().filter_map(|name| value.get(*name).map(|value| ((*name).into(),value.clone()))).collect())
}
fn channel_delta(value: &Value) -> Value {
    fields(value,&["id","guild_id","type","name","parent_id","position","permission_overwrites",
        "last_message_id","owner_id","member_list_id","topic","nsfw","bitrate","user_limit","rtc_region",
        "rate_limit_per_user","flags","default_auto_archive_duration","default_thread_rate_limit_per_user"])
}
impl GuildLoad {
    pub(super) fn begin(&mut self, guild: &str) { *self=Self{guild:guild.into(),..Default::default()}; }
    pub(super) fn clear(&mut self) { *self=Self::default(); }
    pub(super) fn is_loading(&self, guild: &str) -> bool { !guild.is_empty() && self.guild == guild }
    pub(super) fn observe(&mut self, kind: &str, value: &Value, me: &str, known_channel: bool) {
        if self.guild.is_empty() || self.overflow {return;}
        let guild=if matches!(kind,"GUILD_CREATE"|"GUILD_UPDATE") {s(value,"id")} else {s(value,"guild_id")};
        if guild!=self.guild && !(guild.is_empty()&&known_channel&&kind.starts_with("CHANNEL_")) {return;}
        let clean=match kind {
            "CHANNEL_CREATE"|"CHANNEL_UPDATE"|"CHANNEL_DELETE" if parsing::valid_id(&s(value,"id"))=>channel_delta(value),
            "GUILD_CREATE"|"GUILD_UPDATE"=>{
                let mut clean=fields(value,&["id","name","owner_id","roles"]);
                if let Some(channels)=value.get("channels").and_then(Value::as_array) {
                    clean["channels"]=Value::Array(channels.iter().take(2001).map(channel_delta).collect());
                }
                clean
            }
            "GUILD_ROLE_CREATE"|"GUILD_ROLE_UPDATE"=>fields(value,&["guild_id","role"]),
            "GUILD_ROLE_DELETE"=>fields(value,&["guild_id","role_id"]),
            "GUILD_MEMBER_UPDATE" if s(&value["user"],"id")==me=>fields(value,&["guild_id","roles","communication_disabled_until","nick"]),
            _=>return,
        };
        let bytes=clean.to_string().len();
        if self.events.len()>=512 || self.bytes.saturating_add(bytes)>2*1024*1024 {
            self.overflow=true;self.events.clear();self.bytes=0;return;
        }
        self.bytes+=bytes;self.events.push((kind.into(),clean));
    }
    pub(super) fn reconcile(&mut self,guild:&str,mut response:Value)->Result<Value,String> {
        if self.guild!=guild {return Ok(response);}
        let pending=std::mem::take(self);
        if pending.overflow {return Err("La lista de canales cambió demasiado durante su carga. Vuelve a abrir el servidor para actualizarla.".into());}
        let mut channels=items(&response["channels"]).iter().take(2000).filter(|channel|parsing::valid_id(&s(channel,"id")))
            .map(|channel|(s(channel,"id"),channel.clone())).collect::<HashMap<_,_>>();
        for (kind,value) in pending.events {
            match kind.as_str() {
                "GUILD_CREATE"|"GUILD_UPDATE"=>{
                    let mut metadata=value.clone();metadata.as_object_mut().unwrap().remove("channels");
                    response["guild"]=super::incoming::merge_object(&response["guild"],&metadata);
                    if let Some(snapshot)=value.get("channels").and_then(Value::as_array) {
                        if snapshot.len()>2000 {return Err("La lista de canales supera el límite local.".into());}
                        channels=snapshot.iter().filter(|channel|parsing::valid_id(&s(channel,"id"))).map(|channel|(s(channel,"id"),channel.clone())).collect();
                    }
                }
                "GUILD_MEMBER_UPDATE"=>response["member"]=super::incoming::merge_object(&response["member"],&value),
                "GUILD_ROLE_CREATE"|"GUILD_ROLE_UPDATE"|"GUILD_ROLE_DELETE"=>{
                    let id=if kind=="GUILD_ROLE_DELETE" {s(&value,"role_id")} else {s(&value["role"],"id")};
                    if !parsing::valid_id(&id) {continue;}
                    let mut roles=items(&response["guild"]["roles"]).to_vec();
                    if kind=="GUILD_ROLE_DELETE" {roles.retain(|role|s(role,"id")!=id);}
                    else if let Some(role)=roles.iter_mut().find(|role|s(role,"id")==id) {*role=super::incoming::merge_object(role,&value["role"]);}
                    else if roles.len()<1000 {roles.push(value["role"].clone());}
                    response["guild"]["roles"]=json!(roles);
                }
                "CHANNEL_DELETE"=>{channels.remove(&s(&value,"id"));}
                "CHANNEL_CREATE"|"CHANNEL_UPDATE"=>{
                    let id=s(&value,"id");
                    let previous=channels.get(&id);
                    if previous.is_none() && !value.get("type").is_some_and(Value::is_i64) {continue;}
                    if channels.len()<2000||previous.is_some() {
                        let merged=super::incoming::merge_object(previous.unwrap_or(&Value::Null),&value);
                        channels.insert(id,merged);
                    }
                }
                _=>{}
            }
        }
        response["channels"]=json!(channels.into_values().collect::<Vec<_>>());
        Ok(response)
    }
}

#[derive(Default)]
pub(super) struct ThreadCatalog {
    entries: HashMap<String, (Instant, Vec<Value>)>,
    pending: HashSet<String>,
}
pub(super) fn thread(guild: &str, value: &Value) -> Option<Value> {
    let id = s(value, "id");
    if !parsing::valid_id(guild) || !parsing::valid_id(&id)
        || !matches!(i(value, "type"), 10..=12) || !parsing::valid_id(&s(value, "parent_id"))
        || !s(value, "guild_id").is_empty() && s(value, "guild_id") != guild { return None; }
    let mut clean = serde_json::Map::new();
    for key in ["id", "type", "parent_id", "position", "last_message_id", "owner_id", "member_list_id"] {
        if let Some(value) = value.get(key) { clean.insert(key.into(), value.clone()); }
    }
    clean.insert("guild_id".into(), json!(guild));
    clean.insert("name".into(), json!(parsing::limited(&s(value, "name"), 100)));
    for (key, fields) in [("thread_metadata", &["archived", "locked", "invitable", "auto_archive_duration"][..]),
        ("member", &["id", "user_id", "flags", "join_timestamp"][..])] {
        if let Some(object) = value.get(key).and_then(Value::as_object) {
            clean.insert(key.into(), Value::Object(fields.iter().filter_map(|field| object.get(*field)
                .map(|value| ((*field).into(), value.clone()))).collect()));
        }
    }
    Some(Value::Object(clean))
}
pub(super) fn thread_response(guild: &str, value: &Value, me: &str) -> Vec<Value> {
    let members: HashMap<_, _> = items(&value["members"]).iter().take(2000)
        .filter(|member| s(member, "user_id").is_empty() || s(member, "user_id") == me)
        .filter(|member| parsing::valid_id(&s(member, "id")))
        .map(|member| (s(member, "id"), member)).collect();
    items(&value["threads"]).iter().take(2001).filter_map(|value| {
        let mut clean = thread(guild, value)?;
        if let Some(member) = members.get(&s(&clean, "id")) {
            clean["member"] = (*member).clone();
            clean = thread(guild, &clean)?;
        }
        if clean.get("member").is_some_and(|member| !s(member, "user_id").is_empty() && s(member, "user_id") != me) {
            clean.as_object_mut()?.remove("member");
        }
        Some(clean)
    }).collect()
}
impl ThreadCatalog {
    pub(super) fn get(&self, guild: &str) -> Option<Vec<Value>> {
        self.entries.get(guild).filter(|(at, _)| at.elapsed() < Duration::from_secs(300)).map(|(_, values)| values.clone())
    }
    pub(super) fn begin_request(&mut self, guild: &str) -> bool {
        !self.entries.get(guild).is_some_and(|(at, _)| at.elapsed() < Duration::from_secs(300))
            && self.pending.len() < 8 && self.pending.insert(guild.into())
    }
    pub(super) fn finish_request(&mut self, guild: &str) { self.pending.remove(guild); }
    pub(super) fn replace(&mut self, guild: &str, values: &[Value]) {
        self.finish_request(guild);
        if values.len() > 2000 { self.entries.remove(guild); return; }
        let mut seen = HashSet::new();
        let clean: Vec<_> = values.iter().filter_map(|value| thread(guild, value))
            .filter(|value| seen.insert(s(value, "id"))).collect();
        self.entries.remove(guild);
        while self.entries.len() >= 8 || self.entries.values().map(|(_, values)| values.len()).sum::<usize>() + clean.len() > 2000 {
            let Some(oldest) = self.entries.iter().min_by_key(|(_, (at, _))| *at).map(|(id, _)| id.clone()) else { break; };
            self.entries.remove(&oldest);
        }
        self.entries.insert(guild.into(), (Instant::now(), clean));
    }
    pub(super) fn delta(&mut self, guild: &str, value: &Value, deleted: bool) {
        let Some(mut values) = self.get(guild) else { return; };
        let id = s(value, "id");
        let previous = values.iter().find(|item| s(item, "id") == id).cloned().unwrap_or(Value::Null);
        values.retain(|item| s(item, "id") != id);
        if !deleted { values.push(super::incoming::merge_object(&previous, value)); }
        self.replace(guild, &values);
    }
    pub(super) fn remove(&mut self, guild: &str) { self.entries.remove(guild); self.pending.remove(guild); }
}
impl Controller {
    pub(super) fn request_guild_threads(&mut self, guild: &str) {
        let Some(api) = self.api.clone() else { return; };
        if !self.thread_catalog.begin_request(guild) { return; }
        let version = self.next_request(&format!("guild-threads/{guild}"));
        let guild = guild.to_owned();
        let mut action = UiAction::id("__guild-threads", &guild); action.extra = version.to_string();
        self.launch(action, None, async move {
            api.get(&format!("guilds/{guild}/threads/active")).await.map_err(|error| error.to_string())
        });
    }
    pub(super) fn complete_guild_threads(&mut self, action: &UiAction, value: &Value) {
        let guild = action.id.as_str();
        self.thread_catalog.finish_request(guild);
        if !action.extra.parse::<u64>().ok().is_some_and(|version| self.request_current(&format!("guild-threads/{guild}"), version)) { return; }
        if !value["threads"].is_array() { return; }
        let threads = thread_response(guild, value, &self.me());
        self.thread_catalog.replace(guild, &threads);
        if guild != self.state.selected_guild_id { return; }
        if let Some(threads) = self.thread_catalog.get(guild) {
            let ids: HashSet<_> = threads.iter().map(|thread| s(thread, "id")).collect();
            self.channels.retain(|id, channel| !matches!(i(channel, "type"), 10..=12)
                || b(&channel["thread_metadata"], "archived") || ids.contains(id));
            for thread in threads {
                if self.channels.len() < 2000 || self.channels.contains_key(&s(&thread, "id")) { self.channels.insert(s(&thread, "id"), thread); }
            }
        }
        self.rebuild_channels();
        self.revoke_access();
        self.publish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn delayed_guild_completion_preserves_channels_created_during_initial_load_offline() {
        let mut c=guild_fixture();c.channels.clear();c.guild_acl=None;c.guild_load.begin("50");
        let navigation=c.nav;
        c.dispatch("CHANNEL_CREATE",json!({"id":"25","guild_id":"50","type":2,"name":"Live voice"})).await.unwrap();
        assert_eq!(c.nav,navigation,"A channel event must not cancel the initial guild reads before ACL is known");
        assert!(c.guild_load.is_loading("50"));assert!(c.state.channels.is_empty());
        c.complete(UiAction::id("__guild","50"),json!({"guild":{"id":"50","roles":[{"id":"50","permissions":"1024"}]},"member":{"user":{"id":"1"},"roles":[]},"channels":[{"id":"20","type":0,"name":"REST text"}]})).await.unwrap();
        assert_eq!(c.state.channels.iter().map(|channel|channel.id.as_str()).collect::<Vec<_>>(),["20","25"]);assert!(c.api.is_none());assert!(c.guild_load.guild.is_empty());
    }
    #[test]
    fn guild_rest_replays_live_channel_deltas_before_visibility_and_order_projection() {
        let mut load=GuildLoad::default();load.begin("50");
        load.observe("CHANNEL_CREATE",&json!({"id":"20","guild_id":"50","type":0,"position":0,"name":"Created"}),"1",false);
        load.observe("CHANNEL_DELETE",&json!({"id":"21","guild_id":"50"}),"1",false);
        load.observe("CHANNEL_UPDATE",&json!({"id":"22","guild_id":"50","name":"New name","position":-1,"permission_overwrites":[{"id":"50","type":0,"allow":"0","deny":"1024"}]}),"1",false);
        load.observe("CHANNEL_CREATE",&json!({"id":"23","guild_id":"99","type":2}),"1",false);
        let response=load.reconcile("50",json!({"guild":{"id":"50","roles":[{"id":"50","permissions":"1024"}]},"member":{"roles":[]},"channels":[
            {"id":"21","type":2,"name":"Already deleted"},{"id":"22","type":0,"name":"Old name","position":9}]})).unwrap();
        let channels=items(&response["channels"]);assert_eq!(channels.len(),2);
        assert!(!channels.iter().any(|channel|matches!(s(channel,"id").as_str(),"21"|"23")));
        let acl=GuildAccess::from_values(&response["guild"],&response["member"],"1");
        let visible=parsing::ordered_guild_channels(channels.iter().filter(|channel|acl.evaluate(channel,None).can_view));
        assert_eq!(visible.iter().map(|channel|s(channel,"id")).collect::<Vec<_>>(),["20"]);
        let renamed=channels.iter().find(|channel|s(channel,"id")=="22").unwrap();assert_eq!(s(renamed,"name"),"New name");assert_eq!(i(renamed,"position"),-1);
    }
    #[test]
    fn guild_rest_never_restores_newly_revoked_roles_or_current_membership() {
        let mut load=GuildLoad::default();load.begin("50");
        load.observe("GUILD_ROLE_UPDATE",&json!({"guild_id":"50","role":{"id":"9","permissions":"0"}}),"1",false);
        load.observe("GUILD_MEMBER_UPDATE",&json!({"guild_id":"50","user":{"id":"1"},"roles":[]}),"1",false);
        let response=load.reconcile("50",json!({"guild":{"id":"50","roles":[{"id":"50","permissions":"0"},{"id":"9","permissions":"1024"}]},"member":{"user":{"id":"1"},"roles":["9"]},"channels":[{"id":"20","type":0}]})).unwrap();
        let acl=GuildAccess::from_values(&response["guild"],&response["member"],"1");assert!(!acl.evaluate(&response["channels"][0],None).can_view);
        assert_eq!(response["guild"]["roles"][1]["permissions"],"0");assert_eq!(response["member"]["roles"],json!([]));
    }
    #[test]
    fn fresh_guild_snapshot_replaces_old_channels_and_load_buffer_is_bounded_and_clearable() {
        let mut load=GuildLoad::default();load.begin("50");
        load.observe("GUILD_CREATE",&json!({"id":"50","channels":[{"id":"22","type":2}],"voice_states":[{"session_id":"do-not-retain"}],"token":"do-not-retain"}),"1",false);
        assert!(!load.events[0].1.to_string().contains("do-not-retain"));
        let response=load.reconcile("50",json!({"guild":{"id":"50"},"channels":[{"id":"21","type":0}]})).unwrap();assert_eq!(s(&response["channels"][0],"id"),"22");assert_eq!(items(&response["channels"]).len(),1);
        load.begin("50");for _ in 0..513 {load.observe("CHANNEL_UPDATE",&json!({"id":"22","guild_id":"50","position":1}),"1",true);}
        assert!(load.overflow);assert!(load.events.is_empty());assert!(load.reconcile("50",json!({})).is_err());
        load.begin("50");load.clear();assert!(load.guild.is_empty());assert!(load.events.is_empty());
    }
    fn guild_fixture() -> Controller {
        let (mut c, _) = Controller::test_fixture();
        c.state.current_user = Some(Person { id:"1".into(), ..Default::default() });
        c.state.selected_guild_id = "50".into();
        c.state.guilds = vec![NavItem {id:"50".into(),kind:"guild".into(),..Default::default()}];
        c.guild_acl = Some(GuildAccess::from_values(&json!({"id":"50","roles":[{"id":"50","permissions":"1024"}]}), &json!({"roles":[]}), "1"));
        for channel in [json!({"id":"10","type":4,"position":0}),
            json!({"id":"20","type":0,"parent_id":"10","position":0}),
            json!({"id":"21","type":2,"parent_id":"10","position":0}),
            json!({"id":"22","type":0,"position":0}),
            json!({"id":"23","type":0,"parent_id":"10","permission_overwrites":[{"id":"50","type":0,"deny":"1024","allow":"0"}]}),
            json!({"id":"24","type":13,"parent_id":"10","position":1})] {
            c.channels.insert(s(&channel,"id"), channel);
        }
        c.rebuild_channels(); c
    }
    #[tokio::test]
    async fn guild_create_retains_only_current_thread_membership_before_sidebar_projection() {
        let mut c = guild_fixture();
        c.dispatch("GUILD_CREATE", json!({"id":"50","name":"Fixture guild","threads":[
            {"id":"31","guild_id":"50","type":12,"parent_id":"20","member":{"id":"31","user_id":"1"}},
            {"id":"32","guild_id":"50","type":12,"parent_id":"20","member":{"id":"32","user_id":"9"}}]})).await.unwrap();
        assert!(c.state.channels.iter().any(|item|item.id=="31"));
        assert!(!c.state.channels.iter().any(|item|item.id=="32"));
        assert!(c.channels["32"].get("member").is_none(),"Do not overwrite sanitized membership with the raw Gateway object");
        assert!(!c.thread_catalog.begin_request("50"));
    }
    #[tokio::test]
    async fn channel_updates_and_thread_sync_preserve_order_visibility_and_current_membership() {
        let mut c = guild_fixture();
        assert_eq!(c.state.channels.iter().map(|item|item.id.as_str()).collect::<Vec<_>>(), ["22","10","20","21","24"]);
        c.dispatch("CHANNEL_CREATE", json!({"id":"25","guild_id":"50","type":0,"parent_id":"10","name":"Created","position":1})).await.unwrap();
        c.dispatch("CHANNEL_UPDATE", json!({"id":"25","guild_id":"50","name":"Renamed","position":-1})).await.unwrap();
        assert_eq!(c.state.channels.iter().find(|item|item.id=="25").unwrap().name,"Renamed");
        assert_eq!(c.state.channels.iter().map(|item|item.id.as_str()).collect::<Vec<_>>(),["22","10","25","20","21","24"]);
        c.dispatch("THREAD_LIST_SYNC", json!({"guild_id":"50","threads":[
            {"id":"30","type":11,"parent_id":"20","name":"Public"},
            {"id":"31","type":12,"parent_id":"20","name":"Joined private"},
            {"id":"32","type":12,"parent_id":"20","name":"Unknown private"}],
            "members":[{"id":"31","user_id":"1","flags":0},{"id":"32","user_id":"9"}]})).await.unwrap();
        assert!(c.state.channels.iter().any(|item|item.id=="31"));
        assert!(!c.state.channels.iter().any(|item|item.id=="32"));
        assert!(!c.thread_catalog.begin_request("50"),"Gateway supplied a complete valid catalogue");
        c.dispatch("THREAD_MEMBERS_UPDATE", json!({"guild_id":"50","id":"31","removed_member_ids":["1"]})).await.unwrap();
        assert!(!c.state.channels.iter().any(|item|item.id=="31"));
        c.dispatch("THREAD_MEMBER_UPDATE", json!({"guild_id":"50","id":"31","user_id":"1","flags":0})).await.unwrap();
        assert!(c.state.channels.iter().any(|item|item.id=="31"));
        c.dispatch("THREAD_DELETE", json!({"guild_id":"50","id":"30","type":11,"parent_id":"20"})).await.unwrap();
        c.dispatch("CHANNEL_DELETE", json!({"guild_id":"50","id":"25","type":0})).await.unwrap();
        assert!(!c.state.channels.iter().any(|item|matches!(item.id.as_str(),"25"|"30")));
        c.dispatch("THREAD_LIST_SYNC", json!({"guild_id":"50","channel_ids":["20"],"threads":[],"members":[]})).await.unwrap();
        assert!(!c.state.channels.iter().any(|item|item.kind=="thread"));
    }
    #[test]
    fn stale_thread_rest_response_cannot_replace_gateway_and_real_settings_decorate_channels() {
        let mut c = guild_fixture();
        let stale = c.next_request("guild-threads/50");
        c.next_request("guild-threads/50");
        let mut action = UiAction::id("__guild-threads","50");action.extra=stale.to_string();
        c.complete_guild_threads(&action,&json!({"threads":[{"id":"40","type":11,"parent_id":"20"}]}));
        assert!(c.thread_catalog.get("50").is_none());assert!(!c.channels.contains_key("40"));
        c.notification_settings.apply(&json!({"guild_id":"50","message_notifications":1,"channel_overrides":[{"channel_id":"10","message_notifications":2},{"channel_id":"21","message_notifications":0}]}));
        c.rebuild_channels();
        assert!(c.state.channels.iter().find(|item|item.id=="20").unwrap().notifications_disabled);
        assert!(!c.state.channels.iter().find(|item|item.id=="21").unwrap().notifications_disabled);
        assert!(!c.state.channels.iter().find(|item|item.id=="22").unwrap().notifications_disabled);
        c.muted.insert("21".into(),true); c.rebuild_channels();
        assert!(c.state.channels.iter().find(|item|item.id=="21").unwrap().notifications_disabled);
    }
    #[test]
    fn thread_catalog_is_bounded_reuses_complete_sources_and_drops_untrusted_fields() {
        let mut cache = ThreadCatalog::default();
        assert!(cache.begin_request("10")); assert!(!cache.begin_request("10"));
        cache.replace("10", &[json!({"id":"20","parent_id":"30","type":11,"name":"Thread","token":"do-not-retain"})]);
        assert!(!cache.begin_request("10"));
        assert!(!cache.get("10").unwrap()[0].to_string().contains("do-not-retain"));
        cache.delta("10", &json!({"id":"20","name":"Renamed"}), false);
        assert_eq!(s(&cache.get("10").unwrap()[0], "name"), "Renamed");
        for guild in 11..30 { cache.replace(&guild.to_string(), &[]); }
        assert!(cache.entries.len() <= 8);
        cache.replace("30", &vec![json!({}); 2001]); assert!(cache.get("30").is_none());
    }
}
