//! Account-scoped, bounded presentation and demand queue. Drawing never does I/O.
use super::*;
use serde_json::json;

#[derive(Clone)]
pub(super) struct Results { pub owner:String,pub generation:u64,pub query:String,pub items:Vec<MentionGame>,pub error:String,pub loading:bool }
pub(super) fn results(ctx:&egui::Context,query:&str,state:&AppSnapshot)->Option<Results>{
    ctx.data(|d|d.get_temp::<Results>(egui::Id::new("game-completion-results"))).filter(|r|
        r.owner==state.current_user.as_ref().map(|p|p.id.as_str()).unwrap_or("")&&r.generation==state.notification_generation&&r.query==query)
}
struct Cached{game:MentionGame,at:Instant}
struct Pending{stamp:Value,at:Instant}
struct Search{editor:String,query:String,revision:u64,opening:String,at:Instant,sent:bool,items:Vec<MentionGame>,error:String}
#[derive(Default)]
pub(super) struct State {
    owner:String,generation:u64,cache:VecDeque<Cached>,missing:HashMap<String,Instant>,
    visible_scope:(String,String),
    wanted:Vec<String>,pending:HashMap<String,Pending>,search:Option<Search>,commands:Vec<(String,Value)>,
}
impl State{
    pub(super) fn sync(&mut self,state:&AppSnapshot){
        let owner=state.current_user.as_ref().filter(|_|state.signed_in).map(|p|p.id.as_str()).unwrap_or("");
        if self.owner!=owner||self.generation!=state.notification_generation{*self=Self{owner:owner.into(),generation:state.notification_generation,..Default::default()};}
        let scope=(state.view.clone(),state.selected_channel_id.clone());
        if self.visible_scope!=scope{
            self.visible_scope=scope;self.wanted.clear();self.missing.clear();
            let old:Vec<_>=self.pending.iter().filter(|(_,p)|s(&p.stamp,"editor")=="visible-game-mentions").map(|(id,_)|id.clone()).collect();
            for id in old{if let Some(p)=self.pending.remove(&id){self.commands.push(("game-catalog-cancel".into(),p.stamp));}}
        }
        if self.search.as_ref().is_some_and(|search|state.view!="chat"||search.editor!=state.selected_channel_id){self.cancel_search();}
        self.cache.retain(|entry|entry.at.elapsed()<Duration::from_secs(300));
        // An unavailable/rejected ID is not polled by repainting. A new view
        // intent or explicit message metadata can ask again; keep a bounded set.
        while self.missing.len()>256 {if let Some(id)=self.missing.iter().min_by_key(|(_,at)|**at).map(|(id,_)|id.clone()){self.missing.remove(&id);}else{break;}}
        let expired:Vec<_>=self.pending.iter().filter(|(_,p)|p.at.elapsed()>Duration::from_secs(40)).map(|(id,_)|id.clone()).collect();
        for id in expired{if let Some(pending)=self.pending.remove(&id){
            for id in pending.stamp["ids"].as_array().into_iter().flatten().filter_map(Value::as_str){self.missing.insert(id.into(),Instant::now());}
            if let Some(search)=self.search.as_mut().filter(|search|search.opening==s(&pending.stamp,"opening")){search.error="El catálogo no respondió. Cambia la búsqueda para reintentar.".into();}
        }}
    }
    fn cache(&mut self,game:MentionGame){
        self.cache.retain(|entry|entry.game.id!=game.id);self.missing.remove(&game.id);
        self.cache.push_back(Cached{game,at:Instant::now()});
        while self.cache.len()>256||self.cache.iter().map(|v|v.game.id.len()+v.game.name.len()+v.game.icon_url.as_ref().map_or(0,String::len)+128).sum::<usize>()>1024*1024{self.cache.pop_front();}
    }
    pub(super) fn known(&mut self,id:&str,explicit:&[MentionGame])->Option<MentionGame>{
        if let Some(game)=explicit.iter().find(|g|g.id==id){self.cache(game.clone());return Some(game.clone());}
        self.cache.iter().find(|entry|entry.game.id==id).map(|entry|entry.game.clone())
    }
    fn want(&mut self,id:&str){
        if self.owner.is_empty()||self.missing.contains_key(id)||self.wanted.len()>=20||self.wanted.iter().any(|v|v==id)
            ||self.pending.values().any(|p|p.stamp["ids"].as_array().is_some_and(|ids|ids.iter().any(|v|v.as_str()==Some(id)))){return;}
        self.wanted.push(id.into());
    }
    pub(super) fn receive(&mut self,data:&Value,state:&AppSnapshot){
        self.sync(state);let stamp=&data["stamp"];let request=s(stamp,"request");
        let Some(pending)=self.pending.get(request) else{return;};
        if pending.stamp!=*stamp||s(stamp,"owner")!=self.owner||stamp["generation"].as_u64()!=Some(self.generation){return;}
        self.pending.remove(request);
        let items:Vec<MentionGame>=serde_json::from_value(data["items"].clone()).unwrap_or_default();
        let items:Vec<_>=items.into_iter().take(20).filter(|g|notdiscord_core::parsing::valid_id(&g.id)&&g.name.len()<=512
            &&g.icon_url.as_ref().is_none_or(|url|url.len()<=2048&&notdiscord_core::parsing::trusted_media(url))).collect();
        for game in &items{self.cache(game.clone());}
        for id in stamp["ids"].as_array().into_iter().flatten().filter_map(Value::as_str){if !items.iter().any(|g|g.id==id){self.missing.insert(id.into(),Instant::now());}}
        if let Some(search)=self.search.as_mut().filter(|search|search.opening==s(stamp,"opening")&&search.query==s(stamp,"query")&&Some(search.revision)==stamp["revision"].as_u64()){
            search.items=items;search.error=s(data,"error").chars().take(240).collect();
        }
    }
    fn stamp(&self,editor:&str,opening:&str,revision:u64,query:&str,ids:&[String])->Value{
        json!({"owner":self.owner,"generation":self.generation,"editor":editor,"opening":opening,
            "request":uuid::Uuid::new_v4().to_string(),"revision":revision,"query":query,"ids":ids})
    }
    fn cancel_search(&mut self){if let Some(old)=self.search.take(){
        self.commands.push(("game-catalog-cancel".into(),json!({"owner":self.owner,"generation":self.generation,"editor":old.editor,"opening":old.opening})));
        self.pending.retain(|_,p|s(&p.stamp,"opening")!=old.opening);
    }}
    fn search(&mut self,ctx:&egui::Context,state:&AppSnapshot,editor:&str,revision:u64,query:Option<&str>){
        self.sync(state);let query=query.and_then(|s|s.strip_prefix("@game ")).map(str::trim).filter(|s|!s.is_empty());
        if let Some(query)=query{
            if self.search.as_ref().is_none_or(|s|s.editor!=editor||s.query!=query||s.revision!=revision){
                self.cancel_search();self.search=Some(Search{editor:editor.into(),query:query.into(),revision,opening:uuid::Uuid::new_v4().to_string(),at:Instant::now(),sent:false,items:Vec::new(),error:String::new()});
            }
            let search=self.search.as_ref().unwrap();
            if !search.sent&&search.at.elapsed()>=Duration::from_millis(250)&&self.pending.len()<2{
                let stamp=self.stamp(editor,&search.opening,revision,&search.query,&[]);
                self.pending.insert(s(&stamp,"request").into(),Pending{stamp:stamp.clone(),at:Instant::now()});
                self.commands.push(("game-catalog".into(),stamp));self.search.as_mut().unwrap().sent=true;
            }
            let search=self.search.as_ref().unwrap();let loading=search.error.is_empty()&&(!search.sent||self.pending.values().any(|p|s(&p.stamp,"opening")==search.opening));
            if loading{ctx.request_repaint_after(Duration::from_millis(100));}
            let result=Results{owner:self.owner.clone(),generation:self.generation,query:format!("@game {query}"),items:search.items.iter().filter(|game|!game.restricted||state.game_mentions_nsfw_allowed).cloned().collect(),error:search.error.clone(),loading};
            ctx.data_mut(|data|data.insert_temp(egui::Id::new("game-completion-results"),result));
        }else{self.cancel_search();ctx.data_mut(|data|data.remove::<Results>(egui::Id::new("game-completion-results")));}
    }
    fn flush(&mut self)->Vec<(String,Value)>{
        if !self.wanted.is_empty()&&self.pending.len()<2&&!self.owner.is_empty()
            &&!self.pending.values().any(|pending|s(&pending.stamp,"editor")=="visible-game-mentions"){
            let ids=std::mem::take(&mut self.wanted);let opening=uuid::Uuid::new_v4().to_string();
            let stamp=self.stamp("visible-game-mentions",&opening,0,"",&ids);
            self.pending.insert(s(&stamp,"request").into(),Pending{stamp:stamp.clone(),at:Instant::now()});self.commands.push(("game-catalog".into(),stamp));
        }
        std::mem::take(&mut self.commands)
    }
}
impl NotDiscordUi{
    pub(super) fn update_game_query(&mut self,ctx:&egui::Context,state:&AppSnapshot,editor:&str,revision:u64,query:Option<&str>){self.game_mentions.search(ctx,state,editor,revision,query);}
    pub(super) fn flush_game_queries(&mut self){
        for (kind,stamp) in self.game_mentions.flush(){let query=s(&stamp,"query");self.send(&kind,"",query,&stamp.to_string(),"");}
    }
    pub(super) fn message_game(&mut self,ui:&mut egui::Ui,id:&str,explicit:&[MentionGame],state:&AppSnapshot,size:f32,style:notdiscord_core::markdown::Style)->egui::Response{
        let game=self.game_mentions.known(id,explicit);let visible=game.as_ref().filter(|g|!g.restricted||state.game_mentions_nsfw_allowed);
        if let Some(url)=visible.and_then(|g|g.icon_url.as_ref())&&let Some(texture)=self.pictures.texture_sized(url,32){ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(size)));}
        let name=visible.map(|g|g.name.as_str()).unwrap_or("Juego no disponible");
        let (position,galley,response)=egui::Label::new(theme::markdown_text(name,size,self.theme.ink,style))
            .selectable(true).sense(egui::Sense::click_and_drag()).layout_in_ui(ui);
        ui.painter().rect_filled(response.rect,2.,self.theme.selected);
        if !super::chat::register_history_atom(ui,&response,position,galley.clone(),&format!("<@${id}>")){
            egui::text_selection::LabelSelectionState::label_text_selection(ui,&response,position,galley,self.theme.ink,egui::Stroke::NONE);
        }
        let response=response.on_hover_cursor(egui::CursorIcon::Text).on_hover_text(format!("@game · {name}"));
        if game.is_none()&&ui.is_rect_visible(response.rect){self.game_mentions.want(id);}
        response
    }
}
