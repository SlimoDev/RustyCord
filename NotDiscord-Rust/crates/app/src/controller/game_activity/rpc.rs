//! Converts the same-user IPC server's bounded input into the shared Gateway model.
use super::*;
use crate::game_rpc;
use notdiscord_core::game_presence::{GamePresence,GamePresenceTimestamps};

const RPCS3_ID:&str="424004941485572097";
fn public_name(executable:&str)->Result<String,String> {
    if executable.is_empty()||executable.len()>260||executable.contains(['\\','/',':'])||executable.chars().any(char::is_control){return Err("La identidad local de la aplicación no es válida.".into());}
    if let Some(program)=detection::identify(executable){return Ok(match program{detection::Program::Game(name,_)=>name.into(),detection::Program::Rpcs3=>"RPCS3".into()});}
    let name=executable.strip_suffix(".exe").or_else(||executable.strip_suffix(".EXE")).unwrap_or(executable).trim();
    if name.is_empty()||name.chars().count()>80||name.chars().any(|c|!c.is_alphanumeric()&&!matches!(c,' '|'.'|'_'|'-'|'('|')')){
        return Err("No se pudo confirmar un nombre público para la aplicación local.".into());
    }
    Ok(name.to_owned())
}
fn contains_private_data(text:&str)->bool {
    let bytes=text.as_bytes();
    text.contains("\\\\")||text.to_ascii_lowercase().contains("file://")||bytes.windows(3).any(|part|part[0].is_ascii_alphabetic()&&part[1]==b':'&&matches!(part[2],b'\\'|b'/'))
        ||["/home/","/Users/","/private/","/tmp/"].iter().any(|path|text.contains(path))
}
fn convert(activity:&game_rpc::Activity)->Result<GamePresence,String> {
    let name=public_name(&activity.executable)?;
    if [&activity.details,&activity.state].iter().any(|text|contains_private_data(text))
        ||activity.assets.as_ref().is_some_and(|assets|[&assets.large_text,&assets.small_text].into_iter().flatten().any(|text|contains_private_data(text))) {
        return Err("La actividad local contiene una ruta privada y no se publicará.".into());
    }
    let mut payload=json!({"type":0,"details":activity.details,"state":activity.state});
    if let Some(assets)=&activity.assets {
        let mut object=serde_json::Map::new();
        for(key,value)in[("large_image",&assets.large_image),("large_text",&assets.large_text),("small_image",&assets.small_image),("small_text",&assets.small_text)]{
            if let Some(value)=value{object.insert(key.into(),json!(value));}
        }
        payload["assets"]=Value::Object(object);
    }
    // Never honor RPC's freely supplied name, application_id, URLs, buttons, party or secrets.
    let mut result=GamePresence::from_rpc(&activity.client_id,&name,&payload)?;
    if let Some(times)=&activity.timestamps {
        let convert=|value:i64|u64::try_from(value).map_err(|_|"La marca temporal local no es válida.");
        let start=times.start_ms.map(convert).transpose()?;
        let end=times.end_ms.map(convert).transpose()?;
        if start.is_some()||end.is_some(){result.timestamps_ms=Some(GamePresenceTimestamps{start,end});}
    }
    result.validate()?;Ok(result)
}
pub(super) fn idle_emulator(activity:&GamePresence)->bool {
    activity.name=="RPCS3"&&activity.application_id.as_deref()==Some(RPCS3_ID)
        &&activity.details.as_deref()==Some("Idle")&&activity.state.is_none()
}

