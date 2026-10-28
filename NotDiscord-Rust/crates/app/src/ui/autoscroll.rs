//! Middle-button scrolling for egui surfaces (there is no native HWND scroll
//! control behind an egui ScrollArea). One owner survives leaving its bounds.
use eframe::egui::{self, Id, Pos2, Rect};

#[derive(Clone, Default)]
struct Surfaces {
    frame: u64,
    backgrounds: Vec<Id>,
    excluded: Vec<(egui::LayerId, Rect)>,
    primary_rows:Vec<(egui::LayerId,Rect)>,
}
// Resolve viewport IDs and UI metadata before a Context data transaction:
// accessing Context again from that closure would reacquire its same lock.
fn surfaces_key(viewport: egui::ViewportId) -> Id { Id::new(("middle-scroll-surfaces", viewport)) }
fn register_background(ui: &egui::Ui, salt: Id) {
    let ctx=ui.ctx(); let frame=ctx.cumulative_frame_nr();
    let key=surfaces_key(ctx.viewport_id());
    let id=ui.make_persistent_id(salt).with("area");
    ctx.data_mut(|data| {
        let surfaces=data.get_temp_mut_or_default::<Surfaces>(key);
        if surfaces.frame!=frame { *surfaces=Surfaces {frame,..Default::default()}; }
        if !surfaces.backgrounds.contains(&id) { surfaces.backgrounds.push(id); }
    });
}
/// Decorative media can have only Sense::hover and must still exclude middle
/// scrolling. The media caller supplies its exact painted hit rectangle.
pub(super) fn exclude(ui: &egui::Ui, rect: Rect) {
    if !rect.is_positive() {return;}
    let ctx=ui.ctx(); let frame=ctx.cumulative_frame_nr();
    let key=surfaces_key(ctx.viewport_id());
    let excluded=(ui.layer_id(),rect.intersect(ui.clip_rect()));
    ctx.data_mut(|data| {
        let surfaces=data.get_temp_mut_or_default::<Surfaces>(key);
        if surfaces.frame!=frame { *surfaces=Surfaces {frame,..Default::default()}; }
        surfaces.excluded.push(excluded);
    });
}
/// A selectable galley can reserve more layout than the glyphs it paints.
/// Permit its empty region while `exclude` retains each real link rectangle.
pub(super) fn allow_background(ui:&egui::Ui,id:Id) {
    let ctx=ui.ctx();let frame=ctx.cumulative_frame_nr();let key=surfaces_key(ctx.viewport_id());
    ctx.data_mut(|data| {
        let surfaces=data.get_temp_mut_or_default::<Surfaces>(key);
        if surfaces.frame!=frame {*surfaces=Surfaces {frame,..Default::default()};}
        if !surfaces.backgrounds.contains(&id){surfaces.backgrounds.push(id);}
    });
}

