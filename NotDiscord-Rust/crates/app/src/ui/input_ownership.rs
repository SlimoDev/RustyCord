//! A field is chosen before its TextEdit processes events. Modal ownership is
//! established before drawing the shell; ordinary typing never clicks through.
use super::*;

#[derive(Clone, Default)]
struct Owner {
    scope: Option<egui::Id>, opened: u64, frame: u64,
    overlay: bool, primary: bool, selected: Option<egui::Id>, seen: u64,
    blocked: bool, layer: Option<egui::LayerId>,
    selected_in_popup: bool,
}
#[derive(Clone)]
struct MainField {scope:egui::Id,id:egui::Id}
fn key()->egui::Id {egui::Id::new("keyboard-field-owner")}
pub(super) fn request_action(ctx:&egui::Context,action:&'static str) {
    let frame=ctx.cumulative_frame_nr();
    ctx.data_mut(|data|data.insert_temp(egui::Id::new("field-primary-action"),(frame,action)));
}
pub(super) fn take_action(ctx:&egui::Context,action:&str)->bool {
    let frame=ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        let key=egui::Id::new("field-primary-action");
        if data.get_temp::<(u64,&'static str)>(key).is_some_and(|(stored,target)|stored==frame && target==action) {
            data.remove::<(u64,&'static str)>(key);true
        }else {false}
    })
}
pub(super) fn paste_input(input:&egui::InputState)->bool {
    input.focused && (input.events.iter().any(|event|matches!(event,egui::Event::Paste(_)))
        || (input.modifiers.ctrl || input.modifiers.command) && input.key_pressed(egui::Key::V)
        || input.modifiers.shift && input.key_pressed(egui::Key::Insert))
}
pub(super) fn editing_input(input:&egui::InputState)->bool {
    let text=input.events.iter().any(|event|match event {
        egui::Event::Text(text)=>text.chars().any(|ch|!ch.is_control()),
        egui::Event::Ime(_)=>true,_=>false,
    });
    input.focused && (paste_input(input)||text)
}
pub(super) fn begin(app:&NotDiscordUi,ctx:&egui::Context,state:&AppSnapshot) {
    let dialog=app.dialog.as_ref().map(|dialog|match dialog {
        Dialog::Prompt {action,..}=>format!("prompt:{}:{}:{}",action.kind,action.channel_id,action.id),
        Dialog::Settings=>"settings".into(),Dialog::Sounds=>"sounds".into(),Dialog::Media=>"media".into(),
        Dialog::Forward(message)=>format!("forward:{}",message.id),
        _=>"other-dialog".into(),
    });
    let scope=egui::Id::new((state.notification_generation,state.current_user.as_ref().map(|user|user.id.as_str()),
        dialog,app.picker.as_ref().map(|picker|picker.open_id.as_str()),app.search_open,app.viewer.is_some(),
        crate::diagnostics::is_open(ctx),state.view.as_str(),state.selected_channel_id.as_str()));
    let frame=ctx.cumulative_frame_nr();
    let overlay=app.background_blocked();
    let mut owner=ctx.data(|data|data.get_temp::<Owner>(key())).unwrap_or_default();
    let background=egui::Id::new((state.notification_generation,state.current_user.as_ref().map(|user|user.id.as_str()),
        state.view.as_str(),state.selected_channel_id.as_str()));
    let main_key=egui::Id::new("keyboard-return-main-field");
    if owner.scope==Some(scope) && !owner.overlay && !overlay {
        let focused=ctx.memory(|memory|memory.focused());
        if let Some(id)=focused.filter(|id|egui::TextEdit::load_state(ctx,*id).is_some()) {
            ctx.data_mut(|data|data.insert_temp(main_key,MainField {scope:background,id}));
        }
    }
    if owner.scope!=Some(scope) {
        let restore_main=owner.overlay && !overlay;
        owner=Owner {scope:Some(scope),opened:frame,frame,overlay,primary:overlay || state.view!="chat",..Default::default()};
        if overlay {
            let focused=ctx.memory(|memory|memory.focused());
            if let Some(focused)=focused {ctx.memory_mut(|memory|memory.surrender_focus(focused));}
        }
        if restore_main {
            let previous=ctx.data(|data|data.get_temp::<MainField>(main_key)).filter(|field|field.scope==background);
            let restore=ctx.input(|input|input.focused && !input.pointer.any_down());
            let focused=ctx.memory(|memory|memory.focused());
            if let Some(focused)=focused {ctx.memory_mut(|memory|memory.surrender_focus(focused));}
            let target=previous.filter(|_|restore).map(|field|field.id);
            if let Some(id)=target {ctx.memory_mut(|memory|memory.request_focus(id));owner.selected=Some(id);owner.seen=frame;}
            ctx.data_mut(|data|data.insert_temp(egui::Id::new("composer-frame-focus"),(frame,target)));
        }
    }
    owner.frame=frame;
    owner.overlay=overlay;
    owner.blocked=crate::diagnostics::is_open(ctx) || app.viewer.is_some();
    owner.layer=if app.picker.is_some() {
        Some(egui::LayerId::new(egui::Order::Foreground,egui::Id::new("content-picker")))
    }else if app.dialog.is_none() && app.search_open {
        Some(egui::LayerId::new(egui::Order::Foreground,egui::Id::new("search-completion")))
    }else {None};
    let popup=egui::Popup::is_any_open(ctx);
    if !popup && owner.selected_in_popup {
        if let Some(focused)=ctx.memory(|memory|memory.focused()).filter(|focused|Some(*focused)==owner.selected) {ctx.memory_mut(|memory|memory.surrender_focus(focused));}
        owner.selected=None;owner.selected_in_popup=false;owner.primary=overlay || state.view!="chat";
    }
    if owner.blocked || popup && !owner.selected_in_popup {
        if let Some(focused)=ctx.memory(|memory|memory.focused()).filter(|focused|Some(*focused)==owner.selected) {ctx.memory_mut(|memory|memory.surrender_focus(focused));}
        owner.seen=frame;
    }
    let focused=ctx.memory(|memory|memory.focused());
    let restore=ctx.input(|input|editing_input(input) && !input.pointer.any_down() && !input.key_pressed(egui::Key::Tab));
    if !owner.blocked && !popup && focused.is_none() && restore && owner.seen.saturating_add(1)>=frame {
        if let Some(selected)=owner.selected {ctx.memory_mut(|memory|memory.request_focus(selected));}
    }
    ctx.data_mut(|data|data.insert_temp(key(),owner));
}
pub(super) fn before_field(ui:&egui::Ui,id:egui::Id) {
    if !ui.is_enabled() {return;}
    let ctx=ui.ctx();
    let Some(mut owner)=ctx.data(|data|data.get_temp::<Owner>(key())) else {return;};
    if owner.blocked || owner.layer.is_some_and(|layer|layer!=ui.layer_id()) {return;}
    if egui::Popup::is_any_open(ctx) && !ui.stack().iter().any(|entry|matches!(entry.kind(),Some(egui::UiKind::Menu|egui::UiKind::Popup))) {return;}
    if owner.overlay && ui.layer_id().order!=egui::Order::Foreground {return;}
    let focused=ctx.memory(|memory|memory.focused());
    let eligible=ctx.input(|input|input.focused && !input.pointer.any_down() && !input.key_pressed(egui::Key::Tab));
    let selected_input=owner.selected==Some(id) && ctx.input(editing_input);
    if ((owner.primary && owner.selected.is_none()) || selected_input) && focused.is_none() && eligible {
        ctx.memory_mut(|memory|memory.request_focus(id));
        owner.selected=Some(id);owner.primary=false;owner.seen=owner.frame;
        owner.selected_in_popup=ui.stack().iter().any(|entry|matches!(entry.kind(),Some(egui::UiKind::Menu|egui::UiKind::Popup)));
        ctx.data_mut(|data|data.insert_temp(key(),owner));
    }
}
pub(super) fn after_field(ui:&egui::Ui,response:&egui::Response) {
    if !response.has_focus() || !ui.is_enabled() {return;}
    let ctx=ui.ctx();
    let Some(mut owner)=ctx.data(|data|data.get_temp::<Owner>(key())) else {return;};
    if owner.blocked || owner.layer.is_some_and(|layer|layer!=ui.layer_id()) {return;}
    if owner.overlay && ui.layer_id().order!=egui::Order::Foreground {return;}
    owner.selected=Some(response.id);owner.primary=false;owner.seen=owner.frame;
    owner.selected_in_popup=ui.stack().iter().any(|entry|matches!(entry.kind(),Some(egui::UiKind::Menu|egui::UiKind::Popup)));
    ctx.data_mut(|data|data.insert_temp(key(),owner));
}
fn submit_index(ctx:&egui::Context,id:egui::Id)->Option<usize> {
    let frame=ctx.cumulative_frame_nr();
    if ctx.data(|data|data.get_temp::<Owner>(key())).is_some_and(|owner|owner.opened==frame) {return None;}
    if !ctx.memory(|memory|memory.has_focus(id)) || super::chat::composer_ime_active(ctx) {return None;}
    ctx.input(|input| {
        if !input.focused {return None;}
        input.events.iter().position(|event|matches!(event,
            egui::Event::Key {key:egui::Key::Enter,pressed:true,repeat:false,modifiers,..} if *modifiers==egui::Modifiers::NONE))
    })
}
/// A one-line form never submits text typed *after* Enter as if it preceded
/// it. Keep such an input batch in the field and wait for a fresh confirmation.
pub(super) fn take_submit(ctx:&egui::Context,id:egui::Id)->bool {
    let Some(index)=submit_index(ctx,id) else{return false;};
    ctx.input_mut(|input| {
        let tail=input.events[index+1..].iter().any(|event|!matches!(event,
            egui::Event::Key {key:egui::Key::Enter,pressed:false,..}));
        input.events.remove(index);
        !tail
    })
}
/// Message editing can submit a prefix and retain a later plain-text tail as
/// a newer unsent revision. Complex tails stay entirely on TextEdit's path.
pub(super) fn take_edit_submit(ctx:&egui::Context,id:egui::Id,tail_limit:usize)->Option<String> {
    let index=submit_index(ctx,id)?;
    ctx.input_mut(|input| {
        let tail=&input.events[index+1..];
        let simple=tail.iter().all(|event|matches!(event,egui::Event::Text(_)|egui::Event::Paste(_)
            |egui::Event::Key {key:egui::Key::Enter,pressed:false,..}));
        let bytes=tail.iter().fold(0usize,|size,event|size.saturating_add(match event {
            egui::Event::Text(text)|egui::Event::Paste(text)=>text.len(),_=>0,
        }));
        if !simple || bytes>tail_limit {input.events.remove(index);return None;}
        let mut text=String::with_capacity(bytes);
        for event in tail {if let egui::Event::Text(value)|egui::Event::Paste(value)=event{text.push_str(value);}}
        let mut position=0;
        input.events.retain(|event| {
            let keep=position!=index && (position<=index || !matches!(event,egui::Event::Text(_)|egui::Event::Paste(_)));
            position+=1;keep
        });
        Some(text)
    })
}
