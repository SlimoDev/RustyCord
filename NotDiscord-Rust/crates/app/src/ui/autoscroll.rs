//! Middle-button scrolling for egui surfaces (there is no native HWND scroll
//! control behind an egui ScrollArea). One owner survives leaving its bounds.
use eframe::egui::{self, Id, Pos2, Rect};

#[derive(Clone, Default)]
struct Surfaces {
    frame: u64,
    backgrounds: Vec<Id>,
    excluded: Vec<(egui::LayerId, Rect)>,
}
fn surfaces_key(ctx: &egui::Context) -> Id { Id::new(("middle-scroll-surfaces", ctx.viewport_id())) }
fn register_background(ui: &egui::Ui, salt: Id) {
    let ctx=ui.ctx(); let frame=ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        let surfaces=data.get_temp_mut_or_default::<Surfaces>(surfaces_key(ctx));
        if surfaces.frame!=frame { *surfaces=Surfaces {frame,..Default::default()}; }
        let id=ui.make_persistent_id(salt).with("area");
        if !surfaces.backgrounds.contains(&id) { surfaces.backgrounds.push(id); }
    });
}
/// Decorative media can have only Sense::hover and must still exclude middle
/// scrolling. The media caller supplies its exact painted hit rectangle.
pub(super) fn exclude(ui: &egui::Ui, rect: Rect) {
    let ctx=ui.ctx(); let frame=ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        let surfaces=data.get_temp_mut_or_default::<Surfaces>(surfaces_key(ctx));
        if surfaces.frame!=frame { *surfaces=Surfaces {frame,..Default::default()}; }
        surfaces.excluded.push((ui.layer_id(),rect.intersect(ui.clip_rect())));
    });
}

#[derive(Clone, Default)]
struct Owner {
    id: Option<Id>,
    origin: Option<Pos2>,
    seen: u64,
    changed: Option<u64>,
}
fn key(ctx: &egui::Context) -> Id { Id::new(("application-middle-scroll", ctx.viewport_id())) }
pub(super) fn active(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<Owner>(key(ctx))).is_some_and(|state|state.id.is_some())
}
pub(super) fn origin(ctx: &egui::Context, id: Id) -> Option<Pos2> {
    ctx.data(|data|data.get_temp::<Owner>(key(ctx)))
        .filter(|state|state.id==Some(id)).and_then(|state|state.origin)
}
pub(super) fn cancel(ctx: &egui::Context) {
    ctx.data_mut(|data|data.remove::<Owner>(key(ctx)));
}
pub(super) fn begin_frame(ctx: &egui::Context) {
    let frame=ctx.cumulative_frame_nr();
    let stop=ctx.input(|input|input.key_pressed(egui::Key::Escape) || !input.focused
        || input.pointer.primary_pressed() || input.pointer.secondary_pressed());
    ctx.data_mut(|data| {
        if let Some(mut state)=data.get_temp::<Owner>(key(ctx)) {
            if stop || frame>state.seen.saturating_add(1) {
                state.id=None; state.origin=None; state.changed=Some(frame);
                data.insert_temp(key(ctx),state);
            }
        }
    });
}
pub(super) fn end_frame(ctx: &egui::Context) {
    let frame=ctx.cumulative_frame_nr();
    if ctx.data(|data|data.get_temp::<Owner>(key(ctx)))
        .is_some_and(|state|state.id.is_some() && state.seen!=frame) { cancel(ctx); }
}

fn unclaimed(ctx: &egui::Context) -> bool {
    // A plain selectable history label is an intentional exception. Images,
    // emoji atoms, links, sliders, rows with actions and all other controls own
    // their complete hit region, even when no click action ran this frame.
    let allowed=ctx.data(|data|data.get_temp::<Vec<Id>>(Id::new("history-middle-allowed"))).unwrap_or_default();
    let surfaces=ctx.data(|data|data.get_temp::<Surfaces>(surfaces_key(ctx))).unwrap_or_default();
    if surfaces.frame==ctx.cumulative_frame_nr()
        && ctx.input(|input|input.pointer.interact_pos()).is_some_and(|point|
            surfaces.excluded.iter().any(|(layer,rect)|rect.contains(point) && ctx.layer_id_at(point)==Some(*layer))) { return false; }
    let hovered=ctx.interaction_snapshot(|snapshot|snapshot.hovered.iter().copied().collect::<Vec<_>>());
    !hovered.into_iter().any(|id|ctx.read_response(id).is_some_and(|response|
        response.contains_pointer() && (response.sense.senses_click() || response.sense.senses_drag())
            && !allowed.contains(&id) && !surfaces.backgrounds.contains(&id)))
}

