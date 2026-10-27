//! Local guild order. IDs remain stable across virtualized rows/snapshots;
//! only an explicit completed drag writes a candidate to shared navigation.
use super::*;
use sha2::{Digest,Sha256};

#[derive(Clone,Default)]
pub(super) struct State {
    account:String, order:Vec<String>, drag:Option<Drag>,
}
#[derive(Clone)]
struct Drag { id:String, members:Vec<String>, target:usize }
fn key()->egui::Id {egui::Id::new("guild-order-state")}
fn account(state:&AppSnapshot)->String {
    state.current_user.as_ref().map_or(String::new(),|person|format!("{:x}",Sha256::digest(person.id.as_bytes())))
}
impl State {
    pub(super) fn load(ctx:&egui::Context,path:&std::path::Path,snapshot:&AppSnapshot)->Self {
        let account=account(snapshot);
        let mut state=ctx.data(|data|data.get_temp::<State>(key())).filter(|state|state.account==account)
            .unwrap_or_else(|| {
                let raw:Value=crate::storage::read_json(&path.join("navigation.json"),4*1024*1024);
                let order=raw["GuildOrderByAccount"][&account].as_array().map(|items|items.iter().take(4096)
                    .filter_map(Value::as_str).filter(|id|notdiscord_core::parsing::valid_id(id)).map(str::to_owned).collect()).unwrap_or_default();
                Self {account,order,drag:None}
            });
        let present=snapshot.guilds.iter().map(|item|item.id.as_str()).collect::<HashSet<_>>();
        let mut known=HashSet::new();
        state.order.retain(|id|present.contains(id.as_str())&&known.insert(id.clone()));
        for item in snapshot.guilds.iter().take(4096) {if known.insert(item.id.clone()){state.order.push(item.id.clone());}}
        let cancel=ctx.input(|input|!input.focused || input.key_pressed(egui::Key::Escape) || input.pointer.secondary_pressed());
        if cancel || state.drag.as_ref().is_some_and(|drag|drag.members!=state.order) || egui::Popup::is_any_open(ctx) {state.drag=None;}
        state
    }
    pub(super) fn indices(&self,items:&[NavItem])->Vec<usize> {
        let positions=items.iter().enumerate().map(|(i,item)|(item.id.as_str(),i)).collect::<HashMap<_,_>>();
        self.order.iter().filter_map(|id|positions.get(id.as_str()).copied()).collect()
    }
    pub(super) fn row(&mut self,response:&egui::Response,id:&str) {
        if response.drag_started_by(egui::PointerButton::Primary) && response.enabled() && !self.account.is_empty() {
            self.drag=Some(Drag {id:id.into(),members:self.order.clone(),target:0});
        }
    }
    pub(super) fn dragging(&self)->bool {self.drag.is_some()}
    pub(super) fn finish(&mut self,ui:&egui::Ui,out:&mut egui::scroll_area::ScrollAreaOutput<()>,path:&std::path::Path)->Result<(),String> {
        let ctx=ui.ctx();
        if !ui.is_enabled() || egui::Popup::is_any_open(ctx) {self.drag=None;return Ok(());}
        let pitch=56.+ui.spacing().item_spacing.y;
        let pointer=ctx.input(|input|input.pointer.interact_pos());
        let released=ctx.input(|input|input.pointer.primary_released());
        if let Some(drag)=&mut self.drag {
            if let Some(pointer)=pointer.filter(|point|out.inner_rect.contains(*point)) {
                let y=(pointer.y-out.inner_rect.top()+out.state.offset.y).max(0.);
                drag.target=((y/pitch).floor() as usize+usize::from(y%pitch>pitch/2.)).min(self.order.len());
                let line_y=out.inner_rect.top()+drag.target as f32*pitch-out.state.offset.y;
                ui.painter().with_clip_rect(out.inner_rect).hline(out.inner_rect.x_range(),line_y,
                    egui::Stroke::new(3.,ui.visuals().hyperlink_color));
                if !released {
                    let edge=28_f32.min(out.inner_rect.height()/3.);
                    let velocity=if pointer.y<out.inner_rect.top()+edge {-420.}else if pointer.y>out.inner_rect.bottom()-edge {420.}else{0.};
                    if velocity!=0. {
                        let max=(out.content_size.y-out.inner_rect.height()).max(0.);
                        let dt=ctx.input(|input|input.stable_dt).clamp(0.,0.05);
                        out.state.offset.y=(out.state.offset.y+velocity*dt).clamp(0.,max);
                        out.state.store(ctx,out.id);
                    }
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                    ctx.request_repaint_after(Duration::from_millis(16));
                }
            } else if released {self.drag=None;}
        }
        if released && let Some(drag)=self.drag.take() {
            if drag.members==self.order && let Some(from)=self.order.iter().position(|id|id==&drag.id) {
                let mut candidate=self.order.clone();let id=candidate.remove(from);
                let target=drag.target.saturating_sub(usize::from(from<drag.target)).min(candidate.len());
                candidate.insert(target,id);
                if candidate!=self.order {
                    let file=path.join("navigation.json");
                    let mut document:std::collections::BTreeMap<String,Value>=crate::storage::read_json_checked(&file,4*1024*1024)?;
                    let accounts=document.entry("GuildOrderByAccount".into()).or_insert_with(||serde_json::json!({}));
                    if !accounts.is_object(){return Err("El orden de servidores no tiene un formato compatible; se conservó la configuración anterior.".into());}
                    accounts[&self.account]=serde_json::json!(candidate);
                    // The last confirmed in-memory order survives a write error.
                    crate::storage::save_json(&file,&document)?;
                    self.order=candidate;
                }
            }
        }
        Ok(())
    }
    pub(super) fn store(self,ctx:&egui::Context){ctx.data_mut(|data|data.insert_temp(key(),self));}
}
