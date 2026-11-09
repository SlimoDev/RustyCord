//! Bounded JSON egress. A popped message retains its reservation while the pipe
//! writer is blocked. Telemetry is replaceable; control overflow is terminal.
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};
const MESSAGES: usize = 128;
const BYTES: usize = 4 * 1024 * 1024;
const TELEMETRY: usize = 32;
struct Entry { value: Value, bytes: usize }
#[derive(Default)]
struct State { control: VecDeque<Entry>, latest: Vec<(String, Entry)>, messages: usize, bytes: usize,
    peak: usize, coalesced: u64, fault: bool, terminal: bool }
struct Shared { state: Mutex<State>, closing: Arc<AtomicBool> }
#[derive(Clone)]
pub(super) struct Sender(Arc<Shared>);
pub(super) struct Receiver(Arc<Shared>);
pub(super) fn channel(closing: Arc<AtomicBool>) -> (Sender, Receiver) {
    let shared=Arc::new(Shared { state:Mutex::new(State::default()), closing });
    (Sender(shared.clone()), Receiver(shared))
}
fn key(value:&Value)->Option<String> {
    match value["event"].as_str()? {
        "inputLevel" => Some(format!("input:{}",value["mediaSession"].as_u64().unwrap_or(0))),
        "micTest" if value.get("level").is_some() && value["active"]==true && value["pending"]==false =>
            Some(format!("mic:{}",value["token"].as_u64().unwrap_or(0))),
        _=>None,
    }
}
impl Sender {
    pub(super) fn send(&self,value:Value)->Result<(),()> {
        let bytes=serde_json::to_vec(&value).map_err(|_|())?.len();
        let mut state=self.0.state.lock().unwrap_or_else(|e|e.into_inner());
        if state.fault {return Err(());}
        if let Some(key)=key(&value) {
            if let Some(index)=state.latest.iter().position(|(old,_)|old==&key) {
                let old=state.latest.remove(index).1;state.bytes-=old.bytes;state.messages-=1;state.coalesced+=1;
            } else if state.latest.len()>=TELEMETRY {state.coalesced+=1;return Ok(());}
            if bytes<=4096 && state.messages<MESSAGES && state.bytes.saturating_add(bytes)<=BYTES {
                state.bytes+=bytes;state.messages+=1;state.latest.push((key,Entry{value,bytes}));
                state.peak=state.peak.max(state.messages);return Ok(());
            }
            state.coalesced+=1;return Ok(());
        }
        if state.messages>=MESSAGES || state.bytes.saturating_add(bytes)>BYTES {
            state.fault=true;state.terminal=true;self.0.closing.store(true,Ordering::Release);
            return Err(());
        }
        state.bytes+=bytes;state.messages+=1;state.control.push_back(Entry{value,bytes});
        state.peak=state.peak.max(state.messages);Ok(())
    }
}
pub(super) struct Drain<'a> { receiver:&'a Receiver, reserved:Option<usize> }
impl Receiver {pub(super) fn try_iter(&self)->Drain<'_>{Drain{receiver:self,reserved:None}}}
impl Drain<'_> {
    fn release(&mut self){if let Some(bytes)=self.reserved.take(){let mut state=self.receiver.0.state.lock().unwrap_or_else(|e|e.into_inner());state.bytes-=bytes;state.messages-=1;}}
}
impl Iterator for Drain<'_> {
    type Item=Value;
    fn next(&mut self)->Option<Value>{
        self.release();let mut state=self.receiver.0.state.lock().unwrap_or_else(|e|e.into_inner());
        if std::mem::take(&mut state.terminal) {return Some(json!({"event":"fatal","error":{"code":"MEDIA_OUTPUT_QUEUE_FULL","message":"La salida multimedia no pudo entregar el control de forma segura."},"outputQueue":{"maxMessages":MESSAGES,"maxBytes":BYTES,"peak":state.peak,"coalesced":state.coalesced}}));}
        let entry=state.control.pop_front().or_else(||if state.latest.is_empty(){None}else{Some(state.latest.remove(0).1)})?;
        self.reserved=Some(entry.bytes);Some(entry.value)
    }
}
impl Drop for Drain<'_>{fn drop(&mut self){self.release();}}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn stopped_pipe_keeps_inflight_budget_and_control_saturation_is_one_explicit_failure(){
        let closing=Arc::new(AtomicBool::new(false));let (tx,rx)=channel(closing.clone());
        tx.send(json!({"id":1})).unwrap();let mut pipe=rx.try_iter();assert_eq!(pipe.next().unwrap()["id"],1);
        for id in 2..=MESSAGES {tx.send(json!({"id":id})).unwrap();}
        assert!(tx.send(json!({"id":129})).is_err());assert!(closing.load(Ordering::Acquire));
        assert!(tx.send(json!({"id":130})).is_err());drop(pipe);
        let events:Vec<_>=rx.try_iter().collect();assert_eq!(events.iter().filter(|v|v["event"]=="fatal").count(),1);
    }
    #[test] fn latest_level_is_coalesced_without_losing_ordered_replies(){
        let (tx,rx)=channel(Arc::new(AtomicBool::new(false)));
        for level in 0..10000 {tx.send(json!({"event":"inputLevel","mediaSession":1,"level":level})).unwrap();}
        tx.send(json!({"id":1,"ok":true})).unwrap();tx.send(json!({"id":2,"ok":true})).unwrap();
        let events:Vec<_>=rx.try_iter().collect();assert_eq!(events.len(),3);assert_eq!(events[0]["id"],1);assert_eq!(events[1]["id"],2);assert_eq!(events[2]["level"],9999);
    }
}
