//! Geometry is cached independently of painted widgets. A selection keeps the
//! complete layout: its existing text/emoji endpoints must remain available.
use super::*;

#[derive(Clone, Default)]
pub(super) struct Rows {
    scope: Option<egui::Id>,
    layout: Option<(f32, f32, bool, u32)>,
    messages: SharedList<ChatMessage>,
    heights: Vec<Option<f32>>,
}
impl Rows {
    pub fn sync(&mut self, scope: egui::Id, layout: (f32,f32,bool,u32), messages: &SharedList<ChatMessage>) {
        let unchanged = self.scope == Some(scope) && self.layout == Some(layout);
        if unchanged && self.messages.same_storage(messages) { return; }
        let old: HashMap<_,_> = if unchanged {
            self.messages.iter().enumerate().map(|(i,m)|(m.id.as_str(),i)).collect()
        } else { HashMap::new() };
        let heights = messages.iter().enumerate().map(|(i,m)| {
            let &j = old.get(m.id.as_str())?;
            // A date divider/group header also depends on the preceding row.
            (self.messages[j] == *m && i.checked_sub(1).map(|n|&messages[n])
                == j.checked_sub(1).map(|n|&self.messages[n])).then(||self.heights[j]).flatten()
        }).collect();
        self.messages = messages.clone();
        self.heights = heights; self.scope = Some(scope); self.layout = Some(layout);
    }
    pub fn height(&self, index:usize, message:&ChatMessage, continuation:bool, date:bool) -> f32 {
        self.heights[index].unwrap_or_else(|| {
            let text = message.content.lines().count().max(1).min(16) as f32;
            let media = !message.attachments.is_empty() || !message.embeds.is_empty()
                || !message.components.is_empty() || !message.stickers.is_empty();
            (if continuation { 10. } else { 38. }) + text * 25.
                + if date { 46. } else { 0. } + if media { 210. } else { 0. }
        })
    }
    pub fn measure(&mut self,index:usize,height:f32) { self.heights[index] = Some(height.max(1.)); }
}
pub(super) fn complete_layout(ctx:&egui::Context,scope:egui::Id)->bool {
    egui::Popup::is_any_open(ctx) || reaction_panel_open(ctx)
        || ctx.input(|input|input.pointer.primary_down() || input.pointer.primary_pressed())
        || ctx.data(|data|data.get_temp::<HistorySelection>(egui::Id::new("history-selection")))
            .is_some_and(|selection|selection.scope==Some(scope) && selection.anchor.is_some())
}
