//! Read-only, bounded game names/icons for visible mentions and composition.
use super::*;
use notdiscord_core::game_mentions as core_games;
use tokio_util::sync::CancellationToken;

const TTL:Duration=Duration::from_secs(300);
const CACHE_BYTES:usize=1024*1024;
#[derive(Clone)]
enum Lookup { Query(String), Ids(Vec<String>) }
#[derive(Clone)]
struct Request { stamp:Value, lookup:Lookup }
struct QueryCache { key:String, at:Instant, items:Vec<MentionGame>, bytes:usize }
struct IdCache { id:String, at:Instant, game:Option<MentionGame>, bytes:usize }
struct Job { id:String, lookup:Lookup, watchers:Vec<Request>, cancel:CancellationToken }
#[derive(Default)]
pub(super) struct State {
    queries:VecDeque<QueryCache>, ids:VecDeque<IdCache>, jobs:HashMap<String,Job>, queued:VecDeque<Request>,
}
impl State {
    pub(super) fn clear(&mut self) {
        for job in self.jobs.values() {job.cancel.cancel();}
        *self=Self::default();
    }
    fn trim(&mut self) {
        self.queries.retain(|entry|entry.at.elapsed()<TTL);
        self.ids.retain(|entry|entry.at.elapsed()<TTL);
        while self.queries.len()>64 {self.queries.pop_front();}
        while self.ids.len()>256 {self.ids.pop_front();}
        while self.queries.iter().map(|entry|entry.bytes).sum::<usize>()+self.ids.iter().map(|entry|entry.bytes).sum::<usize>()>CACHE_BYTES {
            if self.queries.front().map(|entry|entry.at)<=self.ids.front().map(|entry|entry.at) && !self.queries.is_empty() || self.ids.is_empty() {
                if self.queries.pop_front().is_none() {break;}
            } else {self.ids.pop_front();}
        }
    }
    fn id(&mut self,id:&str)->Option<Option<MentionGame>> {
        let index=self.ids.iter().position(|entry|entry.id==id)?;
        let entry=self.ids.remove(index)?;let game=entry.game.clone();self.ids.push_back(entry);Some(game)
    }
    fn cached(&mut self, lookup:&Lookup)->Option<Vec<MentionGame>> {
        self.trim();
        match lookup {
            Lookup::Query(query)=>{
                let index=self.queries.iter().position(|entry|entry.key==*query)?;
                let entry=self.queries.remove(index)?;let items=entry.items.clone();self.queries.push_back(entry);Some(items)
            }
            Lookup::Ids(ids)=>{
                let mut items=Vec::new();for id in ids {if let Some(game)=self.id(id)? {items.push(game);}}Some(items)
            }
        }
    }
    fn insert(&mut self,lookup:&Lookup,items:Vec<MentionGame>) {
        let now=Instant::now();
        let ids=match lookup {Lookup::Ids(ids)=>ids.clone(),Lookup::Query(_)=>items.iter().map(|game|game.id.clone()).collect()};
        for id in ids {
            let game=items.iter().find(|game|game.id==id).cloned();
            let bytes=id.len()+game.as_ref().map_or(0,|game|serde_json::to_vec(game).map_or(CACHE_BYTES+1,|v|v.len()));
            if bytes>CACHE_BYTES {continue;}
            self.ids.retain(|entry|entry.id!=id);self.ids.push_back(IdCache{id,at:now,game,bytes});
        }
        if let Lookup::Query(query)=lookup {
            let bytes=query.len()+serde_json::to_vec(&items).map_or(CACHE_BYTES+1,|v|v.len());
            if bytes<=CACHE_BYTES {
                self.queries.retain(|entry|entry.key!=*query);
                self.queries.push_back(QueryCache{key:query.clone(),at:now,items,bytes});
            }
        }
        self.trim();
    }
    fn retire(&mut self,editor:&str,opening:&str) {
        let same=|request:&Request|s(&request.stamp,"editor")==editor&&s(&request.stamp,"opening")==opening;
        self.queued.retain(|request|!same(request));
        for job in self.jobs.values_mut() {job.watchers.retain(|request|!same(request));}
    }
    fn retire_editor(&mut self,editor:&str) {
        // A new opening supersedes the old one for this editor. A delayed
        // explicit cancel still matches its exact opening via retire above.
        self.queued.retain(|request|s(&request.stamp,"editor")!=editor);
        for job in self.jobs.values_mut() {job.watchers.retain(|request|s(&request.stamp,"editor")!=editor);}
    }
    fn cancel_unobserved(&self) {
        for job in self.jobs.values().filter(|job|job.watchers.is_empty()) {job.cancel.cancel();}
    }
}
impl Controller {
    fn game_scope(&self,stamp:&Value)->bool {
        s(stamp,"owner")==self.me() && stamp["generation"].as_u64()==Some(self.state.notification_generation)
            && self.state.signed_in
    }
    fn game_reply(&self,request:&Request,items:Vec<MentionGame>,error:&str) {
        if !self.game_scope(&request.stamp) {return;}
        let items=items.into_iter().filter(|game|!game.restricted||self.state.game_mentions_nsfw_allowed).take(20).collect::<Vec<_>>();
        self.data("game-catalog",json!({"stamp":request.stamp,"items":items,"error":error}));
    }
    pub(super) fn game_catalog_action(&mut self,action:UiAction)->Result<(),String> {
        if action.extra.len()>8192 {return Err("Consulta de juegos demasiado grande.".into());}
        let stamp:Value=serde_json::from_str(&action.extra).map_err(|_|"Consulta de juegos sin correlación.")?;
        if !self.game_scope(&stamp) {return Ok(());}
        let editor=s(&stamp,"editor");let opening=s(&stamp,"opening");
        if editor.is_empty()||editor.len()>160||editor.chars().any(char::is_control)
            || uuid::Uuid::parse_str(&opening).is_err()||uuid::Uuid::parse_str(&s(&stamp,"request")).is_err()
            ||stamp["revision"].as_u64().is_none() {return Err("Consulta de juegos inválida.".into());}
        let query=s(&stamp,"query");
        let lookup=if stamp["ids"].as_array().is_some_and(|ids|!ids.is_empty()) {
            if !query.trim().is_empty() {return Err("Elige búsqueda o resolución de juegos.".into());}
            let values=stamp["ids"].as_array().unwrap();
            if values.len()>20||values.iter().any(|id|id.as_str().is_none_or(|id|!parsing::valid_id(id))) {return Err("Identificadores de juego inválidos.".into());}
            let mut ids=values.iter().map(|id|id.as_str().unwrap().to_owned()).collect::<Vec<_>>();ids.sort();ids.dedup();
            Lookup::Ids(ids)
        } else {
            let query=query.trim().to_lowercase().replace('_'," ");
            core_games::search_route(&query).ok_or("Búsqueda de juego inválida.")?;
            Lookup::Query(query)
        };
        let request=Request{stamp,lookup};
        if matches!(&request.lookup,Lookup::Query(_)) {self.game_catalog.retire_editor(&editor);}
        else {self.game_catalog.retire(&editor,&opening);}
        if let Some(items)=self.game_catalog.cached(&request.lookup) {
            self.game_reply(&request,items,"");self.game_catalog.cancel_unobserved();return Ok(());
        }
        if self.game_catalog.queued.len()>=8 && let Some(old)=self.game_catalog.queued.pop_front() {
            self.game_reply(&old,Vec::new(),"Hay demasiadas consultas de juegos pendientes. Puedes reintentar.");
        }
        self.game_catalog.queued.push_back(request);
        self.pump_game_catalog()
    }
    pub(super) fn cancel_game_catalog(&mut self,extra:&str) {
        if extra.len()>8192 {return;}
        let stamp:Value=serde_json::from_str(extra).unwrap_or_default();
        if !self.game_scope(&stamp) {return;}
        self.game_catalog.retire(&s(&stamp,"editor"),&s(&stamp,"opening"));self.game_catalog.cancel_unobserved();
    }
    fn pump_game_catalog(&mut self)->Result<(),String> {
        while let Some(request)=self.game_catalog.queued.pop_front() {
            if !self.game_scope(&request.stamp) {continue;}
            if let Some(items)=self.game_catalog.cached(&request.lookup) {self.game_reply(&request,items,"");continue;}
            let lookup=match &request.lookup {
                Lookup::Query(query)=>Lookup::Query(query.clone()),
                Lookup::Ids(ids)=>Lookup::Ids(ids.iter().filter(|id|self.game_catalog.id(id).is_none()).cloned().collect()),
            };
            let endpoint=match &lookup {Lookup::Query(query)=>core_games::search_route(query),Lookup::Ids(ids)=>core_games::resolve_route(ids)}
                .ok_or("Consulta de juegos inválida.")?;
            if let Some(job)=self.game_catalog.jobs.get_mut(&endpoint).filter(|job|!job.cancel.is_cancelled()) {
                if job.watchers.len()<8 {job.watchers.push(request);}else{self.game_reply(&request,Vec::new(),"Demasiadas consultas simultáneas de juegos. Puedes reintentar.");}
                continue;
            }
            if self.game_catalog.jobs.len()>=2 || self.game_catalog.jobs.contains_key(&endpoint) {
                self.game_catalog.queued.push_front(request);break;
            }
            let api=match self.api(){Ok(api)=>api,Err(_)=>{self.game_reply(&request,Vec::new(),"La conexión no está disponible para consultar juegos.");continue;}};
            let cancel=CancellationToken::new();let id=uuid::Uuid::new_v4().to_string();
            self.game_catalog.jobs.insert(endpoint.clone(),Job{id:id.clone(),lookup,watchers:vec![request],cancel:cancel.clone()});
            let mut action=UiAction::id("__game-catalog",&endpoint);action.extra=id;
            self.launch(action,None,async move {
                Ok(tokio::select! {
                    _=cancel.cancelled()=>json!({"cancelled":true}),
                    result=tokio::time::timeout(Duration::from_secs(10),api.get(&endpoint))=>match result {
                        Ok(Ok(value))=>json!({"value":value}),
                        Ok(Err(error))=>json!({"error":match error.status {Some(401|403)=>"Discord no autorizó la consulta de juegos.",Some(404)=>"El catálogo de juegos no está disponible.",Some(429)=>"Discord pidió esperar antes de consultar juegos.",_=>"No se pudo consultar el catálogo de juegos. Puedes reintentar."}}),
                        Err(_)=>json!({"error":"La consulta de juegos tardó demasiado. Puedes reintentar."}),
                    }
                })
            });
        }
        self.game_catalog.cancel_unobserved();Ok(())
    }
    pub(super) fn complete_game_catalog(&mut self,action:&UiAction,value:&Value)->Result<(),String> {
        if self.game_catalog.jobs.get(&action.id).is_none_or(|job|job.id!=action.extra) {return Ok(());}
        let job=self.game_catalog.jobs.remove(&action.id).unwrap();
        let error=if value["cancelled"]==true {"Consulta de juegos cancelada."}
            else if let Some(error)=value["error"].as_str(){error}
            else if !value["value"].is_array(){"Discord no confirmó una lista de juegos válida."}else{""};
        if error.is_empty() {
            let mut items=core_games::parse(&value["value"]);
            if let Lookup::Ids(ids)=&job.lookup {items.retain(|game|ids.contains(&game.id));}
            self.game_catalog.insert(&job.lookup,items.clone());
            for request in job.watchers {
                let result=self.game_catalog.cached(&request.lookup).unwrap_or_else(||items.clone());self.game_reply(&request,result,"");
            }
        } else {for request in job.watchers {self.game_reply(&request,Vec::new(),error);}}
        self.pump_game_catalog()
    }
    pub(super) fn game_catalog_failed(&mut self,action:&UiAction) {
        if action.kind!="__game-catalog" {return;}
        let _=self.complete_game_catalog(action,&json!({"error":"No se pudo completar la consulta de juegos. Puedes reintentar."}));
    }
}
