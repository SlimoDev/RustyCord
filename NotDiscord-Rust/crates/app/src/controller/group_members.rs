//! Owner-only group-DM removal. No guild moderation or channel deletion is used here.
use super::*;

fn removal_route(channel:&Value, expected:&str, me:&str, target:&str)->Result<String,String> {
    if !parsing::valid_id(expected)||!parsing::valid_id(me)||!parsing::valid_id(target)||target==me {
        return Err("No puedes eliminarte con esta opción ni usar un destinatario no válido.".into());
    }
    if s(channel,"id")!=expected||channel["type"].as_i64()!=Some(3)||!s(channel,"guild_id").is_empty() {
        return Err("La conversación ya no corresponde a este grupo privado.".into());
    }
    if s(channel,"owner_id")!=me {
        return Err("Sólo el propietario actual puede eliminar miembros de este grupo.".into());
    }
    let recipients=channel["recipients"].as_array().ok_or("No se pudo confirmar la lista actual del grupo.")?;
    if recipients.len()>25||recipients.iter().any(|user|!parsing::valid_id(&s(user,"id")))
        || !recipients.iter().any(|user|s(user,"id")==target) {
        return Err("Esa persona ya no pertenece a la lista confirmada del grupo.".into());
    }
    Ok(format!("channels/{expected}/recipients/{target}"))
}

async fn remove_group_member_request<F,R>(channel:&str,me:&str,target:&str,mut request:F)->Reply
where F:FnMut(Method,String,Option<Value>)->R,R:std::future::Future<Output=Reply> {
    if !parsing::valid_id(channel)||!parsing::valid_id(me)||!parsing::valid_id(target)||me==target {
        return Err("El grupo o el destinatario no son válidos.".into());
    }
    let current=request(Method::GET,format!("channels/{channel}"),None).await?;
    let path=removal_route(&current,channel,me,target)?;
    request(Method::DELETE,path,None).await?;
    Ok(json!({"confirmed":true,"actor":me,"channel":channel,"user":target}))
}

fn request_key(channel:&str,target:&str)->String {format!("group-remove/{channel}/{target}")}