pub(super) fn drive(ui: &egui::Ui, id: Id, viewport: Rect, scrollable: bool) -> f32 {
    let ctx=ui.ctx();
    let frame=ctx.cumulative_frame_nr();
    let (pointer, middle)=ctx.input(|input|(input.pointer.interact_pos(),
        input.pointer.button_clicked(egui::PointerButton::Middle)));
    let mut state=ctx.data(|data|data.get_temp::<Owner>(key(ctx))).unwrap_or_default();
    if state.id.is_some() && frame>state.seen.saturating_add(1) {
        state.id=None;state.origin=None;
    }
    if state.id==Some(id) { state.seen=frame; }
    if middle && state.changed!=Some(frame) {
        if state.id.is_some() {
            state.id=None;state.origin=None;state.changed=Some(frame);
        } else if scrollable && ui.is_enabled() && pointer.is_some_and(|point|
            viewport.contains(point) && ctx.layer_id_at(point)==Some(ui.layer_id()))
            && unclaimed(ctx) && !ctx.input(|input|input.pointer.primary_down() || input.pointer.secondary_down()) {
            state.id=Some(id);state.origin=pointer;state.seen=frame;state.changed=Some(frame);
        }
    }
    let velocity=if state.id==Some(id) {
        state.origin.zip(pointer).map_or(0.,|(origin,point)| {
            let painter=ui.painter().with_clip_rect(viewport);
            let color=ui.visuals().text_color();
            painter.circle_filled(origin,10.,ui.visuals().extreme_bg_color);
            painter.circle_stroke(origin,9.,egui::Stroke::new(1_f32,color));
            painter.line_segment([origin-egui::vec2(0.,5.),origin+egui::vec2(0.,5.)],egui::Stroke::new(1_f32,color));
            ctx.set_cursor_icon(egui::CursorIcon::AllScroll);
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
            let distance=point.y-origin.y;
            distance.signum()*((distance.abs()-12.).max(0.)/130.).powi(2).min(4.)*1100.
        })
    } else {0.};
    ctx.data_mut(|data|data.insert_temp(key(ctx),state));
    velocity
}

pub(super) struct ScrollArea { inner: egui::ScrollArea, middle: bool, salt: Id }
impl ScrollArea {
    pub(super) fn vertical() -> Self { Self {inner:egui::ScrollArea::vertical(),middle:true,salt:Id::new("scroll_area")} }
    // Horizontal attachment strips intentionally retain wheel/drag handling.
    pub(super) fn horizontal() -> Self { Self {inner:egui::ScrollArea::horizontal(),middle:false,salt:Id::new("scroll_area")} }
    pub(super) fn middle_scroll(mut self, enabled: bool) -> Self {self.middle=enabled;self}
    pub(super) fn id_salt(mut self,salt:impl std::hash::Hash)->Self {self.salt=Id::new(&salt);self.inner=self.inner.id_salt(salt);self}
    pub(super) fn max_width(mut self,value:f32)->Self {self.inner=self.inner.max_width(value);self}
    pub(super) fn max_height(mut self,value:f32)->Self {self.inner=self.inner.max_height(value);self}
    pub(super) fn min_scrolled_height(mut self,value:f32)->Self {self.inner=self.inner.min_scrolled_height(value);self}
    pub(super) fn auto_shrink(mut self,value:impl Into<egui::Vec2b>)->Self {self.inner=self.inner.auto_shrink(value);self}
    pub(super) fn stick_to_bottom(mut self,value:bool)->Self {self.inner=self.inner.stick_to_bottom(value);self}
    pub(super) fn vertical_scroll_offset(mut self,value:f32)->Self {self.inner=self.inner.vertical_scroll_offset(value);self}
    pub(super) fn scroll_source(mut self,value:egui::scroll_area::ScrollSource)->Self {self.inner=self.inner.scroll_source(value);self}
    pub(super) fn animated(mut self,value:bool)->Self {self.inner=self.inner.animated(value);self}
    pub(super) fn scroll_bar_visibility(mut self,value:egui::scroll_area::ScrollBarVisibility)->Self {self.inner=self.inner.scroll_bar_visibility(value);self}
    fn finish<R>(middle:bool,ui:&egui::Ui,mut out:egui::scroll_area::ScrollAreaOutput<R>)->egui::scroll_area::ScrollAreaOutput<R> {
        if middle {
            let max=(out.content_size.y-out.inner_rect.height()).max(0.);
            let velocity=drive(ui,out.id,out.inner_rect,max>0.);
            let wheel=ui.ctx().input(|input|input.raw_scroll_delta!=egui::Vec2::ZERO || input.smooth_scroll_delta!=egui::Vec2::ZERO);
            if velocity!=0. && ui.ctx().current_pass_index()==0 && !wheel {
                let dt=ui.ctx().input(|input|input.stable_dt).clamp(0.,0.05);
                out.state.offset.y=(out.state.offset.y+velocity*dt).clamp(0.,max);
                out.state.store(ui.ctx(),out.id);
            }
        }
        out
    }
    pub(super) fn show<R>(self,ui:&mut egui::Ui,contents:impl FnOnce(&mut egui::Ui)->R)->egui::scroll_area::ScrollAreaOutput<R> {
        register_background(ui,self.salt);
        let out=self.inner.show(ui,contents);Self::finish(self.middle,ui,out)
    }
    pub(super) fn show_viewport<R>(self,ui:&mut egui::Ui,contents:impl FnOnce(&mut egui::Ui,Rect)->R)->egui::scroll_area::ScrollAreaOutput<R> {
        register_background(ui,self.salt);
        let out=self.inner.show_viewport(ui,contents);Self::finish(self.middle,ui,out)
    }
    pub(super) fn show_rows<R>(self,ui:&mut egui::Ui,height:f32,count:usize,contents:impl FnOnce(&mut egui::Ui,std::ops::Range<usize>)->R)->egui::scroll_area::ScrollAreaOutput<R> {
        register_background(ui,self.salt);
        let out=self.inner.show_rows(ui,height,count,contents);Self::finish(self.middle,ui,out)
    }
}
