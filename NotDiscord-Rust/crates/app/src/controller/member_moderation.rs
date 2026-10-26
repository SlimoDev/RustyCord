use super::*;
use notdiscord_core::member_moderation::{self as policy, MemberModeration};

/// Injectable transport keeps permission tests entirely offline.
async fn moderation_request<F, R>(guild: &str, me: &str, user: &str, intent: Option<(&str, &Value)>, mut request: F) -> Value
where
    F: FnMut(Method, String, Option<Value>, Option<String>) -> R,
    R: std::future::Future<Output = Reply>,
{
    let mut confirmed = false;
    let result: Reply = async {
        if let Some((kind, body)) = intent {policy::validate_intent(kind, body)?;}
        let raw = request(Method::GET, format!("guilds/{guild}"), None, None).await?;
        if s(&raw,"id") != guild {return Err("El servidor recibido no corresponde a esta acción.".into());}
        let actor = request(Method::GET, format!("guilds/{guild}/members/{me}"), None, None).await?;
        let mut target = if user == me {actor.clone()} else {
            request(Method::GET, format!("guilds/{guild}/members/{user}"), None, None).await?
        };
        if s(&target["user"],"id") != user {return Err("El miembro recibido no corresponde al destinatario.".into());}
        let mut capabilities = policy::capabilities(&raw,&actor,&target,me,chrono::Utc::now())?;
        if let Some((kind, body)) = intent {
            let write = policy::prepare_write(&raw,&actor,&target,me,kind,body,chrono::Utc::now())?;
            let method=Method::from_bytes(write.method.as_bytes()).map_err(|_| "Método de moderación no válido.")?;
            request(method,write.path,write.body,write.reason).await?;
            confirmed = true;
            if matches!(kind,"kick"|"ban") {
                capabilities.can_timeout=false;capabilities.can_kick=false;capabilities.can_ban=false;capabilities.can_nickname=false;
                target=Value::Null;
            } else {
                // A partial PATCH or the submitted form is never treated as the canonical member.
                target=request(Method::GET,format!("guilds/{guild}/members/{user}"),None,None).await?;
                if s(&target["user"],"id") != user {return Err("El guardado se confirmó, pero no se pudo recargar al miembro correcto.".into());}
                capabilities=policy::capabilities(&raw,&actor,&target,me,chrono::Utc::now())?;
            }
        }
        Ok(json!({"capabilities":capabilities,"member":target}))
    }.await;
    match result {
        Ok(value)=>json!({"ok":true,"confirmed":confirmed,"value":value}),
        Err(error)=>json!({"ok":false,"confirmed":confirmed,"error":error}),
    }
}