/// Navigation/member rows may contain a decorative avatar or primary-only
/// reorder drag. Their complete primary hitbox remains eligible for middle.
pub(super) fn allow_primary_row(ui:&egui::Ui,response:&egui::Response) {
    let ctx=ui.ctx();let frame=ctx.cumulative_frame_nr();let key=surfaces_key(ctx.viewport_id());
    let region=(ui.layer_id(),response.rect.intersect(ui.clip_rect()));
    ctx.data_mut(|data| {
        let surfaces=data.get_temp_mut_or_default::<Surfaces>(key);
        if surfaces.frame!=frame {*surfaces=Surfaces {frame,..Default::default()};}
        surfaces.primary_rows.push(region);
    });
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Mode { #[default] Initial, Holding, Toggled }
#[derive(Clone, Default)]
struct Owner {
    id: Option<Id>, origin: Option<Pos2>, layer: Option<egui::LayerId>,
    seen: u64, processed: Option<u64>, begun: Option<u64>,
    mode: Mode, axes: egui::Vec2b, cancelling: bool, popup_open:bool,
}
fn key(viewport: egui::ViewportId) -> Id { Id::new(("application-middle-scroll", viewport)) }
fn clear(state:&mut Owner) {state.id=None;state.origin=None;state.layer=None;state.axes=egui::Vec2b::FALSE;}
pub(super) fn active(ctx: &egui::Context) -> bool {
    let key=key(ctx.viewport_id());
    ctx.data(|data| data.get_temp::<Owner>(key)).is_some_and(|state|state.id.is_some())
}
pub(super) fn cancelling(ctx:&egui::Context)->bool {
    let key=key(ctx.viewport_id());
    ctx.data(|data|data.get_temp::<Owner>(key)).is_some_and(|state|state.cancelling)
}
pub(super) fn origin(ctx: &egui::Context, id: Id) -> Option<Pos2> {
    let key=key(ctx.viewport_id());
    ctx.data(|data|data.get_temp::<Owner>(key))
        .filter(|state|state.id==Some(id)).and_then(|state|state.origin)
}
pub(super) fn cancel(ctx: &egui::Context) {
    let key=key(ctx.viewport_id());
    ctx.data_mut(|data| {
        if let Some(mut state)=data.get_temp::<Owner>(key) {clear(&mut state);data.insert_temp(key,state);}
    });
}
pub(super) fn begin_frame(ctx: &egui::Context) {
    let frame=ctx.cumulative_frame_nr(); let key=key(ctx.viewport_id());
    let popup=egui::Popup::is_any_open(ctx);
    let (focused,pressed,down,released)=ctx.input(|input|(input.focused,
        input.pointer.primary_pressed()||input.pointer.secondary_pressed(),
        input.pointer.primary_down()||input.pointer.secondary_down(),
        input.pointer.primary_released()||input.pointer.secondary_released()));
    ctx.data_mut(|data| {
        let mut state=data.get_temp::<Owner>(key).unwrap_or_default();
        if state.begun==Some(frame) {return;}
        state.begun=Some(frame);
        if state.cancelling && (!focused || !down && !released) {state.cancelling=false;}
        if state.id.is_some() {
            if pressed && focused {state.cancelling=true;clear(&mut state);state.processed=Some(frame);}
            else if !focused || popup&&!state.popup_open || frame>state.seen.saturating_add(1) {clear(&mut state);}
        }
        data.insert_temp(key,state);
    });
}
pub(super) fn end_frame(ctx: &egui::Context) {
    let frame=ctx.cumulative_frame_nr(); let key=key(ctx.viewport_id());
    if ctx.data(|data|data.get_temp::<Owner>(key))
        .is_some_and(|state|state.id.is_some() && state.seen!=frame) { cancel(ctx); }
}

fn unclaimed(ctx: &egui::Context,point:Pos2) -> bool {
    // Primary click areas are not middle-button actions. Keep whole row/button
    // hitboxes, while editable/drag controls and explicit media regions retain
    // their own gestures. A caller can allow a primary-only draggable row.
    let allowed=ctx.data(|data|data.get_temp::<Vec<Id>>(Id::new("history-middle-allowed"))).unwrap_or_default();
    let key=surfaces_key(ctx.viewport_id());
    let surfaces=ctx.data(|data|data.get_temp::<Surfaces>(key)).unwrap_or_default();
    if surfaces.frame==ctx.cumulative_frame_nr() && surfaces.primary_rows.iter().any(|(layer,rect)|
        rect.contains(point)&&ctx.layer_id_at(point)==Some(*layer)) {return true;}
    if surfaces.frame==ctx.cumulative_frame_nr()
        && surfaces.excluded.iter().any(|(layer,rect)|rect.contains(point)&&ctx.layer_id_at(point)==Some(*layer)) {return false;}
    let hovered=ctx.interaction_snapshot(|snapshot|snapshot.hovered.iter().copied()
        .chain(snapshot.dragged).chain(snapshot.drag_stopped).chain(snapshot.clicked).collect::<Vec<_>>());
    !hovered.into_iter().any(|id|ctx.read_response(id).is_some_and(|response|
        response.rect.contains(point) && response.sense.senses_drag()
            && !allowed.contains(&id) && !surfaces.backgrounds.contains(&id)))
}
fn beyond_dead_zone(origin:Pos2,point:Pos2,axes:egui::Vec2b)->bool {
    axes.x && (point.x-origin.x).abs()>12. || axes.y && (point.y-origin.y).abs()>12.
}
fn speed(distance:f32)->f32 {
    distance.signum()*((distance.abs()-12.).max(0.)/130.).powi(2).min(4.)*1100.
}
/// Called after child areas, so one innermost eligible area claims the ordered
/// native down/move/up sequence. This is independent of egui's click deadline.
pub(crate) fn drive_axes(ui:&egui::Ui,id:Id,viewport:Rect,axes:egui::Vec2b)->egui::Vec2 {
    let ctx=ui.ctx();let frame=ctx.cumulative_frame_nr();let key=key(ctx.viewport_id());
    let popup=egui::Popup::is_any_open(ctx);
    let (pointer,focused,other_down,events)=ctx.input(|input|(input.pointer.interact_pos(),input.focused,
        input.pointer.primary_down()||input.pointer.secondary_down(),input.raw.events.iter().filter(|event|matches!(event,
        egui::Event::PointerMoved(_)|egui::Event::PointerButton {..}|egui::Event::MouseWheel {..}
        |egui::Event::Key {key:egui::Key::Escape,pressed:true,..})).cloned().collect::<Vec<_>>()));
    let mut state=ctx.data(|data|data.get_temp::<Owner>(key)).unwrap_or_default();
    if state.id==Some(id) {
        if !ui.is_enabled() || !focused || !axes.any() || state.layer!=Some(ui.layer_id()) {clear(&mut state);}
        else {state.seen=frame;state.axes=axes;}
    }
    if state.id.is_none_or(|owner|owner==id) && state.processed!=Some(frame) && !state.cancelling {
        let eligible=|point|axes.any() && ui.is_enabled() && focused && !other_down && viewport.contains(point)
            && ctx.layer_id_at(point)==Some(ui.layer_id()) && unclaimed(ctx,point);
        // Do not let a non-owning sibling consume the sequence before its real
        // destination has been drawn. Down position, not final pointer, wins.
        let can_process=state.id==Some(id) || events.iter().any(|event|matches!(event,
            egui::Event::PointerButton {pos,button:egui::PointerButton::Middle,pressed:true,..} if eligible(*pos)));
        if can_process {
            for event in &events {
                match event {
                    egui::Event::PointerButton {pos,button:egui::PointerButton::Middle,pressed:true,..}=> {
                        if state.id.is_some() {clear(&mut state);}
                        else if eligible(*pos) {state.id=Some(id);state.origin=Some(*pos);state.layer=Some(ui.layer_id());
                            state.axes=axes;state.mode=Mode::Initial;state.seen=frame;state.popup_open=popup;}
                    }
                    egui::Event::PointerMoved(point) if state.id==Some(id) && state.mode==Mode::Initial=> {
                        if state.origin.is_some_and(|origin|beyond_dead_zone(origin,*point,state.axes)) {state.mode=Mode::Holding;}
                    }
                    egui::Event::PointerButton {pos,button:egui::PointerButton::Middle,pressed:false,..} if state.id==Some(id)=> {
                        if state.mode==Mode::Initial && state.origin.is_some_and(|origin|beyond_dead_zone(origin,*pos,state.axes)) {state.mode=Mode::Holding;}
                        if state.mode==Mode::Holding {clear(&mut state);}else{state.mode=Mode::Toggled;}
                    }
                    egui::Event::MouseWheel {..}|egui::Event::Key {key:egui::Key::Escape,pressed:true,..}=>clear(&mut state),
                    egui::Event::PointerButton {button:egui::PointerButton::Primary|egui::PointerButton::Secondary,pressed:true,..}=>clear(&mut state),
                    _=>{}
                }
            }
            state.processed=Some(frame);
        }
    }
    let velocity=if state.id==Some(id) {
        state.origin.zip(pointer).map_or(egui::Vec2::ZERO,|(origin,point)| {
            let painter=ui.painter().with_clip_rect(viewport);let color=ui.visuals().text_color();
            painter.circle_filled(origin,10.,ui.visuals().extreme_bg_color);
            painter.circle_stroke(origin,9.,egui::Stroke::new(1_f32,color));
            if state.axes.y {painter.line_segment([origin-egui::vec2(0.,5.),origin+egui::vec2(0.,5.)],egui::Stroke::new(1_f32,color));}
            if state.axes.x {painter.line_segment([origin-egui::vec2(5.,0.),origin+egui::vec2(5.,0.)],egui::Stroke::new(1_f32,color));}
            ctx.set_cursor_icon(if state.axes.x&&state.axes.y {egui::CursorIcon::AllScroll}
                else if state.axes.x {egui::CursorIcon::ResizeHorizontal}else{egui::CursorIcon::ResizeVertical});
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            egui::vec2(if state.axes.x {speed(point.x-origin.x)}else{0.},if state.axes.y {speed(point.y-origin.y)}else{0.})
        })
    }else{egui::Vec2::ZERO};
    ctx.data_mut(|data|data.insert_temp(key,state));velocity
}
pub(crate) fn drive(ui:&egui::Ui,id:Id,viewport:Rect,scrollable:bool)->f32 {
    drive_axes(ui,id,viewport,egui::Vec2b::new(false,scrollable)).y
}

pub(crate) struct ScrollArea { inner: egui::ScrollArea, middle: bool, salt: Id, axes:egui::Vec2b, source:egui::scroll_area::ScrollSource }
impl ScrollArea {
    pub(crate) fn vertical() -> Self { Self {inner:egui::ScrollArea::vertical(),middle:true,salt:Id::new("scroll_area"),axes:egui::Vec2b::new(false,true),source:Default::default()} }
    // Horizontal strips keep their own drag gesture; free space can own middle scrolling.
    pub(super) fn horizontal() -> Self { Self {inner:egui::ScrollArea::horizontal(),middle:true,salt:Id::new("scroll_area"),axes:egui::Vec2b::new(true,false),source:Default::default()} }
    pub(super) fn middle_scroll(mut self, enabled: bool) -> Self {self.middle=enabled;self}
    pub(crate) fn id_salt(mut self,salt:impl std::hash::Hash)->Self {self.salt=Id::new(&salt);self.inner=self.inner.id_salt(salt);self}
    pub(super) fn max_width(mut self,value:f32)->Self {self.inner=self.inner.max_width(value);self}
    pub(crate) fn max_height(mut self,value:f32)->Self {self.inner=self.inner.max_height(value);self}
    pub(super) fn min_scrolled_height(mut self,value:f32)->Self {self.inner=self.inner.min_scrolled_height(value);self}
    pub(super) fn auto_shrink(mut self,value:impl Into<egui::Vec2b>)->Self {self.inner=self.inner.auto_shrink(value);self}
    pub(super) fn stick_to_bottom(mut self,value:bool)->Self {self.inner=self.inner.stick_to_bottom(value);self}
    pub(super) fn vertical_scroll_offset(mut self,value:f32)->Self {self.inner=self.inner.vertical_scroll_offset(value);self}
    pub(super) fn scroll_source(mut self,value:egui::scroll_area::ScrollSource)->Self {self.source=value;self.inner=self.inner.scroll_source(value);self}
    pub(super) fn animated(mut self,value:bool)->Self {self.inner=self.inner.animated(value);self}
    pub(super) fn scroll_bar_visibility(mut self,value:egui::scroll_area::ScrollBarVisibility)->Self {self.inner=self.inner.scroll_bar_visibility(value);self}
    fn prepare(mut self,ui:&egui::Ui)->Self {
        // egui's drag pan accepts any button. Disable only that competing path
        // for a native middle sequence/active owner; primary and touch retain it.
        let middle_input=ui.input(|input|input.pointer.button_down(egui::PointerButton::Middle)
            || input.raw.events.iter().any(|event|matches!(event,egui::Event::PointerButton {button:egui::PointerButton::Middle,..})));
        if self.middle && (active(ui.ctx()) || middle_input || cancelling(ui.ctx())) {
            let fresh_wheel=ui.input(|input|input.raw.events.iter().any(|event|matches!(event,egui::Event::MouseWheel {..})));
            self.inner=self.inner.scroll_source(egui::scroll_area::ScrollSource {drag:false,
                mouse_wheel:self.source.mouse_wheel&&fresh_wheel,..self.source});
        }
        self
    }
    fn finish<R>(middle:bool,axes:egui::Vec2b,ui:&egui::Ui,mut out:egui::scroll_area::ScrollAreaOutput<R>)->egui::scroll_area::ScrollAreaOutput<R> {
        if middle {
            let max=(out.content_size-out.inner_rect.size()).max(egui::Vec2::ZERO);
            let axes=egui::Vec2b::new(axes.x&&max.x>0.,axes.y&&max.y>0.);
            let velocity=drive_axes(ui,out.id,out.inner_rect,axes);
            if velocity!=egui::Vec2::ZERO && ui.ctx().current_pass_index()==0 {
                let dt=ui.ctx().input(|input|input.stable_dt).clamp(0.,0.05);
                out.state.offset=(out.state.offset+velocity*dt).clamp(egui::Vec2::ZERO,max);
                out.state.store(ui.ctx(),out.id);
            }
        }
        out
    }
    pub(crate) fn show<R>(self,ui:&mut egui::Ui,contents:impl FnOnce(&mut egui::Ui)->R)->egui::scroll_area::ScrollAreaOutput<R> {
        let this=self.prepare(ui);register_background(ui,this.salt);
        let out=this.inner.show(ui,|ui| {if cancelling(ui.ctx()){ui.disable();}contents(ui)});
        Self::finish(this.middle,this.axes,ui,out)
    }
    pub(super) fn show_viewport<R>(self,ui:&mut egui::Ui,contents:impl FnOnce(&mut egui::Ui,Rect)->R)->egui::scroll_area::ScrollAreaOutput<R> {
        let this=self.prepare(ui);register_background(ui,this.salt);
        let out=this.inner.show_viewport(ui,|ui,rect| {if cancelling(ui.ctx()){ui.disable();}contents(ui,rect)});
        Self::finish(this.middle,this.axes,ui,out)
    }
    pub(super) fn show_rows<R>(self,ui:&mut egui::Ui,height:f32,count:usize,contents:impl FnOnce(&mut egui::Ui,std::ops::Range<usize>)->R)->egui::scroll_area::ScrollAreaOutput<R> {
        let this=self.prepare(ui);register_background(ui,this.salt);
        let out=this.inner.show_rows(ui,height,count,|ui,rows| {if cancelling(ui.ctx()){ui.disable();}contents(ui,rows)});
        Self::finish(this.middle,this.axes,ui,out)
    }
}