impl Controller {
    pub(super) fn group_refresh_action(&mut self,channel:&str)->UiAction {
        let token=self.next_request(&format!("group-members/{channel}"));
        UiAction{kind:"__group".into(),id:channel.into(),channel_id:channel.into(),extra:token.to_string(),..Default::default()}
    }
    pub(super) fn group_refresh_current(&self,action:&UiAction,value:&Value)->bool {
        let channel=&self.state.selected_channel_id;
        !channel.is_empty()&&self.state.selected_guild_id.is_empty()&&self.state.selected_channel_kind=="group"
            &&action.id==*channel&&s(value,"id")==*channel&&value["type"].as_i64()==Some(3)
            &&self.request_current(&format!("group-members/{channel}"),action.extra.parse::<u64>().unwrap_or(0))
    }
    pub(super) fn remove_group_member_action(&mut self,action:&UiAction)->Result<bool,String> {
        if action.kind!="remove-group-member" {return Ok(false);}
        let channel=Self::valid(&action.channel_id)?.to_owned();let target=Self::valid(&action.id)?.to_owned();
        let me=self.me();Self::valid(&me)?;
        let context=action.extra.parse::<u64>().map_err(|_|"Vuelve a abrir el menú del miembro en el grupo actual.")?;
        if !self.state.signed_in||self.state.demo||self.state.view!="chat"||!self.state.selected_guild_id.is_empty()
            ||self.state.selected_channel_id!=channel||self.state.selected_channel_kind!="group"
            ||self.state.group_owner_id!=me||context!=self.state.member_context_version||target==me
            ||!self.state.group_members.iter().any(|member|member.user.id==target)
            ||!self.state.group_members.iter().any(|member|member.user.id==me&&member.is_owner) {
            return Err("El grupo, su propietario o el miembro cambiaron. Vuelve a abrir esta acción.".into());
        }
        if self.state.group_member_removal_pending_ids.contains(&target) {return Err("Espera a que se confirme la eliminación pendiente.".into());}
        let api=self.api()?;api.safety().check().map_err(|error|error.to_string())?;
        let token=self.next_request(&request_key(&channel,&target));
        self.state.group_member_removal_pending_ids.push(target.clone());self.publish();
        let done=UiAction{kind:"__group-member-removed".into(),id:target.clone(),channel_id:channel.clone(),extra:json!({"actor":me,"nav":self.nav,"context":context,"token":token}).to_string(),..Default::default()};
        // Navigation must not interrupt a DELETE already sent. Its result is still scoped below.
        self.launch(done,None,async move {
            remove_group_member_request(&channel,&me,&target,move|method,path,body|{let api=api.clone();async move{api.send(method,&path,body).await.map_err(|error|error.to_string())}}).await
        });
        Ok(true)
    }
    fn removal_completion_current(&self,action:&UiAction,scope:&Value)->bool {
        self.request_current(&request_key(&action.channel_id,&action.id),scope["token"].as_u64().unwrap_or(0))
            &&scope["actor"].as_str()==Some(self.me().as_str())&&scope["nav"].as_u64()==Some(self.nav)
            &&self.state.selected_channel_id==action.channel_id&&self.state.selected_channel_kind=="group"
            &&self.state.selected_guild_id.is_empty()&&self.state.signed_in&&!self.state.demo
    }
    pub(super) fn finish_group_member_removal(&mut self,action:&UiAction) {
        let scope:Value=serde_json::from_str(&action.extra).unwrap_or_default();
        if self.removal_completion_current(action,&scope) {
            self.state.group_member_removal_pending_ids.retain(|id|id!=&action.id);self.publish();
        }
    }
    pub(super) fn complete_group_member_removal(&mut self,action:&UiAction,value:&Value) {
        let scope:Value=serde_json::from_str(&action.extra).unwrap_or_default();
        if !self.removal_completion_current(action,&scope) {return;}
        self.finish_group_member_removal(action);
        if value["confirmed"]!=true||s(value,"actor")!=self.me()||s(value,"channel")!=action.channel_id||s(value,"user")!=action.id {return;}
        // Invalidate pre-DELETE reads even when another membership change raced this ACK.
        self.next_request(&format!("group-members/{}",action.channel_id));
        if scope["context"].as_u64()!=Some(self.state.member_context_version) {
            // A rejoin or owner transfer may be newer than the DELETE; reload instead of replaying an old delta.
            if let Some(api)=self.api.clone().filter(|api|api.safety().check().is_ok()) {
                let channel=action.channel_id.clone();let done=self.group_refresh_action(&channel);
                self.launch(done,Some(self.nav),async move{api.get(&format!("channels/{channel}")).await.map_err(|error|error.to_string())});
            }
            self.state.status="Eliminación confirmada; actualizando la lista actual del grupo.".into();self.publish();return;
        }
        // A matching membership revision lets this ACK retire the old invitation.
        // A raced rejoin must instead wait for the canonical roster above.
        self.observe_private_recipient("CHANNEL_RECIPIENT_REMOVE",&json!({"channel_id":action.channel_id,"user_id":action.id}));
        self.state.group_members.retain(|member|member.user.id!=action.id);
        if let Some(person)=self.state.relationships.iter().find(|r|r.kind==1&&r.user.id==action.id).map(|r|r.user.clone())
            && !self.state.group_recipient_candidates.iter().any(|p|p.id==person.id) {
            self.state.group_recipient_candidates.push(person);
        }
        self.state.member_context_version=self.state.member_context_version.wrapping_add(1);
        self.state.status="Miembro eliminado del grupo; cambio confirmado por Discord.".into();
        self.search_people();self.publish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn channel()->Value{json!({"id":"10","type":3,"owner_id":"1","recipients":[{"id":"2","username":"Target"},{"id":"3","username":"Other"}]})}
    #[tokio::test]
    async fn fresh_owner_membership_and_group_type_are_required_before_any_delete() {
        let mut variants=vec![];
        let mut raw=channel();raw["owner_id"]=json!("3");variants.push(raw);
        let mut raw=channel();raw["type"]=json!(1);variants.push(raw);
        let mut raw=channel();raw["id"]=json!("11");variants.push(raw);
        let mut raw=channel();raw["recipients"]=json!([{"id":"3"}]);variants.push(raw);
        let mut raw=channel();raw["recipients"]=Value::Null;variants.push(raw);
        for raw in variants {
            let mut methods=vec![];let result=remove_group_member_request("10","1","2",|method,_,_|{methods.push(method);std::future::ready(Ok(raw.clone()))}).await;
            assert!(result.is_err());assert_eq!(methods,vec![Method::GET]);
        }
        for (group,me,target) in [("10","1","1"),("bad","1","2"),("10","1","bad")] {
            let mut reads=0;assert!(remove_group_member_request(group,me,target,|_,_,_|{reads+=1;std::future::ready(Ok(channel()))}).await.is_err());assert_eq!(reads,0);
        }
    }
    #[tokio::test]
    async fn removal_only_uses_get_and_exact_recipient_delete_without_body() {
        let mut requests=vec![];
        let result=remove_group_member_request("10","1","2",|method,path,body|{let value=if method==Method::GET{channel()}else{json!({})};requests.push((method,path,body));std::future::ready(Ok(value))}).await.unwrap();
        assert_eq!(requests,vec![(Method::GET,"channels/10".into(),None),(Method::DELETE,"channels/10/recipients/2".into(),None)]);
        assert_eq!(result["confirmed"],true);
        let result=remove_group_member_request("10","1","2",|method,_,_|std::future::ready(if method==Method::GET{Ok(channel())}else{Err("Synthetic DELETE rejected".into())})).await;
        assert!(result.is_err());
    }
    fn fixture()->Controller {
        let(mut c,_)=Controller::test_fixture();c.state.signed_in=true;c.state.current_user=Some(Person{id:"1".into(),..Default::default()});c.state.view="chat".into();c.state.selected_channel_id="10".into();c.state.selected_channel_kind="group".into();c.refresh_group(&channel());c
    }
    fn completion(c:&mut Controller)->UiAction {
        let token=c.next_request(&request_key("10","2"));c.state.group_member_removal_pending_ids.push("2".into());
        UiAction{kind:"__group-member-removed".into(),id:"2".into(),channel_id:"10".into(),extra:json!({"actor":"1","nav":c.nav,"context":c.state.member_context_version,"token":token}).to_string(),..Default::default()}
    }
    #[test]
    fn only_ack_removes_target_and_stale_context_account_or_navigation_do_not_mutate_members() {
        let ack=json!({"confirmed":true,"actor":"1","channel":"10","user":"2"});
        let mut c=fixture();let action=completion(&mut c);assert!(c.state.group_members.iter().any(|m|m.user.id=="2"));
        c.complete_group_member_removal(&action,&ack);assert!(!c.state.group_members.iter().any(|m|m.user.id=="2"));assert!(c.state.group_members.iter().any(|m|m.user.id=="3"));assert!(c.state.group_member_removal_pending_ids.is_empty());
        for mode in 0..4 {
            let mut c=fixture();let action=completion(&mut c);
            match mode{0=>c.nav+=1,1=>c.state.current_user.as_mut().unwrap().id="4".into(),2=>c.state.member_context_version+=1,_=>{c.state.selected_channel_id="11".into();}}
            c.complete_group_member_removal(&action,&ack);assert!(c.state.group_members.iter().any(|m|m.user.id=="2"));
        }
        let mut c=fixture();let action=completion(&mut c);c.finish_group_member_removal(&action);assert!(c.state.group_member_removal_pending_ids.is_empty());assert!(c.state.group_members.iter().any(|m|m.user.id=="2"));
    }
    #[test]
    fn group_owner_and_membership_revision_survive_partial_updates_but_change_with_transfer() {
        let mut c=fixture();let original=c.state.member_context_version;
        c.refresh_group(&json!({"id":"10","type":3,"name":"Renamed"}));assert_eq!(c.state.group_owner_id,"1");assert_eq!(c.state.group_members.len(),3);assert_eq!(c.state.member_context_version,original);
        c.refresh_group(&json!({"id":"10","type":3,"owner_id":"3"}));assert_eq!(c.state.group_owner_id,"3");assert!(c.state.group_members.iter().find(|m|m.user.id=="3").unwrap().is_owner);assert_ne!(c.state.member_context_version,original);
    }
    #[tokio::test]
    async fn delayed_roster_read_cannot_restore_recipient_after_confirmed_delete() {
        let mut c=fixture();let old=c.group_refresh_action("10");let done=completion(&mut c);
        c.complete_group_member_removal(&done,&json!({"confirmed":true,"actor":"1","channel":"10","user":"2"}));
        assert!(!c.group_refresh_current(&old,&channel()));c.social_complete(old,channel()).await.unwrap();
        assert!(!c.state.group_members.iter().any(|m|m.user.id=="2"));
        let new=c.group_refresh_action("10");let mut current=channel();current["recipients"]=json!([{"id":"3","username":"Other"}]);
        assert!(c.group_refresh_current(&new,&current));c.social_complete(new,current).await.unwrap();assert_eq!(c.state.group_members.len(),2);
    }
    #[tokio::test]
    async fn two_confirmed_removals_invalidate_old_reads_and_reload_when_membership_races() {
        let mut c=fixture();let first=completion(&mut c);let before=c.group_refresh_action("10");
        let token=c.next_request(&request_key("10","3"));c.state.group_member_removal_pending_ids.push("3".into());
        let second=UiAction{kind:"__group-member-removed".into(),id:"3".into(),channel_id:"10".into(),extra:json!({"actor":"1","nav":c.nav,"context":c.state.member_context_version,"token":token}).to_string(),..Default::default()};
        c.complete_group_member_removal(&first,&json!({"confirmed":true,"actor":"1","channel":"10","user":"2"}));
        let middle=c.group_refresh_action("10");
        c.complete_group_member_removal(&second,&json!({"confirmed":true,"actor":"1","channel":"10","user":"3"}));
        assert!(!c.group_refresh_current(&before,&channel()));assert!(!c.group_refresh_current(&middle,&channel()));assert!(c.state.group_member_removal_pending_ids.is_empty());
        assert!(c.state.group_members.iter().any(|member|member.user.id=="3"),"A raced ACK must wait for a canonical roster");
        let fresh=c.group_refresh_action("10");c.social_complete(fresh,json!({"id":"10","type":3,"owner_id":"1","recipients":[{"id":"4","username":"Concurrent new member"}]})).await.unwrap();
        assert!(!c.state.group_members.iter().any(|member|matches!(member.user.id.as_str(),"2"|"3")));assert!(c.state.group_members.iter().any(|member|member.user.id=="4"));assert!(c.api.is_none());
    }
}
