//! Explicitly started local Discord IPC compatibility. No account credentials,
//! outgoing connections, HTTP requests, launchers, or message actions live here.
use std::collections::HashMap;
use tokio::{net::windows::named_pipe::NamedPipeServer,sync::{mpsc,watch},task::{JoinHandle,JoinSet}};
use tokio_util::sync::CancellationToken;

mod protocol;
mod security;
#[cfg(test)] mod tests;
const MAX_CLIENTS:usize=8;

#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Identity {pub id:String,pub username:String}
#[derive(Clone,Debug,Default,PartialEq,Eq)]
pub struct Snapshot {
    pub listening:bool,
    pub pipe_index:Option<u8>,
    pub revision:u64,
    pub activity:Option<Activity>,
    pub error:Option<String>,
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct Activity {
    pub client_id:String,
    pub pid:u32,
    pub executable:String,
    pub name:Option<String>,
    pub details:String,
    pub state:String,
    pub timestamps:Option<Timestamps>,
    pub assets:Option<Assets>,
    pub party:Option<Party>,
    pub instance:Option<bool>,
}
#[derive(Clone,Debug,Default,PartialEq,Eq)]
pub struct Timestamps {pub start_ms:Option<i64>,pub end_ms:Option<i64>}
#[derive(Clone,Debug,Default,PartialEq,Eq)]
pub struct Assets {
    pub large_image:Option<String>,pub large_text:Option<String>,
    pub small_image:Option<String>,pub small_text:Option<String>,
}
#[derive(Clone,Debug,Default,PartialEq,Eq)]
pub struct Party {pub id:Option<String>,pub size:Option<(u32,u32)>,pub privacy:Option<u8>}

pub struct Listener {
    close:CancellationToken,
    updates:watch::Sender<Snapshot>,
    task:Option<JoinHandle<()>>,
}
impl Listener {
    pub fn snapshot(&self)->Snapshot {self.updates.borrow().clone()}
    pub fn subscribe(&self)->watch::Receiver<Snapshot> {self.updates.subscribe()}
    pub fn close(&self) {
        self.close.cancel();
        self.updates.send_modify(|snapshot| {
            if snapshot.listening||snapshot.activity.is_some() {
                snapshot.listening=false;snapshot.activity=None;snapshot.revision=snapshot.revision.wrapping_add(1);
            }
        });
    }
    pub async fn shutdown(mut self) {
        self.close();
        if let Some(task)=self.task.take() {let _=task.await;}
    }
    #[cfg(test)]
    pub(crate) fn fake()->(Self,watch::Sender<Snapshot>) {
        let (updates,_)=watch::channel(Snapshot {listening:true,..Default::default()});
        (Self {close:CancellationToken::new(),updates:updates.clone(),task:None},updates)
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.close();
        if let Some(task)=self.task.take() {task.abort();}
    }
}

/// The controller must authorize privacy, settings, Gateway and network safety
/// before calling this function, and close it again as soon as authorization ends.
pub async fn start(identity:Identity)->Result<Listener,String> {
    start_under(identity,r"\\.\pipe\discord-ipc").await
}
async fn start_under(identity:Identity,base:&str)->Result<Listener,String> {
    if !protocol::valid_id(&identity.id)||identity.username.is_empty()||identity.username.chars().count()>80
        ||identity.username.chars().any(char::is_control) {
        return Err("La identidad autorizada de Rich Presence no es válida.".into());
    }
    let security=security::Context::current()?;
    let mut available=None;
    // FIRST_PIPE_INSTANCE never attaches to or replaces the original client's
    // endpoint. If it owns an index, only a later free index is considered.
    for index in 0..10u8 {
        let name=format!("{base}-{index}");
        if let Ok(pipe)=security.pipe(&name,true) {available=Some((index,name,pipe));break;}
    }
    let Some((index,name,pipe))=available else {return Err("No hay un canal local de Rich Presence disponible.".into());};
    let close=CancellationToken::new();
    let (updates,_)=watch::channel(Snapshot {listening:true,pipe_index:Some(index),..Default::default()});
    let task=tokio::spawn(run_listener(pipe,name,security,identity,updates.clone(),close.clone()));
    Ok(Listener {close,updates,task:Some(task)})
}

enum Event {Activity {connection:u64,activity:Option<Activity>}}
#[derive(Default)]
struct Activities {next:u64,clients:HashMap<u64,(u64,Option<Activity>)>}
impl Activities {
    fn set(&mut self,connection:u64,activity:Option<Activity>) {
        self.next=self.next.wrapping_add(1);self.clients.insert(connection,(self.next,activity));
    }
    fn remove(&mut self,connection:u64) {self.clients.remove(&connection);}
    fn winner(&self)->Option<Activity> {
        self.clients.values().filter_map(|(order,activity)|activity.as_ref().map(|activity|(*order,activity)))
            .max_by_key(|(order,_)|*order).map(|(_,activity)|activity.clone())
    }
    fn publish(&self,updates:&watch::Sender<Snapshot>,close:&CancellationToken) {
        if close.is_cancelled() {return;}
        let activity=self.winner();
        updates.send_if_modified(|snapshot| {
            // close() may have cleared the watch value after the outer check.
            if !snapshot.listening||close.is_cancelled() {return false;}
            if snapshot.activity==activity {return false;}
            snapshot.activity=activity;snapshot.revision=snapshot.revision.wrapping_add(1);true
        });
    }
}
async fn run_listener(
    mut pipe:NamedPipeServer,name:String,security:security::Context,identity:Identity,
    updates:watch::Sender<Snapshot>,close:CancellationToken,
) {
    let (events,mut incoming)=mpsc::channel(32);
    let mut clients=JoinSet::new();let mut activities=Activities::default();let mut next_connection=0u64;
    let mut fatal=None;
    loop {
        tokio::select! {biased;
            _=close.cancelled()=>break,
            result=clients.join_next(),if !clients.is_empty()=> {
                if let Some(Ok(connection))=result {activities.remove(connection);activities.publish(&updates,&close);}
            }
            Some(event)=incoming.recv()=>match event {
                Event::Activity {connection,activity}=> {
                    // Late events from an already removed client cannot revive it.
                    if activities.clients.contains_key(&connection) {activities.set(connection,activity);activities.publish(&updates,&close);}
                }
            },
            connected=pipe.connect(),if clients.len()<MAX_CLIENTS=> {
                if connected.is_err() {fatal=Some("Se cerró el receptor local de Rich Presence.".into());break;}
                let peer=security.verify(&pipe);
                // Create the next server while the connected handle still owns
                // this name. There is never an unowned gap between accepts.
                let replacement=match security.pipe(&name,false) {Ok(pipe)=>pipe,Err(_)=> {
                    fatal=Some("No se pudo mantener el receptor local de Rich Presence.".into());break;
                }};
                let connected=std::mem::replace(&mut pipe,replacement);
                if let Ok(peer)=peer {
                    next_connection=next_connection.wrapping_add(1);let connection=next_connection;
                    activities.set(connection,None);
                    let identity=identity.clone();let events=events.clone();let cancelled=close.clone();
                    clients.spawn(async move {
                        tokio::select! {_=cancelled.cancelled()=>{},_=protocol::serve(connected,peer,identity,connection,events)=>{}}
                        connection
                    });
                }
            }
        }
    }
    close.cancel();
    // shutdown() is also a resource boundary: wait until every aborted client
    // future has dropped its connected pipe before releasing the listening name.
    clients.shutdown().await;
    drop(pipe);
    updates.send_modify(|snapshot| {
        snapshot.listening=false;snapshot.activity=None;snapshot.error=fatal;snapshot.revision=snapshot.revision.wrapping_add(1);
    });
}