impl Controller {
    fn moderation_context(&self, guild:&str, user:&str)->bool {
        parsing::valid_id(guild) && parsing::valid_id(user)
            && self.state.selected_guild_id==guild
            && self.state.guilds.iter().any(|g|g.id==guild)
            && self.state.signed_in && !self.state.demo
    }
    pub(super) fn invalidate_moderation(&mut self) {
        self.next_request("member-moderation");
        self.state.member_moderation=None;
    }
    pub(super) fn moderation_gateway_delta(&mut self,kind:&str,value:&Value) {
        let Some(state)=&self.state.member_moderation else {return;};
        let guild=if kind=="GUILD_UPDATE"||kind=="GUILD_DELETE" {s(value,"id")} else {s(value,"guild_id")};
        if guild!=state.guild_id {return;}
        let user=s(&value["user"],"id");
        if matches!(kind,"GUILD_ROLE_CREATE"|"GUILD_ROLE_UPDATE"|"GUILD_ROLE_DELETE"|"GUILD_UPDATE"|"GUILD_DELETE")
            || matches!(kind,"GUILD_MEMBER_UPDATE"|"GUILD_MEMBER_REMOVE") && (user==state.actor_id||user==state.user_id) {
            self.invalidate_moderation();
            self.publish();
        }
    }
    pub(super) fn member_moderation_action(&mut self,action:&UiAction)->Result<bool,String> {
        if !matches!(action.kind.as_str(),"member-moderation-load"|"member-moderation") {return Ok(false);}
        let guild=Self::valid(&action.channel_id)?.to_owned();
        let user=Self::valid(&action.id)?.to_owned();
        let me=self.me();Self::valid(&me)?;
        if !self.moderation_context(&guild,&user) {return Err("El contexto del servidor cambió. Vuelve a abrir el menú del miembro.".into());}
        if self.state.member_moderation.as_ref().is_some_and(|s|s.saving) {return Err("Espera a que se confirme la acción de moderación pendiente.".into());}
        let editing=action.kind=="member-moderation";
        let intent=if editing {
            if action.text.len()>8192 {return Err("La modificación supera el límite permitido.".into());}
            let body:Value=serde_json::from_str(&action.text).map_err(|_|"La modificación no es válida.")?;
            let revision=body["revision"].as_u64().ok_or("Vuelve a abrir el menú para confirmar los permisos actuales.")?;
            if self.state.member_moderation.as_ref().is_none_or(|s|s.loading||s.saving||!s.error.is_empty()||s.actor_id!=me||s.guild_id!=guild||s.user_id!=user||s.revision!=revision
                || !match action.extra.as_str(){"timeout"|"untimeout"=>s.can_timeout,"kick"=>s.can_kick,"ban"=>s.can_ban,"nickname"=>s.can_nickname,_=>false})
                || !self.request_current("member-moderation",revision) {
                return Err("Los permisos o el destinatario cambiaron. Vuelve a abrir el menú del miembro.".into());
            }
            policy::validate_intent(&action.extra,&body)?;
            Some((action.extra.clone(),body))
        } else {
            if self.state.member_moderation.as_ref().is_some_and(|s|s.actor_id==me&&s.guild_id==guild&&s.user_id==user&&s.loading) {return Ok(true);}
            None
        };
        let api=self.api()?;api.safety().check().map_err(|e|e.to_string())?;
        let version=self.next_request("member-moderation");
        let previous=self.state.member_moderation.take().filter(|s|s.actor_id==me&&s.guild_id==guild&&s.user_id==user);
        let mut state=previous.unwrap_or_else(||MemberModeration{actor_id:me.clone(),guild_id:guild.clone(),user_id:user.clone(),..Default::default()});
        state.revision=version;state.loading=!editing;state.saving=editing;state.error.clear();state.note.clear();
        // A refresh never leaves stale capabilities actionable while the reads are pending.
        if !editing {state.can_timeout=false;state.can_kick=false;state.can_ban=false;state.can_nickname=false;}
        self.state.member_moderation=Some(state);self.publish();
        let completion=UiAction{kind:"__member-moderation".into(),id:user.clone(),channel_id:guild.clone(),extra:version.to_string(),text:intent.as_ref().map(|(kind,_)|kind.clone()).unwrap_or_default(),..Default::default()};
        // Writes remain session-bound, but navigation does not cancel a request already sent.
        self.launch(completion,if editing {None}else{Some(self.nav)},async move {
            Ok(moderation_request(&guild,&me,&user,intent.as_ref().map(|(kind,body)|(kind.as_str(),body)),move |method,path,body,reason|{
                let api=api.clone();async move {api.send_audited(method,&path,body,reason.as_deref().unwrap_or("")).await.map_err(|e|e.to_string())}
            }).await)
        });
        Ok(true)
    }
    pub(super) fn complete_member_moderation(&mut self,action:&UiAction,value:Value)->Result<(),String> {
        let revision=action.extra.parse::<u64>().unwrap_or(0);
        if !self.request_current("member-moderation",revision)||!self.moderation_context(&action.channel_id,&action.id)
            || self.state.member_moderation.as_ref().is_none_or(|s|s.actor_id!=self.me()||s.guild_id!=action.channel_id||s.user_id!=action.id||s.revision!=revision) {return Ok(());}
        if value["ok"]!=true {
            if let Some(state)=&mut self.state.member_moderation {
                state.loading=false;state.saving=false;state.error=s(&value,"error");
                state.can_timeout=false;state.can_kick=false;state.can_ban=false;state.can_nickname=false;
                if value["confirmed"]==true {state.note="Discord confirmó el cambio, pero no se pudo recargar al miembro. Actualiza antes de volver a modificarlo.".into();}
            }
            self.publish();return Ok(());
        }
        let mut state:MemberModeration=serde_json::from_value(value["value"]["capabilities"].clone()).map_err(|_|"La respuesta de permisos del miembro no es válida.")?;
        if state.actor_id!=self.me()||state.guild_id!=action.channel_id||state.user_id!=action.id {return Err("La respuesta de moderación corresponde a otro contexto.".into());}
        state.revision=revision;
        if value["confirmed"]==true {
            state.note="Cambio confirmado por Discord.".into();
            let member=&value["value"]["member"];
            if member.is_object() {self.remember_guild_nickname(&action.channel_id,&action.id,member);}
            if matches!(action.text.as_str(),"kick"|"ban") {
                self.state.guild_members.retain(|m|m.user.id!=action.id);
                for slot in &mut self.guild_member_slots {if slot.as_ref().is_some_and(|m|m.user.id==action.id){*slot=None;}}
                self.state.guild_voice_members.retain(|m|m.guild_id!=action.channel_id||m.user.id!=action.id);
                self.state.guild_nicknames.remove(&action.id);
            }
        }
        self.state.member_moderation=Some(state);self.publish();Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn responses()->VecDeque<Reply>{
        [json!({"id":"10","owner_id":"9","roles":[{"id":"10","position":0,"permissions":"0"},{"id":"20","position":5,"permissions":"1099712954374"},{"id":"21","position":1,"permissions":"0"}]}),json!({"user":{"id":"1"},"roles":["20"]}),json!({"user":{"id":"2"},"roles":["21"],"nick":"Before"})].into_iter().map(Ok).collect()
    }
    #[tokio::test]
    async fn revoked_permissions_and_wrong_target_never_emit_a_write() {
        for wrong_target in [false,true] {
            let mut replies=responses();if wrong_target{replies[2]=Ok(json!({"user":{"id":"3"},"roles":["21"]}));}else{replies[1]=Ok(json!({"user":{"id":"1"},"roles":[]}));}
            let mut writes=0;
            let result=moderation_request("10","1","2",Some(("kick",&json!({}))),|method,_,_,_|{if method!=Method::GET{writes+=1;}std::future::ready(replies.pop_front().unwrap())}).await;
            assert_eq!(result["ok"],false);assert_eq!(writes,0);
        }
    }
    #[tokio::test]
    async fn canonical_nickname_and_audit_reason_are_not_form_echoes() {
        let mut replies=responses();replies.push_back(Ok(json!({})));replies.push_back(Ok(json!({"user":{"id":"2"},"roles":["21"],"nick":"Canonical"})));
        let mut methods=vec![];
        let value=moderation_request("10","1","2",Some(("nickname",&json!({"nick":"Submitted"}))),|method,path,body,reason|{methods.push((method,path,body,reason));std::future::ready(replies.pop_front().unwrap())}).await;
        assert_eq!(value["ok"],true);assert_eq!(value["value"]["capabilities"]["Nickname"],"Canonical");assert_eq!(methods.len(),5);assert_eq!(methods[3].0,Method::PATCH);assert_eq!(methods[4].0,Method::GET);
        let mut replies=responses();replies.push_back(Ok(json!({})));let mut audit=None;
        let value=moderation_request("10","1","2",Some(("ban",&json!({"reason":"Razón ñ","delete_message_seconds":3600}))),|method,_,_,reason|{if method==Method::PUT{audit=reason;}std::future::ready(replies.pop_front().unwrap())}).await;
        assert_eq!(value["ok"],true);assert_eq!(audit.as_deref(),Some("Razón ñ"));assert_eq!(value["confirmed"],true);
    }
    #[test]
    fn navigation_and_gateway_role_changes_retire_confirmation_revisions() {
        let (mut c,_)=Controller::test_fixture();c.state.signed_in=true;c.state.current_user=Some(Person{id:"1".into(),..Default::default()});c.state.selected_guild_id="10".into();c.state.guilds=vec![NavItem{id:"10".into(),..Default::default()}];
        let revision=c.next_request("member-moderation");c.state.member_moderation=Some(MemberModeration{actor_id:"1".into(),guild_id:"10".into(),user_id:"2".into(),revision,can_ban:true,..Default::default()});
        c.moderation_gateway_delta("GUILD_ROLE_UPDATE",&json!({"guild_id":"10","role":{"id":"20"}}));assert!(c.state.member_moderation.is_none());assert!(!c.request_current("member-moderation",revision));
        let action=UiAction{kind:"member-moderation".into(),id:"2".into(),channel_id:"10".into(),extra:"ban".into(),text:json!({"revision":revision}).to_string(),..Default::default()};assert!(c.member_moderation_action(&action).is_err());
        c.state.selected_guild_id="11".into();assert!(!c.moderation_context("10","2"));
    }
    #[tokio::test]
    async fn failed_confirmation_reload_reports_committed_change_without_echoing_input() {
        let mut replies=responses();replies.push_back(Ok(json!({"nick":"Submitted"})));replies.push_back(Err("Synthetic reload failure".into()));
        let value=moderation_request("10","1","2",Some(("nickname",&json!({"nick":"Submitted"}))),|_,_,_,_|std::future::ready(replies.pop_front().unwrap())).await;
        assert_eq!(value["ok"],false);assert_eq!(value["confirmed"],true);assert!(value.get("value").is_none());
    }
    #[test]
    fn stale_completion_cannot_restore_another_member_or_account() {
        let (mut c,_)=Controller::test_fixture();c.state.signed_in=true;c.state.current_user=Some(Person{id:"1".into(),..Default::default()});c.state.selected_guild_id="10".into();c.state.guilds=vec![NavItem{id:"10".into(),..Default::default()}];
        let revision=c.next_request("member-moderation");
        c.state.member_moderation=Some(MemberModeration{actor_id:"1".into(),guild_id:"10".into(),user_id:"3".into(),revision,..Default::default()});
        let action=UiAction{kind:"__member-moderation".into(),id:"2".into(),channel_id:"10".into(),extra:revision.to_string(),..Default::default()};
        c.complete_member_moderation(&action,json!({"ok":true,"confirmed":true,"value":{"capabilities":MemberModeration{actor_id:"1".into(),guild_id:"10".into(),user_id:"2".into(),can_ban:true,..Default::default()}}})).unwrap();
        assert_eq!(c.state.member_moderation.as_ref().unwrap().user_id,"3");assert!(!c.state.member_moderation.as_ref().unwrap().can_ban);
        c.state.current_user.as_mut().unwrap().id="4".into();
        c.complete_member_moderation(&UiAction{id:"3".into(),..action},json!({"ok":false,"error":"old account error"})).unwrap();
        assert!(c.state.member_moderation.as_ref().unwrap().error.is_empty());
    }
}
