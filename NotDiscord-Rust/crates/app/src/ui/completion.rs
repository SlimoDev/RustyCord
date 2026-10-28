//! One bounded suggestion presenter; mention and emoji search/insertion stay separate.
use super::super::*;
use std::hash::{Hash,Hasher};

#[derive(Clone)]
pub(in crate::ui) enum Choice {
    Mention(Person),
    GameStart,
    TimeStart,
    Game(MentionGame),
    Notice(String),
    Emoji { emoji: StatusEmoji, label: String, value: String },
}

pub(in crate::ui) fn query(text: &str, cursor: usize) -> Option<(usize, String)> {
    let chars:Vec<_>=text.chars().collect();let end=cursor.min(chars.len());
    if let Some(start)=chars[..end].iter().rposition(|ch|*ch=='@').filter(|start|end-*start<=106) {
        let tail:String=chars[start..end].iter().collect();
        if tail.starts_with("@game ")&&!tail.contains(['\n','\r'])&&(start==0||chars[start-1].is_whitespace())
            &&text.char_indices().nth(start).is_some_and(|(byte,_)|super::plain_text_at(text,byte)){
            return Some((start,format!("@game {}",tail[6..].trim())));
        }
    }
    if let Some((start, query))=super::query(text,cursor) {return Some((start,format!("@{query}")));}
    emoji_query(text,cursor).map(|(start,query)|(start,format!(":{query}")))
}
/// Predict only text preceding Tab in this batch. TextEdit remains the owner of
/// the actual edit; this prevents egui's queued Tab traversal from taking its
/// focus before a brand-new query has reached the editor.
pub(in crate::ui) fn query_before_tab(text:&str,selection:(usize,usize),events:&[egui::Event])->Option<(usize,String)> {
    let mut chars:Vec<_>=text.chars().collect();
    let (mut start,mut end)=(selection.0.min(chars.len()),selection.1.min(chars.len()));
    let mut edited=false;
    for event in events {
        match event {
            egui::Event::Key{key:egui::Key::Tab,pressed:true,..}=>break,
            egui::Event::Text(text)|egui::Event::Paste(text)=>{
                let count=text.chars().count();chars.splice(start..end,text.chars());start+=count;end=start;edited=true;
            }
            _=>{}
        }
    }
    edited.then(||query(&chars.iter().collect::<String>(),start)).flatten()
}
fn emoji_query(text:&str,cursor:usize)->Option<(usize,String)> {
    let chars:Vec<_>=text.chars().collect();let end=cursor.min(chars.len());
    let start=chars[..end].iter().rposition(|&c|c==':')?;
    if end<=start+1 || end-start>65 || start>0 && !chars[start-1].is_whitespace()
        && !matches!(chars[start-1],'('| '[' | '{' | ',' | ';') {return None;}
    let byte=text.char_indices().nth(start)?.0;
    if !super::plain_text_at(text,byte) {return None;}
    let query=&chars[start+1..end];
    if !query.iter().all(|c|c.is_alphanumeric()||matches!(c,'_'|'-'|'+')) {return None;}
    Some((start,query.iter().collect()))
}
pub(in crate::ui) fn choices(query:&str,state:&AppSnapshot,limit:usize)->Vec<Choice> {
    let limit=limit.min(8);
    if let Some(query)=query.strip_prefix('@') {
        if query.starts_with("game "){return vec![Choice::Notice("Escribe el nombre de un juego…".into())];}
        let mut found=Vec::new();
        if "time".starts_with(&query.to_lowercase()){found.push(Choice::TimeStart);}
        if "game".starts_with(&query.to_lowercase()){found.push(Choice::GameStart);}
        found.extend(super::choices(query,state,limit.saturating_sub(found.len())).into_iter().map(Choice::Mention));
        found.truncate(limit);return found;
    }
    query.strip_prefix(':').map(|query|emoji_choices(query,state,limit)).unwrap_or_default()
}
pub(in crate::ui) fn cached_choices(ctx:&egui::Context,query:&str,state:&AppSnapshot,limit:usize)->Vec<Choice> {
    if query.starts_with("@game "){
        let Some(result)=super::super::game_mentions::results(ctx,query,state) else{return vec![Choice::Notice("Escribe el nombre de un juego…".into())];};
        if !result.error.is_empty(){return vec![Choice::Notice(result.error)];}
        if result.loading{return vec![Choice::Notice("Buscando juegos…".into())];}
        if result.items.is_empty(){return vec![Choice::Notice("No se encontraron juegos".into())];}
        return result.items.into_iter().take(limit.min(8)).map(Choice::Game).collect();
    }
    if !query.starts_with(':') {return choices(query,state,limit);}
    let key=egui::Id::new("composer-emoji-results");
    let mut hash=std::collections::hash_map::DefaultHasher::new();
    for emoji in &state.emojis {(&emoji.id,&emoji.name,&emoji.guild_id,emoji.animated,emoji.usable).hash(&mut hash);}
    let signature=hash.finish();let owner=(state.selected_guild_id.clone(),query.to_owned(),signature,limit.min(8));
    type Cached=((String,String,u64,usize),Vec<Choice>);
    if let Some((previous,choices))=ctx.data(|data|data.get_temp::<Cached>(key)) {
        if previous==owner {return choices;}
    }
    let found=choices(query,state,limit);ctx.data_mut(|data|data.insert_temp(key,(owner,found.clone())));found
}
fn normalized(query:&str)->String {query.to_lowercase().replace(['_','-']," ")}
fn emoji_choices(query:&str,state:&AppSnapshot,limit:usize)->Vec<Choice> {
    if query.is_empty()||limit==0 {return Vec::new();}
    let query=normalized(query);let mut result=Vec::with_capacity(limit);
    for emoji in state.emojis.iter().filter(|emoji|emoji.usable && !state.selected_guild_id.is_empty()
        && emoji.guild_id==state.selected_guild_id && notdiscord_core::parsing::valid_id(&emoji.id)) {
        if normalized(&emoji.name).contains(&query) && !result.iter().any(|choice|matches!(choice,Choice::Emoji{emoji:known,..} if known.id==emoji.id)) {
            result.push(Choice::Emoji {emoji:StatusEmoji{id:emoji.id.clone(),name:emoji.name.clone(),animated:emoji.animated},
                label:format!(":{}:",emoji.name),value:format!("<{}:{}:{}>",if emoji.animated {"a"} else {""},emoji.name,emoji.id)});
            if result.len()==limit {return result;}
        }
    }
    // Search the existing bundled names/codes directly. Only the <=8 matches
    // own strings; opening completion never allocates the whole Unicode index.
    for line in EMOJI_CATALOG_TSV.lines().filter(|line|!line.starts_with('#')) {
        let mut fields=line.splitn(4,'\t');
        let (Some(value),Some(_category),Some(name))=(fields.next(),fields.next(),fields.next()) else {continue;};
        let aliases=fields.next().unwrap_or("");
        if value.contains(&query)||normalized(name).contains(&query)||normalized(aliases).contains(&query) {
            result.push(Choice::Emoji {emoji:StatusEmoji{name:value.into(),..Default::default()},
                label:format!(":{}:",name.replace(' ',"_")),value:value.into()});
            if result.len()==limit {break;}
        }
    }
    result
}
pub(in crate::ui) fn insert(draft:&mut Draft,start:usize,end:usize,choice:&Choice)->Option<usize> {
    if start>end||end>draft.text.chars().count() {return None;}
    match choice {
        Choice::TimeStart|Choice::Notice(_)=>None,
        Choice::GameStart|Choice::Game(_)=>{
            let value=match choice{Choice::Game(game)=>format!("<@${}> ",game.id),_=>"@game ".into()};
            let mut chars:Vec<_>=draft.text.chars().collect();chars.splice(start..end,value.chars());
            let old=std::mem::replace(&mut draft.text,chars.iter().collect());super::adjust(&old,&draft.text,&mut draft.mentions);draft.revision+=1;
            Some(start+value.chars().count())
        }
        Choice::Mention(person)=>{super::insert(draft,start,end,person);Some(start+super::person_name(person).chars().count()+2)}
        Choice::Emoji{value,..}=>{
            let mut chars:Vec<_>=draft.text.chars().collect();chars.splice(start..end,value.chars().chain([' ']));
            let old=std::mem::replace(&mut draft.text,chars.iter().collect());
            super::adjust(&old,&draft.text,&mut draft.mentions);draft.revision+=1;
            Some(start+value.chars().count()+1)
        }
    }
}
impl NotDiscordUi {
    pub(in crate::ui) fn draw_completions(&mut self,ctx:&egui::Context,anchor:egui::Rect,choices:&[Choice],selected:usize)->Option<usize> {
        let height=choices.len() as f32*39.+16.;
        let bounds=ctx.content_rect();let width=320_f32.min(bounds.width().max(1.));
        let pos=egui::pos2(anchor.left().clamp(bounds.left(),(bounds.right()-width).max(bounds.left())),
            (anchor.top()-height).max(bounds.top()+4.));
        let mut choose=None;
        egui::Area::new(egui::Id::new("mention-completion")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx,|ui| {
            egui::Frame::popup(ui.style()).show(ui,|ui| {
                ui.set_width((width-16.).max(1.));
                for (index,choice) in choices.iter().enumerate() {
                    let (rect,response)=ui.allocate_exact_size(egui::vec2(ui.available_width(),35.),egui::Sense::click());
                    let label=match choice {Choice::Mention(person)=>match person.id.as_str() {
                        "everyone"=>"@everyone · Todas las personas".into(),"here"=>"@here · Personas disponibles".into(),
                        _=>format!("{}  @{}",super::person_name(person),person.username),
                    },Choice::Emoji{label,..}=>label.clone(),Choice::GameStart=>"@game · Buscar un juego".into(),Choice::TimeStart=>"@time · Fecha y hora".into(),Choice::Game(game)=>game.name.clone(),Choice::Notice(text)=>text.clone()};
                    if index==selected||response.hovered() {ui.painter().rect_filled(rect,4,self.theme.selected);}
                    response.widget_info(||egui::WidgetInfo::labeled(egui::WidgetType::SelectableLabel,true,&label));
                    let mut row=ui.new_child(egui::UiBuilder::new().id_salt(("completion-row",index)).max_rect(rect.shrink2(egui::vec2(5.,2.)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)));
                    row.style_mut().interaction.selectable_labels=false;
                    match choice {
                        Choice::Mention(person) if matches!(person.id.as_str(),"everyone"|"here")=>{row.label(RichText::new("@").color(self.theme.accent));}
                        Choice::Mention(person)=>{let (avatar,_)=row.allocate_exact_size(egui::Vec2::splat(30.),egui::Sense::hover());self.pictures.paint_avatar(&row,avatar,person);}
                        Choice::Emoji{emoji,..}=>{self.emoji(&mut row,emoji,28.);}
                        Choice::GameStart=>{row.label("◈");}
                        Choice::TimeStart=>{row.label("◷");}
                        Choice::Game(game)=>{if let Some(url)=game.icon_url.as_ref()&&let Some(texture)=self.pictures.texture_sized(url,32){row.add(egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(28.)));}}
                        Choice::Notice(_)=>{}
                    }
                    row.add(egui::Label::new(&label).selectable(false).sense(egui::Sense::hover()).truncate());
                    #[cfg(test)] ctx.data_mut(|data|data.insert_temp(egui::Id::new(("composer-completion-row",index)),rect));
                    let selectable=!matches!(choice,Choice::Notice(_));
                    if response.on_hover_cursor(if selectable{egui::CursorIcon::PointingHand}else{egui::CursorIcon::Default}).on_hover_text(label).clicked()&&selectable {choose=Some(index);}
                }
            });
        });choose
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colon_query_respects_code_urls_time_and_unicode_character_cursor() {
        for text in ["https://example.test/:jazz","https:","12:30","C:\\:jazz","<:jazz","`:jazz","\\:jazz",":jazz:","word:jazz"] {
            assert!(query(text,text.chars().count()).is_none(),"{text}");
        }
        assert_eq!(query("日本語 (:smile",11),Some((5,":smile".into())));
        assert_eq!(query("x :jazz tail",7),Some((2,":jazz".into())));
    }
    #[test]
    fn batched_tab_prediction_uses_only_prior_text_and_respects_the_editor_selection() {
        let tab=egui::Event::Key{key:egui::Key::Tab,physical_key:None,pressed:true,repeat:false,modifiers:egui::Modifiers::NONE};
        assert_eq!(query_before_tab("outside tail",(0,7),&[egui::Event::Text(":joy".into()),tab.clone(),egui::Event::Text("ignored".into())]),Some((0,":joy".into())));
        assert!(query_before_tab("",(0,0),&[egui::Event::Text("https://example.test/:joy".into()),tab.clone()]).is_none());
        assert!(query_before_tab("",(0,0),&[tab,egui::Event::Text(":joy".into())]).is_none());
    }
    #[test]
    fn emoji_search_is_bounded_and_insertion_preserves_surrounding_text_and_mentions() {
        let mut state=AppSnapshot::welcome();state.selected_guild_id="50".into();
        state.emojis=vec![CustomEmoji{id:"401".into(),name:"JazzCat".into(),guild_id:"50".into(),available:true,usable:true,..Default::default()},
            CustomEmoji{id:"402".into(),name:"JazzForbidden".into(),guild_id:"50".into(),available:false,usable:false,..Default::default()},
            CustomEmoji{id:"403".into(),name:"JazzOtherGuild".into(),guild_id:"51".into(),available:true,usable:true,..Default::default()}];
        let results=choices(":jazz",&state,100);assert_eq!(results.len(),1);
        let mut draft=Draft{text:"α :jazz tail @Self".into(),mentions:vec![Mention{start:13,end:18,id:"100".into(),label:"@Self".into()}],..Default::default()};
        let cursor=insert(&mut draft,2,7,&results[0]).unwrap();assert_eq!(draft.text,"α <:JazzCat:401>  tail @Self");
        assert_eq!(cursor,"α <:JazzCat:401> ".chars().count());assert_eq!(draft.mentions.len(),1);
        assert_eq!(draft.text.chars().skip(draft.mentions[0].start).take(5).collect::<String>(),"@Self");
        assert!(!choices(":joy",&state,8).is_empty());assert!(choices(":face",&state,100).len()<=8);
    }
}
