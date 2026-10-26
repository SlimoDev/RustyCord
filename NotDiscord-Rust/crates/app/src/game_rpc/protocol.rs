//! Discord IPC v1: eight-byte little-endian header, bounded UTF-8 JSON payload.
//! References: discord/discord-rpc serialization.cpp and rpc_connection.cpp;
//! installed RPCIPC.js and the matching cached Discord web bundle.
use super::{Activity,Assets,Event,Identity,Party,Timestamps,security::Peer};
use serde_json::{Map,Value,json};
use std::{io,time::{Duration,Instant}};
use tokio::{io::{AsyncRead,AsyncReadExt,AsyncWrite,AsyncWriteExt},sync::mpsc};
const MAX_FRAME:usize=16*1024;
const FRAME_TIMEOUT:Duration=Duration::from_secs(5);
const HANDSHAKE:u32=0;
const FRAME:u32=1;
const CLOSE:u32=2;
const PING:u32=3;
const PONG:u32=4;
pub(super) fn valid_id(value:&str)->bool {
    !value.is_empty()&&value.len()<=20&&value.bytes().all(|byte|byte.is_ascii_digit())
        &&value.parse::<u64>().is_ok_and(|id|id!=0)
}
#[derive(Debug)]
pub(super) struct Frame {pub opcode:u32,pub bytes:Vec<u8>}
pub(super) async fn read_frame<R:AsyncRead+Unpin>(reader:&mut R)->io::Result<Option<Frame>> {
    let mut header=[0u8;8];
    if reader.read(&mut header[..1]).await?==0 {return Ok(None);}
    // An idle RPC client may legitimately have no updates for hours. Only an
    // already-started partial frame has a completion deadline.
    tokio::time::timeout(FRAME_TIMEOUT,async {
        reader.read_exact(&mut header[1..]).await?;
        let opcode=u32::from_le_bytes(header[..4].try_into().unwrap());
        let size=u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        if size>MAX_FRAME {return Err(io::Error::new(io::ErrorKind::InvalidData,"RPC frame is too large"));}
        let mut bytes=vec![0;size];reader.read_exact(&mut bytes).await?;
        Ok(Some(Frame {opcode,bytes}))
    }).await.map_err(|_|io::Error::new(io::ErrorKind::TimedOut,"RPC partial frame timed out"))?
}
async fn write_frame<W:AsyncWrite+Unpin>(writer:&mut W,opcode:u32,bytes:&[u8])->io::Result<()> {
    if bytes.len()>MAX_FRAME {return Err(io::ErrorKind::InvalidData.into());}
    let mut header=[0u8;8];header[..4].copy_from_slice(&opcode.to_le_bytes());header[4..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    tokio::time::timeout(FRAME_TIMEOUT,async {
        writer.write_all(&header).await?;writer.write_all(bytes).await?;writer.flush().await
    }).await.map_err(|_|io::Error::from(io::ErrorKind::TimedOut))?
}
async fn reply<W:AsyncWrite+Unpin>(writer:&mut W,opcode:u32,value:Value)->io::Result<()> {
    write_frame(writer,opcode,&serde_json::to_vec(&value)?).await
}
async fn reject<W:AsyncWrite+Unpin>(writer:&mut W,cmd:&str,nonce:Value,message:&str)->io::Result<()> {
    reply(writer,FRAME,json!({"cmd":cmd,"evt":"ERROR","nonce":nonce,"data":{"code":4000,"message":message}})).await
}
fn object(value:&Value)->Result<&Map<String,Value>,&'static str> {value.as_object().ok_or("Se esperaba un objeto JSON.")}
fn optional_text(value:Option<&Value>,max:usize)->Result<Option<String>,&'static str> {
    match value {
        None|Some(Value::Null)=>Ok(None),
        Some(Value::String(text)) if text.chars().count()<=max&&!text.chars().any(char::is_control)=>Ok(Some(text.clone())),
        _=>Err("El texto de actividad no es válido."),
    }
}
fn timestamp(value:Option<&Value>)->Result<Option<i64>,&'static str> {
    match value {
        None|Some(Value::Null)=>Ok(None),
        Some(value)=>normalize_timestamp(value.as_i64().ok_or("Marca de tiempo RPC inválida.")?,chrono::Utc::now().timestamp_millis()).map(Some),
    }
}
pub(super) fn normalize_timestamp(value:i64,now_ms:i64)->Result<i64,&'static str> {
    const MAX_SAFE_INTEGER:i64=9_007_199_254_740_991;
    if !(0..=MAX_SAFE_INTEGER).contains(&value) {return Err("Marca de tiempo RPC inválida.");}
    // Match SET_ACTIVITY in the shipped renderer: values whose digit count is
    // more than two below Date.now() are seconds; otherwise they are already ms.
    let millis=if now_ms.to_string().len().saturating_sub(value.to_string().len())>2 {
        value.checked_mul(1000).ok_or("Marca de tiempo RPC inválida.")?
    } else {value};
    if millis>MAX_SAFE_INTEGER {return Err("Marca de tiempo RPC inválida.");}Ok(millis)
}
fn parse_activity(value:&Value,client_id:&str,peer:&Peer)->Result<Activity,&'static str> {
    let activity=object(value)?;
    if let Some(kind)=activity.get("type") {if kind.as_u64()!=Some(0) {return Err("Sólo se admite actividad de juego.");}}
    let name=optional_text(activity.get("name"),128)?;
    let details=optional_text(activity.get("details"),128)?.unwrap_or_default();
    let state=optional_text(activity.get("state"),128)?.unwrap_or_default();
    let timestamps=match activity.get("timestamps") {
        None|Some(Value::Null)=>None,
        Some(value)=> {
            let values=object(value)?;let start_ms=timestamp(values.get("start"))?;let end_ms=timestamp(values.get("end"))?;
            if start_ms.zip(end_ms).is_some_and(|(start,end)|end<start) {return Err("El final de actividad precede al inicio.");}
            Some(Timestamps {start_ms,end_ms})
        }
    };
    let assets=match activity.get("assets") {
        None|Some(Value::Null)=>None,
        Some(value)=> {let values=object(value)?;Some(Assets {
            large_image:optional_text(values.get("large_image"),256)?,large_text:optional_text(values.get("large_text"),128)?,
            small_image:optional_text(values.get("small_image"),256)?,small_text:optional_text(values.get("small_text"),128)?,
        })}
    };
    let party=match activity.get("party") {
        None|Some(Value::Null)=>None,
        Some(value)=> {
            let values=object(value)?;let id=optional_text(values.get("id"),128)?;
            let size=match values.get("size") {
                None|Some(Value::Null)=>None,
                Some(value)=> {
                    let values=value.as_array().filter(|values|values.len()==2).ok_or("Tamaño de grupo inválido.")?;
                    let current=values[0].as_u64().filter(|value|*value<=1_000_000).ok_or("Tamaño de grupo inválido.")? as u32;
                    let maximum=values[1].as_u64().filter(|value|(1..=1_000_000).contains(value)).ok_or("Tamaño de grupo inválido.")? as u32;
                    if current>maximum {return Err("Tamaño de grupo inválido.");}Some((current,maximum))
                }
            };
            let privacy=match values.get("privacy") {None|Some(Value::Null)=>None,Some(value)=>Some(value.as_u64().filter(|value|*value<=1).ok_or("Privacidad de grupo inválida.")? as u8)};
            Some(Party {id,size,privacy})
        }
    };
    let instance=match activity.get("instance") {None|Some(Value::Null)=>None,Some(value)=>Some(value.as_bool().ok_or("Instancia inválida.")?)};
    Ok(Activity {client_id:client_id.into(),pid:peer.pid,executable:peer.executable.clone(),name,details,state,timestamps,assets,party,instance})
}
fn wire_activity(activity:&Activity)->Value {
    let mut value=json!({"type":0,"details":activity.details,"state":activity.state});
    let values=value.as_object_mut().unwrap();
    if let Some(name)=&activity.name {values.insert("name".into(),json!(name));}
    if let Some(timestamps)=&activity.timestamps {
        let mut times=Map::new();
        if let Some(start)=timestamps.start_ms {times.insert("start".into(),json!(start));}
        if let Some(end)=timestamps.end_ms {times.insert("end".into(),json!(end));}
        values.insert("timestamps".into(),Value::Object(times));
    }
    if let Some(assets)=&activity.assets {
        let mut images=Map::new();
        for (key,value) in [("large_image",&assets.large_image),("large_text",&assets.large_text),("small_image",&assets.small_image),("small_text",&assets.small_text)] {
            if let Some(value)=value {images.insert(key.into(),json!(value));}
        }
        values.insert("assets".into(),Value::Object(images));
    }
    if let Some(party)=&activity.party {
        let mut group=Map::new();if let Some(id)=&party.id {group.insert("id".into(),json!(id));}
        if let Some(size)=party.size {group.insert("size".into(),json!([size.0,size.1]));}
        if let Some(privacy)=party.privacy {group.insert("privacy".into(),json!(privacy));}
        values.insert("party".into(),Value::Object(group));
    }
    if let Some(instance)=activity.instance {values.insert("instance".into(),json!(instance));}
    value
}
struct Quota {start:Instant,frames:usize}
impl Quota {
    fn allow(&mut self)->bool {
        if self.start.elapsed()>=Duration::from_secs(5) {self.start=Instant::now();self.frames=0;}
        self.frames+=1;self.frames<=32
    }
}
pub(super) async fn serve<S:AsyncRead+AsyncWrite+Unpin>(
    mut stream:S,peer:Peer,identity:Identity,connection:u64,events:mpsc::Sender<Event>,
) ->io::Result<()> {
    let Some(handshake)=tokio::time::timeout(FRAME_TIMEOUT,read_frame(&mut stream)).await
        .map_err(|_|io::Error::from(io::ErrorKind::TimedOut))?? else {return Ok(());};
    let value:Value=serde_json::from_slice(&handshake.bytes)?;
    if handshake.opcode!=HANDSHAKE||value["v"].as_u64()!=Some(1)||!value["client_id"].as_str().is_some_and(valid_id) {
        reply(&mut stream,CLOSE,json!({"code":4000,"message":"Handshake RPC inválido."})).await?;return Ok(());
    }
    let client_id=value["client_id"].as_str().unwrap().to_owned();
    reply(&mut stream,FRAME,json!({"cmd":"DISPATCH","evt":"READY","nonce":null,
        "data":{"v":1,"config":{"environment":"production","cdn_host":"cdn.discordapp.com","api_endpoint":"//discord.com/api"},
            "user":{"id":identity.id,"username":identity.username,"discriminator":"0","avatar":null}}})).await?;
    let mut quota=Quota {start:Instant::now(),frames:0};
    while let Some(frame)=read_frame(&mut stream).await? {
        if !quota.allow() {reply(&mut stream,CLOSE,json!({"code":4008,"message":"Demasiadas solicitudes RPC locales."})).await?;break;}
        match frame.opcode {
            PING=>write_frame(&mut stream,PONG,&frame.bytes).await?,
            PONG=>{},
            CLOSE=>break,
            FRAME=> {
                let value:Value=serde_json::from_slice(&frame.bytes)?;
                let command=optional_text(value.get("cmd"),64).ok().flatten().unwrap_or_default();
                let nonce=optional_text(value.get("nonce"),128).ok().flatten();
                if command!="SET_ACTIVITY" {reject(&mut stream,&command,nonce.map_or(Value::Null,Value::String),"Comando RPC no admitido.").await?;continue;}
                let Some(nonce)=nonce else {reject(&mut stream,&command,Value::Null,"Nonce RPC inválido.").await?;continue;};
                let parsed=(|| {
                    let args=object(&value["args"])?;
                    if let Some(pid)=args.get("pid") {if pid.as_u64()!=Some(peer.pid as u64) {return Err("El PID RPC no corresponde al cliente verificado.");}}
                    // Official discord-rpc omits activity to clear presence.
                    match args.get("activity") {None|Some(Value::Null)=>Ok(None),Some(value)=>parse_activity(value,&client_id,&peer).map(Some)}
                })();
                match parsed {
                    Err(error)=> {
                        events.send(Event::Activity {connection,activity:None}).await.map_err(|_|io::Error::from(io::ErrorKind::BrokenPipe))?;
                        reject(&mut stream,&command,Value::String(nonce),error).await?;
                    }
                    Ok(activity)=> {
                        let data=activity.as_ref().map(wire_activity).unwrap_or(Value::Null);
                        events.send(Event::Activity {connection,activity}).await.map_err(|_|io::Error::from(io::ErrorKind::BrokenPipe))?;
                        reply(&mut stream,FRAME,json!({"cmd":"SET_ACTIVITY","evt":null,"nonce":nonce,"data":data})).await?;
                    }
                }
            }
            _=> {reply(&mut stream,CLOSE,json!({"code":4000,"message":"Opcode RPC no admitido."})).await?;break;}
        }
    }
    Ok(())
}
