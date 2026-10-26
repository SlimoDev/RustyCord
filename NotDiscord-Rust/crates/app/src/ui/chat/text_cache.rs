//! Bounded lexical cache. Tokens contain byte ranges, never resolved names or
//! permissions, so member/role changes are reflected by the shared presenters.
use super::*;
use std::{ops::Range, sync::Arc};
#[derive(Clone)]
pub(super) struct Token { groups: [Option<Range<usize>>;7] }
#[derive(Clone, Copy)]
pub(super) struct Match<'a> { text:&'a str, start:usize, end:usize }
impl<'a> Match<'a> {
    pub fn as_str(self)->&'a str { &self.text[self.start..self.end] }
    pub fn start(self)->usize {self.start}
    pub fn end(self)->usize {self.end}
}
impl Token {
    pub fn get<'a>(&self,index:usize,line:&'a str)->Option<Match<'a>> {
        self.groups[index].as_ref().map(|r|Match{text:line,start:r.start,end:r.end})
    }
}
pub(super) struct Line { pub range:Range<usize>, pub tokens:Vec<Token> }
pub(super) struct Text { pub source:String, pub lines:Vec<Line> }
fn retained_bytes(text:&Text)->usize {
    // Three independently allocated strings survive insertion: source,
    // HashMap key and FIFO key. Empty lines have no tokens but still own a
    // Line/Vec slot; account for capacities rather than just token lengths.
    text.source.len().saturating_mul(3)
        .saturating_add(std::mem::size_of::<Text>()+2*std::mem::size_of::<String>())
        .saturating_add(text.lines.capacity().saturating_mul(std::mem::size_of::<Line>()))
        .saturating_add(text.lines.iter().map(|line|line.tokens.capacity().saturating_mul(std::mem::size_of::<Token>())).sum::<usize>())
}
#[derive(Clone,Default)]
struct Cache { entries:HashMap<String,Arc<Text>>, order:VecDeque<String>, bytes:usize }
pub(super) fn parsed(ctx:&egui::Context,text:&str)->Arc<Text> {
    ctx.data_mut(|data| {
        let cache=data.get_temp_mut_or_default::<Cache>(egui::Id::new("message-lexical-cache"));
        if let Some(found)=cache.entries.get(text) { return found.clone(); }
        let mut start=0;
        let lines=text.split('\n').map(|line| {
            let tokens=message_rich_pattern().captures_iter(line).filter_map(|capture| {
                let whole=capture.get(0)?;
                if (capture.get(1).is_some() || capture.get(6).is_some())
                    && !super::super::mentions::plain_text_at(text,start+whole.start()) {return None;}
                if capture.get(6).is_some() && !super::super::mentions::broadcast_at(text,start+whole.start(),start+whole.end()) {return None;}
                Some(Token{groups:std::array::from_fn(|i|capture.get(i).map(|m|m.range()))})
            }).collect();
            let range=start..start+line.len();start+=line.len()+1;Line{range,tokens}
        }).collect();
        let parsed=Arc::new(Text{source:text.into(),lines});
        let cost=retained_bytes(&parsed);
        if cost<=256*1024 {
            while cache.entries.len()>=256 || cache.bytes+cost>2*1024*1024 {
                let Some(key)=cache.order.pop_front() else {break;};
                if let Some(old)=cache.entries.remove(&key) {
                    cache.bytes=cache.bytes.saturating_sub(retained_bytes(&old));
                }
            }
            cache.bytes+=cost;cache.order.push_back(text.into());cache.entries.insert(text.into(),parsed.clone());
        }
        parsed
    })
}

#[derive(Clone,Default)]
struct Names { content:String, catalogue:SharedList<CustomEmoji>, guild:String, values:HashMap<String,Option<StatusEmoji>> }
pub(super) fn named(ctx:&egui::Context,name:&str,message:&ChatMessage,state:&AppSnapshot)->Option<StatusEmoji> {
    if message.content.len()>16*1024 {return known_named_emoji(name,message,state);}
    ctx.data_mut(|data| {
        let key=egui::Id::new("message-named-emoji-cache");
        let entries=data.get_temp_mut_or_default::<HashMap<(String,String),Names>>(key);
        if entries.len()>64 {entries.clear();}
        let entry=entries.entry((message.channel_id.clone(),message.id.clone())).or_default();
        if entry.content!=message.content || !entry.catalogue.same_storage(&state.emojis) || entry.guild!=state.selected_guild_id {
            *entry=Names{content:message.content.clone(),catalogue:state.emojis.clone(),guild:state.selected_guild_id.clone(),values:HashMap::new()};
        }
        entry.values.entry(name.into()).or_insert_with(||known_named_emoji(name,message,state)).clone()
    })
}

#[derive(Clone,Default)]
struct Replies { source:SharedList<ChatMessage>, authors:HashMap<(String,String),String> }
pub(super) fn personal_reply(ctx:&egui::Context,message:&ChatMessage,state:&AppSnapshot)->bool {
    let Some(current)=state.current_user.as_ref().filter(|user|!user.id.is_empty()) else{return false;};
    let Some(reference)=message.reply_to.as_deref().filter(|id|notdiscord_core::parsing::valid_id(id)&&*id!=message.id) else{return false;};
    if message.forwarded || message.deleted || message.channel_id.is_empty() {return false;}
    let author=ctx.data_mut(|data| {
        let cache=data.get_temp_mut_or_default::<Replies>(egui::Id::new("message-reply-index"));
        if !cache.source.same_storage(&state.messages) {
            cache.authors=state.messages.iter().filter(|m|!m.author.id.is_empty())
                .map(|m|((m.channel_id.clone(),m.id.clone()),m.author.id.clone())).collect();
            cache.source=state.messages.clone();
        }
        cache.authors.get(&(message.channel_id.clone(),reference.into())).cloned()
    });
    if let Some(author)=author {return author==current.id;}
    message.reply_preview.as_ref().is_some_and(|preview|preview.message_id==reference
        && preview.channel_id==message.channel_id && !preview.unavailable
        && preview.author.as_ref().is_some_and(|author|author.id==current.id))
}
pub(super) fn clear(ctx:&egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<Cache>(egui::Id::new("message-lexical-cache"));
        data.remove::<HashMap<(String,String),Names>>(egui::Id::new("message-named-emoji-cache"));
        data.remove::<Replies>(egui::Id::new("message-reply-index"));
    });
}
