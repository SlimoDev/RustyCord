//! On-demand, read-only reaction tooltips on messages already received.
use super::*;
#[derive(Default)]
pub(super) struct Requests {
    pending: Option<String>, queued: Option<UiAction>,
    pending_key: Option<(String, String, String)>, invalidated: bool,
}
impl Requests {
    pub(super) fn clear_navigation(&mut self) { self.queued=None; self.invalidated=true; }
}
fn emoji_matches(key: &str, emoji: &str, id: &str) -> bool {
    key == emoji || !id.is_empty() && key.rsplit_once(':').is_some_and(|(_, value)| value == id)
}
fn metadata(action: &UiAction) -> Value {
    let extra: Value = if action.extra.len() <= 2048 { serde_json::from_str(&action.extra).unwrap_or_default() } else { Value::Null };
    json!({"requestId":s(&extra,"requestId").chars().take(64).collect::<String>(),"channelId":action.channel_id,
        "messageId":action.id,"emoji":action.text,"kind":extra["kind"].as_u64().unwrap_or(0),"users":[],"hasMore":false,"error":""})
}
fn route(channel: &str, message: &str, emoji: &str, kind: u64) -> Result<String,String> {
    if !parsing::valid_id(channel) || !parsing::valid_id(message) || kind > 1 {return Err("La reacción no está disponible en este contexto.".into());}
    let key = super::social::reaction_key(emoji)?;
    Ok(format!("channels/{channel}/messages/{message}/reactions/{}?limit=100&type={kind}",url::form_urlencoded::byte_serialize(key.as_bytes()).collect::<String>()))
}
fn users(value: &Value) -> Result<(Vec<Person>,bool),String> {
    let values = value.as_array().ok_or("El servidor no confirmó la lista de reacciones.")?;
    let mut seen = HashSet::new();
    let people = values.iter().take(100).map(parsing::person).filter(|person|parsing::valid_id(&person.id) && seen.insert(person.id.clone())).collect();
    Ok((people, values.len() >= 100))
}
async fn execute_query<F,R>(mut result: Value, endpoint: String, get: F) -> Value
where F: FnOnce(String)->R,
    R: std::future::Future<Output=Result<Value,String>> {
    match get(endpoint).await {
        Ok(value)=>match users(&value) {
            Ok((people,more))=>{result["users"]=json!(people);result["hasMore"]=json!(more);},
            Err(error)=>result["error"]=json!(error),
        },
        Err(error)=>result["error"]=json!(error.chars().take(512).collect::<String>()),
    }
    result
}
impl Controller {
    pub(super) fn invalidate_reaction_users(&mut self, kind: &str, value: &Value) {
        let channel=s(value,"channel_id");let message=s(value,"message_id");
        if !parsing::valid_id(&channel)||!parsing::valid_id(&message) {return;}
        let emoji=StatusEmoji {id:s(&value["emoji"],"id"),name:s(&value["emoji"],"name"),animated:false};
        let key=emoji.reaction_key();let all=kind=="MESSAGE_REACTION_REMOVE_ALL"||key.is_empty();
        let pending=self.reaction_queries.pending_key.as_ref().is_some_and(|(pending_channel,pending_message,pending_emoji)|
            pending_channel==&channel&&pending_message==&message&&(all||emoji_matches(pending_emoji,&key,&emoji.id)));
        let observed=self.state.messages.iter().chain(&self.state.pinned_messages).any(|value|
            value.channel_id==channel&&value.id==message&&!value.deleted);
        if !pending&&!observed {return;}
        if pending {
            self.reaction_queries.invalidated=true;
        }
        // Use the existing correlated response channel, including while a GET
        // is pending. Equal-count add/remove pairs must still retire old lists.
        self.data("reaction-users",json!({"event":"invalidate","channelId":channel,"messageId":message,
            "emoji":key,"emojiId":emoji.id,"all":all}));
    }
    pub(super) fn request_reaction_users(&mut self, mut action: UiAction) {
        let mut result = metadata(&action);
        if self.reaction_queries.pending.is_some() {
            if self.reaction_queries.pending.as_deref()!=Some(s(&result,"requestId").as_str())
                && action.extra.len()<=2048 && action.text.len()<=512
                && parsing::valid_id(&action.id) && parsing::valid_id(&action.channel_id)
                && uuid::Uuid::parse_str(&s(&result,"requestId")).is_ok() {
                self.reaction_queries.queued=Some(action);
            }
            return;
        }
        let prepared = (|| {
            if uuid::Uuid::parse_str(&s(&result,"requestId")).is_err() { return Err("Solicitud de reacción no válida.".to_owned()); }
            let message = self.state.messages.iter().chain(&self.state.pinned_messages)
                .find(|message|message.id==action.id && message.channel_id==action.channel_id && !message.deleted).ok_or("El mensaje ya no está disponible.")?;
            let key = super::social::reaction_key(&action.text)?;
            if !message.reactions.iter().any(|reaction|reaction.emoji.reaction_key()==key) {return Err("Esta reacción ya no está disponible.".into());}
            // Confirmed revocations win immediately. Search may include other
            // channels; the server also enforces this read-only GET's ACL.
            let access = if action.channel_id==self.state.selected_channel_id {Some(self.state.channel_access.clone())}
                else {self.channels.get(&action.channel_id).map(|channel|self.channel_access(channel))};
            if access.is_some_and(|access|access.known && (!access.can_view || !access.can_read_history)) {return Err("No puedes ver el historial de este canal.".into());}
            let endpoint = route(&action.channel_id,&action.id,&key,result["kind"].as_u64().unwrap_or(0))?;
            Ok((self.api()?,endpoint))
        })();
        match prepared {
            Err(error) => {result["error"]=json!(error);self.data("reaction-users",result);}
            Ok((api,endpoint)) => {
                self.reaction_queries.pending=Some(s(&result,"requestId"));
                self.reaction_queries.pending_key=Some((action.channel_id.clone(),action.id.clone(),action.text.clone()));
                self.reaction_queries.invalidated=false;
                action.kind="__reaction-users".into();action.extra=result.to_string();
                self.launch(action,None,async move {
                    let mut timeout_result=result.clone();
                    let query=execute_query(result,endpoint,move |path| async move {
                        api.get(&path).await.map_err(|error|error.to_string())
                    });
                    // Dropping a timed-out read releases this slot. The API's
                    // shared rate deadlines remain in force for later hovers.
                    Ok(match tokio::time::timeout(Duration::from_secs(10),query).await {
                        Ok(result)=>result,
                        Err(_)=>{
                            timeout_result["error"]=json!("La consulta tardó demasiado. Vuelve a intentarlo en unos segundos.");
                            timeout_result
                        }
                    })
                });
            }
        }
    }
    pub(super) fn complete_reaction_users(&mut self, mut result: Value) {
        if self.reaction_queries.pending.as_deref()!=Some(s(&result,"requestId").as_str()) {return;}
        self.reaction_queries.pending=None;
        self.reaction_queries.pending_key=None;
        let invalidated=std::mem::take(&mut self.reaction_queries.invalidated);
        let channel=s(&result,"channelId");
        let access=if channel==self.state.selected_channel_id {Some(self.state.channel_access.clone())}
            else {self.channels.get(&channel).map(|channel|self.channel_access(channel))};
        let revoked=access.is_some_and(|access|access.known && (!access.can_view || !access.can_read_history));
        let available=self.state.messages.iter().chain(&self.state.pinned_messages).any(|message|
            message.id==s(&result,"messageId") && message.channel_id==channel && !message.deleted
            && message.reactions.iter().any(|reaction|reaction.emoji.reaction_key()==s(&result,"emoji")));
        if invalidated || revoked || !available {
            result["users"]=json!([]);result["hasMore"]=json!(false);
            result["invalidated"]=json!(invalidated);
            result["error"]=json!(if revoked {"No puedes ver el historial de este canal."} else {"Esta reacción ya no está disponible."});
        }
        self.data("reaction-users",result);
        if let Some(action)=self.reaction_queries.queued.take() {self.request_reaction_users(action);}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tooltip_routes_escape_emoji_and_bound_the_server_page() {
        assert_eq!(route("20","30","😀",0).unwrap(),"channels/20/messages/30/reactions/%F0%9F%98%80?limit=100&type=0");
        assert_eq!(route("20","30","<a:wave:123>",1).unwrap(),"channels/20/messages/30/reactions/wave%3A123?limit=100&type=1");
        assert!(route("20/21","30","😀",0).is_err());assert!(route("20","30","😀",2).is_err());
        let (people,more)=users(&json!([{"id":"1","username":"One"},{"id":"1","username":"Duplicate"},{"id":"bad"}])).unwrap();
        assert_eq!(people.len(),1);assert!(!more);assert!(users(&json!({})).is_err());
    }
    #[test]
    fn tooltip_requires_known_reaction_and_respects_confirmed_revocations_without_network() {
        let (mut c,updates)=Controller::test_fixture();
        let mut action=UiAction::id("reaction-users","30");action.channel_id="20".into();action.text="😀".into();
        action.extra=json!({"requestId":uuid::Uuid::new_v4().to_string(),"kind":0}).to_string();
        c.request_reaction_users(action.clone());
        let errors=||updates.try_iter().filter_map(|update|match update {UiUpdate::Data(kind,data) if kind=="reaction-users"=>Some(data),_=>None}).collect::<Vec<_>>();
        assert!(s(&errors()[0],"error").contains("mensaje"));
        c.state.selected_channel_id="20".into();c.state.channel_access=ChannelAccess {known:true,..Default::default()};
        c.state.messages=vec![ChatMessage {id:"30".into(),channel_id:"20".into(),reactions:vec![MessageReaction {emoji:StatusEmoji {name:"😀".into(),..Default::default()},count:1,..Default::default()}],..Default::default()}].into();
        c.request_reaction_users(action);assert!(s(&errors()[0],"error").contains("historial"));assert!(c.api.is_none());
    }
    #[test]
    fn tooltip_discards_people_if_access_or_the_reaction_changes_while_the_get_is_pending() {
        let (mut c,updates)=Controller::test_fixture();
        c.state.selected_channel_id="20".into();
        c.state.messages=vec![ChatMessage {id:"30".into(),channel_id:"20".into(),reactions:vec![MessageReaction {
            emoji:StatusEmoji {name:"😀".into(),..Default::default()},count:1,..Default::default()}],..Default::default()}].into();
        for revoked in [true,false] {
            c.state.channel_access=if revoked {ChannelAccess {known:true,..Default::default()}} else {ChannelAccess::private()};
            if !revoked {c.state.messages=vec![].into();}
            let mut result=metadata(&request("30"));
            c.reaction_queries.pending=Some(s(&result,"requestId"));
            result["users"]=json!([Person {id:"40".into(),name:"Synthetic person".into(),..Default::default()}]);
            result["hasMore"]=json!(true);
            c.complete_reaction_users(result);
            let delivered=replies(&updates);
            assert_eq!(delivered.len(),1);
            assert_eq!(delivered[0]["users"],json!([]));
            assert_eq!(delivered[0]["hasMore"],false);
            assert!(!s(&delivered[0],"error").is_empty());
            assert!(c.reaction_queries.pending.is_none());
        }
    }
    fn request(message: &str) -> UiAction {
        UiAction { kind:"reaction-users".into(),id:message.into(),channel_id:"20".into(),text:"😀".into(),
            extra:json!({"requestId":uuid::Uuid::new_v4().to_string(),"kind":0}).to_string() }
    }
    fn replies(updates: &std::sync::mpsc::Receiver<UiUpdate>) -> Vec<Value> {
        updates.try_iter().filter_map(|update| match update {
            UiUpdate::Data(kind,value) if kind=="reaction-users"=>Some(value),_=>None
        }).collect()
    }
    #[test]
    fn tooltip_coalesces_to_one_latest_query_and_ignores_foreign_completions() {
        let (mut c,updates)=Controller::test_fixture();
        let first=request("30");let replaced=request("31");let latest=request("32");
        let first_result=metadata(&first);let latest_result=metadata(&latest);
        c.reaction_queries.pending=Some(s(&first_result,"requestId"));
        c.request_reaction_users(replaced);
        c.request_reaction_users(latest.clone());
        c.request_reaction_users(first.clone());
        assert_eq!(c.reaction_queries.queued.as_ref().unwrap().id,"32");
        assert!(replies(&updates).is_empty());
        let mut foreign=first_result.clone();foreign["requestId"]=json!(uuid::Uuid::new_v4().to_string());
        c.complete_reaction_users(foreign);
        assert!(c.reaction_queries.pending.is_some());
        assert_eq!(c.reaction_queries.queued.as_ref().unwrap().id,"32");
        assert!(replies(&updates).is_empty());
        c.complete_reaction_users(first_result);
        let delivered=replies(&updates);
        assert_eq!(delivered.len(),2);
        assert_eq!(delivered[0]["messageId"],"30");
        assert_eq!(delivered[1]["requestId"],latest_result["requestId"]);
        assert_eq!(delivered[1]["messageId"],"32");
        assert!(s(&delivered[1],"error").contains("mensaje"));
        assert!(c.reaction_queries.pending.is_none() && c.reaction_queries.queued.is_none());
        assert!(c.api.is_none(),"coalescing tests must not use a network transport");
    }
    #[test]
    fn tooltip_navigation_clears_the_queue_and_session_reset_rejects_old_data() {
        let (mut c,updates)=Controller::test_fixture();
        let first=request("30");let obsolete=request("31");let next=request("32");
        let first_result=metadata(&first);
        c.reaction_queries.pending=Some(s(&first_result,"requestId"));
        c.request_reaction_users(obsolete);
        c.cancel_view_requests();
        assert!(c.reaction_queries.queued.is_none());
        // The existing GET retains its one bounded slot while navigation can
        // enqueue only the latest desired reaction for the new view.
        assert_eq!(c.reaction_queries.pending.as_deref(),first_result["requestId"].as_str());
        c.request_reaction_users(next);
        c.complete_reaction_users(first_result.clone());
        let delivered=replies(&updates);
        assert_eq!(delivered.len(),2);
        assert_eq!(delivered[1]["messageId"],"32");
        assert!(delivered.iter().all(|value|value["messageId"]!="31"));
        c.reaction_queries.pending=Some(s(&first_result,"requestId"));
        c.reaction_queries.queued=Some(request("33"));
        let stale=Completed {generation:c.generation,nav:None,action:UiAction::new("__reaction-users"),
            result:Ok(first_result.clone()),credential:None};
        c.generation=c.generation.wrapping_add(1);
        c.reset_session_view();
        assert!(!c.accepts(&stale));
        c.complete_reaction_users(first_result);
        assert!(replies(&updates).is_empty());
        assert!(c.reaction_queries.pending.is_none() && c.reaction_queries.queued.is_none());
        assert!(c.api.is_none());
    }
    #[tokio::test]
    async fn tooltip_get_transport_uses_the_exact_route_and_preserves_correlated_results_and_errors() {
        let mut action=request("30");action.text="wave:123".into();
        let token=uuid::Uuid::new_v4().to_string();
        action.extra=json!({"requestId":token,"kind":1}).to_string();
        let expected=metadata(&action);
        let path=route("20","30",&action.text,1).unwrap();
        let observed=Arc::new(Mutex::new(Vec::<String>::new()));
        let seen=observed.clone();
        let result=execute_query(expected.clone(),path.clone(),move |path| async move {
            seen.lock().unwrap().push(path);
            Ok(json!([{"id":"701","username":"reader","global_name":"Reaction Reader","bot":true},
                {"id":"701","username":"duplicate"},{"id":"invalid"}]))
        }).await;
        assert_eq!(*observed.lock().unwrap(),vec!["channels/20/messages/30/reactions/wave%3A123?limit=100&type=1"]);
        for key in ["requestId","channelId","messageId","emoji","kind"] {assert_eq!(result[key],expected[key]);}
        let people:Vec<Person>=serde_json::from_value(result["users"].clone()).unwrap();
        assert_eq!(people.len(),1);assert_eq!(people[0].id,"701");
        assert_eq!(people[0].display_name,"Reaction Reader");assert!(people[0].is_bot);
        assert_eq!(result["hasMore"],false);assert_eq!(result["error"],"");
        let malformed=execute_query(expected.clone(),path.clone(),|_|async{Ok(json!({"unexpected":"object"}))}).await;
        assert!(s(&malformed,"error").contains("lista"));assert_eq!(malformed["users"],json!([]));
        let failed=execute_query(expected.clone(),path,|_|async{Err("Synthetic offline transport failure".into())}).await;
        assert_eq!(failed["requestId"],expected["requestId"]);
        assert_eq!(failed["users"],json!([]));assert_eq!(s(&failed,"error"),"Synthetic offline transport failure");
        assert_eq!(observed.lock().unwrap().len(),1,"the read closure is invoked once; this path has no mutation or retry callback");
    }
}
