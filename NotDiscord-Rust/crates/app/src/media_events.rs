//! One bounded reservation from pipe decoding through actor completion. Latest
//! telemetry replaces queued telemetry; control overload terminates that host.
use serde_json::{Value,json};
use std::{collections::{VecDeque,HashMap},sync::{Arc,Mutex,OnceLock,Weak,atomic::{AtomicUsize,Ordering}}};
use tokio::sync::{Notify,Semaphore,OwnedSemaphorePermit,mpsc::error::TryRecvError};
const MESSAGES:usize=128;const BYTES:usize=4*1024*1024;const TELEMETRY:usize=32;
struct Budget{messages:Arc<Semaphore>,bytes:Arc<Semaphore>,terminals:Arc<Semaphore>,telemetry:Arc<Semaphore>}
impl Budget{fn new()->Self{Self{messages:Arc::new(Semaphore::new(MESSAGES)),bytes:Arc::new(Semaphore::new(BYTES)),terminals:Arc::new(Semaphore::new(16)),telemetry:Arc::new(Semaphore::new(TELEMETRY))}}}
fn budget()->Arc<Budget>{static B:OnceLock<Arc<Budget>>=OnceLock::new();B.get_or_init(||Arc::new(Budget::new())).clone()}
/// Deliberately held by Completed even after the forwarding task advances.
pub(crate) struct Permit{_message:Option<OwnedSemaphorePermit>,_bytes:Option<OwnedSemaphorePermit>,_terminal:Option<OwnedSemaphorePermit>,_telemetry:Option<OwnedSemaphorePermit>,latest:Option<Arc<Mutex<Value>>>}
impl Permit{pub(crate) fn refresh(&self,mut value:Value)->Value{if let Some(latest)=&self.latest{let latest=latest.lock().unwrap().clone();if value["event"]=="receiver-host"{value["value"]=latest;}else{value=latest;}}value}}
pub(crate) struct Delivery{value:Value,permit:Permit}
impl std::ops::Deref for Delivery{type Target=Value;fn deref(&self)->&Value{&self.value}}
impl Delivery{pub(crate) fn into_parts(self)->(Value,Permit){(self.value,self.permit)}}
#[derive(Default)]struct State{control:VecDeque<Delivery>,latest:Vec<(String,Delivery)>,slots:HashMap<String,Weak<Mutex<Value>>>,terminal:Option<OwnedSemaphorePermit>,fault:bool,closed:bool,close:Option<tokio_util::sync::CancellationToken>}
struct Shared{state:Mutex<State>,notify:Notify,senders:AtomicUsize,budget:Arc<Budget>}
pub(crate) struct Sender(Arc<Shared>);
pub(crate) struct Receiver(Arc<Shared>);
pub(crate) fn channel()->(Sender,Receiver){channel_using(budget())}
fn channel_using(budget:Arc<Budget>)->(Sender,Receiver){let terminal=budget.terminals.clone().try_acquire_owned().ok();let closed=terminal.is_none();let shared=Arc::new(Shared{state:Mutex::new(State{terminal,closed,..Default::default()}),notify:Notify::new(),senders:AtomicUsize::new(1),budget});(Sender(shared.clone()),Receiver(shared))}
impl Clone for Sender{fn clone(&self)->Self{self.0.senders.fetch_add(1,Ordering::Relaxed);Self(self.0.clone())}}
impl Drop for Sender{fn drop(&mut self){if self.0.senders.fetch_sub(1,Ordering::AcqRel)==1{self.0.state.lock().unwrap().closed=true;self.0.notify.notify_waiters();}}}
impl Sender{
    pub(crate) fn available(&self)->bool{let state=self.0.state.lock().unwrap();!state.closed&&!state.fault}
    pub(crate) fn bind_close(&self,close:tokio_util::sync::CancellationToken){let mut state=self.0.state.lock().unwrap();if state.closed||state.fault{close.cancel();}state.close=Some(close);}
    pub(crate) fn send(&self,value:Value)->Result<(),()>{
        let mut state=self.0.state.lock().unwrap();if state.closed||state.fault{return Err(());}
        let key=match value["event"].as_str(){Some("inputLevel")=>Some(format!("input:{}:{}",value["mediaSession"],value["transportGeneration"])),Some("micTest") if value["active"]==true&&value["pending"]==false&&value.get("level").is_some()=>Some(format!("mic:{}",value["token"])),_=>None};
        let size=serde_json::to_vec(&value).map_err(|_|())?.len();
        if let Some(key)=&key{
            if size>4096{return Ok(());}
            if let Some(slot)=state.slots.get(key).and_then(Weak::upgrade){*slot.lock().unwrap()=value;return Ok(());}
            state.slots.retain(|_,slot|slot.strong_count()>0);if state.slots.len()>=TELEMETRY{return Ok(());}
        }
        let telemetry=if key.is_some(){match self.0.budget.telemetry.clone().try_acquire_owned(){Ok(permit)=>Some(permit),Err(_)=>return Ok(())}}else{None};
        let allocation=if size<=BYTES{self.0.budget.messages.clone().try_acquire_owned().ok().and_then(|message|self.0.budget.bytes.clone().try_acquire_many_owned(if key.is_some(){8192}else{size as u32}).ok().map(|bytes|Permit{_message:Some(message),_bytes:Some(bytes),_terminal:None,_telemetry:telemetry,latest:None}))}else{None};
        if let Some(mut permit)=allocation{if let Some(key)=&key{let slot=Arc::new(Mutex::new(value.clone()));state.slots.insert(key.clone(),Arc::downgrade(&slot));permit.latest=Some(slot);}let delivery=Delivery{value,permit};if let Some(key)=key{state.latest.push((key,delivery));}else{state.control.push_back(delivery);}drop(state);self.0.notify.notify_one();return Ok(());}
        if key.is_some(){return Ok(());}
        state.fault=true;if let Some(close)=&state.close{close.cancel();}drop(state);self.0.notify.notify_one();Err(())
    }
}
impl Receiver{
    pub(crate) fn try_recv(&mut self)->Result<Delivery,TryRecvError>{let mut state=self.0.state.lock().unwrap();
        if state.fault&&let Some(terminal)=state.terminal.take(){state.closed=true;state.control.clear();state.latest.clear();state.slots.clear();return Ok(Delivery{value:json!({"event":"fatal","error":{"code":"MEDIA_OUTPUT_QUEUE_FULL","controlCode":"MEDIA_OUTPUT_QUEUE_FULL","message":"La salida multimedia excedió su presupuesto de entrega."}}),permit:Permit{_message:None,_bytes:None,_terminal:Some(terminal),_telemetry:None,latest:None}});}
        if let Some(event)=state.control.pop_front(){return Ok(event);}
        if !state.latest.is_empty(){let mut delivery=state.latest.remove(0).1;delivery.value=delivery.permit.refresh(delivery.value);return Ok(delivery);}
        Err(if state.closed||state.fault{TryRecvError::Disconnected}else{TryRecvError::Empty})
    }
    pub(crate) async fn recv(&mut self)->Option<Delivery>{loop{let shared=self.0.clone();let notified=shared.notify.notified();tokio::pin!(notified);notified.as_mut().enable();match self.try_recv(){Ok(value)=>return Some(value),Err(TryRecvError::Disconnected)=>return None,Err(TryRecvError::Empty)=>notified.await}}}
}
impl Drop for Receiver{fn drop(&mut self){let mut state=self.0.state.lock().unwrap();state.closed=true;state.control.clear();state.latest.clear();if let Some(close)=&state.close{close.cancel();}}}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn blocked_actor_keeps_its_inflight_reservation_until_completion_is_dropped(){
        let shared=Arc::new(Budget::new());let (tx,mut rx)=channel_using(shared.clone());
        tx.send(json!({"id":1})).unwrap();let (_,held)=rx.try_recv().unwrap().into_parts();
        for id in 2..=MESSAGES {tx.send(json!({"id":id})).unwrap();}
        assert_eq!(shared.messages.available_permits(),0);
        assert!(tx.send(json!({"id":129})).is_err());
        let terminal=rx.try_recv().unwrap();assert_eq!(terminal["event"],"fatal");
        assert_eq!(shared.messages.available_permits(),MESSAGES-1);assert!(matches!(rx.try_recv(),Err(TryRecvError::Disconnected)));drop(held);assert_eq!(shared.messages.available_permits(),MESSAGES);
    }
    #[tokio::test]async fn reservation_follows_completion_and_telemetry_is_coalesced(){
        let (tx,mut rx)=channel_using(Arc::new(Budget::new()));for n in 0..10000{tx.send(json!({"event":"inputLevel","mediaSession":7,"level":n})).unwrap();}
        let delivery=rx.recv().await.unwrap();assert_eq!(delivery["level"],9999);let (value,permit)=delivery.into_parts();drop(value);
        tx.send(json!({"event":"inputLevel","mediaSession":7,"level":12345})).unwrap();assert_eq!(permit.refresh(json!({}))["level"],12345);
        tx.send(json!({"event":"inputLevel","mediaSession":7,"level":99,"extra":"x".repeat(100000)})).unwrap();
        assert_eq!(permit.refresh(json!({}))["level"],12345);
        assert!(serde_json::to_vec(&permit.refresh(json!({}))).unwrap().len()<4096);assert!(rx.try_recv().is_err());drop(permit);
        tx.send(json!({"id":"a"})).unwrap();assert_eq!(rx.recv().await.unwrap()["id"],"a");
    }
}
