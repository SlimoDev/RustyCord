//! Server member actions use a scoped policy snapshot for presentation. The
//! actor rechecks the server's current permissions before every mutation.
use super::*;
use notdiscord_core::member_moderation::MemberModeration;
#[cfg(test)]
mod tests;

#[derive(Clone)]
struct Load { guild:String, user:String, actor:String, generation:u64, at:Instant }
fn load_key()->egui::Id {egui::Id::new("member-moderation-load")}
fn matches(info:&MemberModeration, person:&str, state:&AppSnapshot)->bool {
    state.signed_in && !state.demo && state.network_safety==notdiscord_core::safety::SafetyState::Allowed
        && !state.selected_guild_id.is_empty() && info.guild_id==state.selected_guild_id
        && info.user_id==person && state.current_user.as_ref().is_some_and(|user|user.id==info.actor_id)
}
pub(super) fn action_error(action:&UiAction,state:&AppSnapshot)->Option<String> {
    if action.kind!="member-moderation" {return None;}
    let payload:Value=serde_json::from_str(&action.text).unwrap_or_default();
    let Some(info)=state.member_moderation.as_ref().filter(|info|matches(info,&action.id,state)
        && info.guild_id==action.channel_id && payload["revision"].as_u64()==Some(info.revision)) else {
        return Some("El contexto o los permisos del miembro cambiaron. Vuelve a abrir la acción.".into());
    };
    if info.loading||info.saving {return Some("Espera a que termine la comprobación de permisos.".into());}
    if !info.error.is_empty() {return Some("No se pudieron confirmar los permisos actuales. Vuelve a comprobarlos.".into());}
    let permitted=match action.extra.as_str() {
        "timeout"|"untimeout"=>info.can_timeout,"kick"=>info.can_kick,"ban"=>info.can_ban,"nickname"=>info.can_nickname,_=>false,
    };
    if !permitted {Some("No tienes permiso para esta acción sobre este miembro.".into())}else{None}
}
pub(super) fn prompt_limit(action:&UiAction)->Option<usize> {
    (action.kind=="member-moderation").then_some(if action.extra=="nickname" {32}else{512})
}
pub(super) fn prompt_error(action:&UiAction,text:&str)->Option<String> {
    let limit=prompt_limit(action)?;
    if text.chars().count()>limit {Some(format!("El texto supera el máximo de {limit} caracteres."))}
    else if text.chars().any(char::is_control) {Some("El texto no puede contener saltos de línea ni caracteres de control.".into())}
    else {None}
}
impl NotDiscordUi {
    fn load_member_moderation(&mut self, ui:&egui::Ui, person:&Person, state:&AppSnapshot, force:bool) {
        let actor=state.current_user.as_ref().map_or("",|user|user.id.as_str());
        let pending=ui.ctx().data(|data|data.get_temp::<Load>(load_key()));
        if !force && pending.is_some_and(|pending|pending.guild==state.selected_guild_id && pending.user==person.id
            && pending.actor==actor && pending.generation==state.notification_generation) {return;}
        notdiscord_core::diagnostics::record(notdiscord_core::diagnostics::Component::Menus,notdiscord_core::diagnostics::Code::MenuDemand,notdiscord_core::diagnostics::Phase::Dispatch,0);
        self.send("member-moderation-load",&person.id,"","",&state.selected_guild_id);
        ui.ctx().data_mut(|data|data.insert_temp(load_key(),Load {guild:state.selected_guild_id.clone(),user:person.id.clone(),
            actor:actor.into(),generation:state.notification_generation,at:Instant::now()}));
    }
    pub(super) fn draw_member_moderation_menu(&mut self, ui:&mut egui::Ui, person:&Person, state:&AppSnapshot) {
        if !state.signed_in || state.demo || !notdiscord_core::parsing::valid_id(&state.selected_guild_id)
            || !notdiscord_core::parsing::valid_id(&person.id) {return;}
        ui.separator();
        if state.network_safety!=notdiscord_core::safety::SafetyState::Allowed {
            ui.weak("Las acciones del servidor están detenidas en esta sesión.");return;
        }
        let Some(info)=state.member_moderation.as_ref().filter(|info|matches(info,&person.id,state)) else {
            self.load_member_moderation(ui,person,state,false);
            ui.weak("Comprobando permisos del servidor…");
            let timed_out=ui.ctx().data(|data|data.get_temp::<Load>(load_key())).is_some_and(|load|load.at.elapsed()>Duration::from_secs(10));
            if timed_out && ui.button("Reintentar comprobación").clicked() {self.load_member_moderation(ui,person,state,true);}
            return;
        };
        ui.ctx().data_mut(|data|data.remove::<Load>(load_key()));
        if info.loading {ui.weak("Comprobando permisos del servidor…");return;}
        if !info.error.is_empty() {
            ui.add(egui::Label::new(RichText::new(&info.error).color(self.theme.muted)).wrap());
            if ui.button("Volver a comprobar permisos").clicked() {self.load_member_moderation(ui,person,state,true);}
            return;
        }
        if !info.note.is_empty() {ui.add(egui::Label::new(&info.note).wrap());}
        let enabled=!info.saving;
        if ui.add_enabled(enabled&&info.can_nickname,egui::Button::new("Cambiar apodo")).clicked() {
            self.open_member_action(person,info,"nickname",None);ui.close();
        }
        ui.add_enabled_ui(enabled&&info.can_timeout,|ui| {
            ui.menu_button("Aislar temporalmente",|ui| {
                for (seconds,label) in [(60,"1 minuto"),(300,"5 minutos"),(600,"10 minutos"),(3600,"1 hora"),
                    (86400,"1 día"),(604800,"7 días"),(2419200,"28 días")] {
                    if ui.button(label).clicked() {self.open_member_action(person,info,"timeout",Some(seconds));ui.close();}
                }
            });
        });
        if info.timed_out && ui.add_enabled(enabled&&info.can_timeout,egui::Button::new("Quitar aislamiento")).clicked() {
            self.open_member_action(person,info,"untimeout",None);ui.close();
        }
        for (operation,label,allowed) in [("kick","Expulsar del servidor",info.can_kick),("ban","Banear del servidor",info.can_ban)] {
            if ui.add_enabled(enabled&&allowed,egui::Button::new(RichText::new(label).color(Color32::from_rgb(242,63,67)))).clicked() {
                self.open_member_action(person,info,operation,None);ui.close();
            }
        }
    }
    fn open_member_action(&mut self,person:&Person,info:&MemberModeration,operation:&str,seconds:Option<u64>) {
        let title=match operation {"timeout"=>"Aislar temporalmente","untimeout"=>"Quitar aislamiento","kick"=>"Expulsar del servidor","ban"=>"Banear del servidor",_=>"Cambiar apodo"};
        let mut payload=serde_json::json!({"revision":info.revision,"targetName":person.name});
        if let Some(seconds)=seconds {payload["duration_seconds"]=serde_json::json!(seconds);}
        if operation=="ban" {payload["delete_message_seconds"]=serde_json::json!(0);}
        self.prompt=if operation=="nickname" {info.nickname.clone()}else{String::new()};
        self.dialog_error.clear();
        self.dialog=Some(Dialog::Prompt {title:format!("{title} · {}",person.name),
            description:if operation=="nickname" {"Apodo en este servidor (máximo 32 caracteres). Déjalo vacío para quitarlo.".into()}
                else {"Motivo opcional (máximo 512 caracteres). La siguiente pantalla permite confirmar el destino y la acción.".into()},
            action:UiAction {kind:"member-moderation".into(),id:person.id.clone(),channel_id:info.guild_id.clone(),
                extra:operation.into(),text:payload.to_string()},multiline:operation!="nickname"});
    }
    pub(super) fn member_moderation_submission(&self, action:&UiAction, state:&AppSnapshot)->Result<(UiAction,Option<Dialog>),String> {
        if let Some(error)=action_error(action,state) {return Err(error);}
        if let Some(error)=prompt_error(action,&self.prompt) {return Err(error);}
        let mut payload:Value=serde_json::from_str(&action.text).map_err(|_|"No se pudo preparar la acción.".to_owned())?;
        let target=payload["targetName"].as_str().unwrap_or("este miembro").to_owned();
        payload.as_object_mut().unwrap().remove("targetName");
        payload[if action.extra=="nickname" {"nick"}else{"reason"}]=serde_json::json!(self.prompt);
        let mut action=action.clone();action.text=payload.to_string();
        if action.extra=="nickname" {return Ok((action,None));}
        let effect=match action.extra.as_str() {
            "timeout"=>format!("Aislar temporalmente a {target} durante {} segundos",payload["duration_seconds"].as_u64().unwrap_or(0)),
            "untimeout"=>format!("Quitar el aislamiento de {target}"),"kick"=>format!("Expulsar a {target} del servidor"),
            "ban"=>format!("Banear a {target} del servidor sin borrar mensajes anteriores"),_=>return Err("Acción no disponible.".into()),
        };
        let description=format!("{effect}.\nMotivo: {}",if self.prompt.trim().is_empty() {"Sin motivo especificado"}else{self.prompt.trim()});
        let dialog=Dialog::Confirm {title:"Confirmar acción del servidor".into(),description,action:action.clone()};
        Ok((action,Some(dialog)))
    }
}