pub(super) struct Receiver {
    listener:Option<game_rpc::Listener>,
    starting:Option<tokio::task::JoinHandle<Result<game_rpc::Listener,String>>>,
    changes:Option<tokio::sync::watch::Receiver<game_rpc::Snapshot>>,
    owner:Option<(String,u64)>,
    stopping:bool,
    retry_at:Instant,
    revision:Option<u64>,
    pub activity:Option<GamePresence>,
    pub error:Option<String>,
}
impl Receiver {
    pub fn new()->Self {Self{listener:None,starting:None,changes:None,owner:None,stopping:false,retry_at:Instant::now(),revision:None,activity:None,error:None}}
    pub fn close(&mut self){
        if let Some(task)=self.starting.take(){task.abort();}
        if let Some(listener)=&self.listener{listener.close();self.stopping=true;}
        self.changes=None;self.activity=None;self.revision=None;
    }
    pub fn reset(&mut self){self.close();self.owner=None;self.error=None;self.retry_at=Instant::now();}
    pub async fn shutdown(&mut self){
        self.close();
        if let Some(listener)=self.listener.take(){let _=tokio::time::timeout(Duration::from_secs(5),listener.shutdown()).await;}
        self.stopping=false;self.owner=None;
    }
    fn install(&mut self,listener:game_rpc::Listener){self.changes=Some(listener.subscribe());self.listener=Some(listener);self.revision=None;self.error=None;}
    #[cfg(test)]
    pub(super) fn install_fake(&mut self,listener:game_rpc::Listener){self.install(listener);}
    fn read(&mut self)->bool {
        let Some(changes)=&mut self.changes else{return self.listener.is_some();};
        if self.revision.is_some()&&!changes.has_changed().unwrap_or(true){return true;}
        let snapshot=changes.borrow_and_update().clone();self.revision=Some(snapshot.revision);
        if !snapshot.listening {self.activity=None;self.error=snapshot.error;return false;}
        match snapshot.activity.as_ref().map(convert).transpose(){Ok(activity)=>{self.activity=activity;self.error=None;},Err(error)=>{self.activity=None;self.error=Some(error);}}
        true
    }
    pub async fn poll(&mut self,allowed:bool,identity:Option<(game_rpc::Identity,u64)>){
        self.poll_cached(allowed,identity.as_ref()).await;
    }
    pub(super) fn needs_fast_poll(&self)->bool {self.listener.is_some()||self.starting.is_some()||self.stopping}
    pub async fn poll_cached(&mut self,allowed:bool,identity:Option<&(game_rpc::Identity,u64)>){
        let Some((identity,generation))=identity.filter(|_|allowed) else{self.shutdown().await;return;};
        if self.stopping||self.owner.as_ref().is_some_and(|previous|previous.0!=identity.id||previous.1!=*generation){self.shutdown().await;self.retry_at=Instant::now();}
        if self.starting.as_ref().is_some_and(|task|task.is_finished()){
            if let Some(task)=self.starting.take(){match task.await{
                Ok(Ok(listener))=>self.install(listener),
                Ok(Err(error))=>{self.error=Some(error);self.retry_at=Instant::now()+Duration::from_secs(60);},
                Err(_)=>{self.error=Some("No se pudo iniciar el receptor local de actividad.".into());self.retry_at=Instant::now()+Duration::from_secs(60);},
            }}
        }
        if self.listener.is_some()&&!self.read(){self.shutdown().await;self.retry_at=Instant::now()+Duration::from_secs(60);}
        if self.listener.is_none()&&self.starting.is_none()&&Instant::now()>=self.retry_at{
            self.owner=Some((identity.id.clone(),*generation));self.starting=Some(tokio::spawn(game_rpc::start(identity.clone())));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn activity()->game_rpc::Activity{game_rpc::Activity{client_id:RPCS3_ID.into(),pid:123,executable:"rpcs3.exe".into(),name:Some("FORGED payload title".into()),details:"Synthetic PS3 game".into(),state:"[BLUS31604]".into(),timestamps:Some(game_rpc::Timestamps{start_ms:Some(1_700_000_000_000),end_ms:None}),assets:Some(game_rpc::Assets{large_image:Some("rpcs3_logo".into()),..Default::default()}),party:None,instance:None}}
    #[test]
    fn rpc_maps_only_verified_program_identity_and_normalized_safe_metadata(){
        let input=activity();let result=convert(&input).unwrap();assert_eq!(result.name,"RPCS3");assert_eq!(result.application_id.as_deref(),Some(RPCS3_ID));assert_eq!(result.timestamps_ms.unwrap().start,Some(1_700_000_000_000));assert!(result.assets.is_none());
        let mut input=activity();input.executable="ExampleGame.exe".into();assert_eq!(convert(&input).unwrap().name,"ExampleGame");input.executable=r"C:\Users\Someone\ExampleGame.exe".into();assert!(convert(&input).is_err());
        let mut input=activity();input.timestamps.as_mut().unwrap().start_ms=Some(-1);assert!(convert(&input).is_err());
        for text in [r"C:\Users\Someone\private", "file:///private/data", "Editing /home/person/project",r"\\server\secret"]{let mut input=activity();input.details=text.into();assert!(convert(&input).is_err());}
    }
    #[tokio::test]
    async fn privacy_stop_clears_fake_listener_and_never_creates_an_ipc_endpoint(){
        let(listener,updates)=game_rpc::Listener::fake();let mut receiver=Receiver::new();receiver.install(listener);
        updates.send_modify(|snapshot|{snapshot.activity=Some(activity());snapshot.revision+=1;});assert!(receiver.read());assert!(receiver.activity.is_some());
        receiver.poll(false,None).await;assert!(receiver.activity.is_none());assert!(!updates.borrow().listening);assert!(updates.borrow().activity.is_none());assert!(receiver.listener.is_none()&&receiver.starting.is_none());
    }
    #[test]
    fn idle_rpcs3_is_distinct_from_an_actual_game(){let mut input=activity();input.details="Idle".into();input.state.clear();assert!(idle_emulator(&convert(&input).unwrap()));input.details="Synthetic game".into();assert!(!idle_emulator(&convert(&input).unwrap()));}
}
