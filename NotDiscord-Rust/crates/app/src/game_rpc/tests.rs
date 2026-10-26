use super::*;
use std::time::Duration;
use serde_json::{Value,json};
use tokio::io::{AsyncRead,AsyncWrite,AsyncWriteExt};

fn identity()->Identity {Identity {id:"100".into(),username:"Offline fixture".into()}}
fn peer()->security::Peer {security::Peer {pid:42,executable:"rpcs3.exe".into()}}
async fn send_raw<W:AsyncWrite+Unpin>(writer:&mut W,opcode:u32,bytes:&[u8]) {
    writer.write_all(&opcode.to_le_bytes()).await.unwrap();
    writer.write_all(&(bytes.len() as u32).to_le_bytes()).await.unwrap();
    writer.write_all(bytes).await.unwrap();writer.flush().await.unwrap();
}
async fn send<W:AsyncWrite+Unpin>(writer:&mut W,opcode:u32,value:Value) {send_raw(writer,opcode,&serde_json::to_vec(&value).unwrap()).await;}
async fn receive<R:AsyncRead+Unpin>(reader:&mut R)->protocol::Frame {
    tokio::time::timeout(Duration::from_secs(2),protocol::read_frame(reader)).await.unwrap().unwrap().unwrap()
}
async fn receive_json<R:AsyncRead+Unpin>(reader:&mut R)->Value {
    let frame=receive(reader).await;assert_eq!(frame.opcode,1);serde_json::from_slice(&frame.bytes).unwrap()
}
async fn handshake<S:AsyncRead+AsyncWrite+Unpin>(client:&mut S) {
    send(client,0,json!({"v":1,"client_id":"424004941485572097"})).await;
    let ready=receive_json(client).await;
    assert_eq!(ready["cmd"],"DISPATCH");assert_eq!(ready["evt"],"READY");
    assert_eq!(ready["data"]["v"],1);assert_eq!(ready["data"]["user"]["id"],"100");
    assert_eq!(ready["data"]["user"]["username"],"Offline fixture");
    assert!(ready.get("token").is_none());
}

#[tokio::test]
async fn official_rpcs3_shape_preserves_structured_presence_and_handles_both_clear_forms() {
    let (server,mut client)=tokio::io::duplex(65536);let (events,mut received)=mpsc::channel(32);
    let task=tokio::spawn(protocol::serve(server,peer(),identity(),7,events));
    handshake(&mut client).await;
    let bytes=[0,255,17,2];send_raw(&mut client,3,&bytes).await;
    let pong=receive(&mut client).await;assert_eq!(pong.opcode,4);assert_eq!(pong.bytes,bytes);
    send(&mut client,1,json!({"cmd":"AUTHENTICATE","nonce":"unsupported","args":{"access_token":"never-retain-this"}})).await;
    let rejected=receive_json(&mut client).await;assert_eq!(rejected["evt"],"ERROR");
    assert!(!rejected.to_string().contains("never-retain-this"));assert!(received.try_recv().is_err());
    send(&mut client,1,json!({"cmd":"SET_ACTIVITY","nonce":"wrong-pid","args":{"pid":43,"activity":{"details":"Wrong process"}}})).await;
    assert_eq!(receive_json(&mut client).await["evt"],"ERROR");
    let Event::Activity {activity,..}=received.recv().await.unwrap();assert!(activity.is_none());
    send(&mut client,1,json!({"cmd":"SET_ACTIVITY","nonce":"game","args":{"pid":42,"activity":{
        "details":"Demon's Souls","state":"Playing","timestamps":{"start":1700000000,"end":1700000060},
        "assets":{"large_image":"rpcs3_logo","large_text":"RPCS3","small_image":"12345"},
        "party":{"id":"offline-party","size":[1,4],"privacy":0},"instance":true,
        "secrets":{"join":"never-publish-this"},"buttons":[{"label":"Never execute","url":"https://example.invalid"}]
    }}})).await;
    let acknowledged=receive_json(&mut client).await;
    assert_eq!(acknowledged["cmd"],"SET_ACTIVITY");assert_eq!(acknowledged["nonce"],"game");assert!(acknowledged["evt"].is_null());
    assert_eq!(acknowledged["data"]["details"],"Demon's Souls");
    assert_eq!(acknowledged["data"]["timestamps"]["start"],1700000000000u64);
    assert!(acknowledged["data"].get("name").is_none(),"the game did not supply an application name");
    assert!(!acknowledged.to_string().contains("never-publish-this"));assert!(acknowledged["data"].get("buttons").is_none());
    let Event::Activity {connection,activity}=received.recv().await.unwrap();assert_eq!(connection,7);
    let activity=activity.unwrap();
    assert_eq!((activity.client_id.as_str(),activity.pid,activity.executable.as_str()),("424004941485572097",42,"rpcs3.exe"));
    assert!(activity.name.is_none());assert_eq!(activity.details,"Demon's Souls");assert_eq!(activity.state,"Playing");
    assert_eq!(activity.timestamps.as_ref().unwrap().start_ms,Some(1_700_000_000_000));
    assert_eq!(activity.timestamps.as_ref().unwrap().end_ms,Some(1_700_000_060_000));
    assert_eq!(activity.assets.unwrap().large_image.as_deref(),Some("rpcs3_logo"));
    assert_eq!(activity.party.unwrap().size,Some((1,4)));
    send(&mut client,1,json!({"cmd":"AUTHENTICATE","nonce":"unsupported-after-game","args":{"access_token":"never-retain-this"}})).await;
    assert_eq!(receive_json(&mut client).await["evt"],"ERROR");
    assert!(received.try_recv().is_err(),"unsupported OAuth does not clear a valid game's activity");
    for (nonce,args) in [("clear-null",json!({"pid":42,"activity":null})),("clear-missing",json!({"pid":42}))] {
        send(&mut client,1,json!({"cmd":"SET_ACTIVITY","nonce":nonce,"args":args})).await;
        let acknowledged=receive_json(&mut client).await;assert_eq!(acknowledged["nonce"],nonce);assert!(acknowledged["data"].is_null());
        let Event::Activity {activity,..}=received.recv().await.unwrap();assert!(activity.is_none());
    }
    drop(client);assert!(task.await.unwrap().is_ok());
}

#[tokio::test]
async fn invalid_rpc_metadata_and_commands_never_enter_the_activity_channel() {
    let (server,mut client)=tokio::io::duplex(65536);let (events,mut received)=mpsc::channel(32);
    let task=tokio::spawn(protocol::serve(server,peer(),identity(),1,events));handshake(&mut client).await;
    send(&mut client,1,json!({"cmd":"SET_ACTIVITY","nonce":"previous","args":{"pid":42,"activity":{"details":"Previous title"}}})).await;
    receive_json(&mut client).await;
    let Event::Activity {activity,..}=received.recv().await.unwrap();assert_eq!(activity.unwrap().details,"Previous title");
    for activity in [json!({"type":2}),json!({"details":42}),json!({"details":"x".repeat(129)}),json!({"state":"bad\nstate"}),
        json!({"timestamps":{"start":1700000000,"end":1}}),json!({"timestamps":{"start":9007199254740992u64}}),
        json!({"assets":{"large_image":false}}),json!({"party":{"size":[5,2]}}),json!({"party":{"size":[1,2,3]}}),
        json!({"party":{"privacy":2}}),json!({"instance":"yes"})] {
        send(&mut client,1,json!({"cmd":"SET_ACTIVITY","nonce":"invalid","args":{"pid":42,"activity":activity}})).await;
        assert_eq!(receive_json(&mut client).await["evt"],"ERROR");
        let Event::Activity {activity,..}=received.recv().await.unwrap();assert!(activity.is_none(),"invalid SET_ACTIVITY retires only its own stale presence");
    }
    send(&mut client,1,json!({"cmd":"SET_ACTIVITY","args":{"pid":42,"activity":{"details":"No nonce"}}})).await;
    assert_eq!(receive_json(&mut client).await["evt"],"ERROR");assert!(received.try_recv().is_err());
    send(&mut client,0,json!({"v":1,"client_id":"424004941485572097"})).await;
    assert_eq!(receive(&mut client).await.opcode,2);assert!(task.await.unwrap().is_ok());
}

#[test]
fn timestamps_accept_sdk_seconds_and_already_normalized_milliseconds_without_double_conversion() {
    let now=1_700_000_000_000;
    assert_eq!(protocol::normalize_timestamp(1_700_000_000,now).unwrap(),1_700_000_000_000);
    assert_eq!(protocol::normalize_timestamp(1_700_000_000_123,now).unwrap(),1_700_000_000_123);
    assert_eq!(protocol::normalize_timestamp(17_000_000_000,now).unwrap(),17_000_000_000,"the renderer's two-digit boundary is preserved");
    assert!(protocol::normalize_timestamp(-1,now).is_err());
    assert!(protocol::normalize_timestamp(9_007_199_254_740_992,now).is_err());
}

#[tokio::test]
async fn frames_are_fragment_safe_and_reject_oversized_or_truncated_bodies_before_reading_them() {
    let payload=serde_json::to_vec(&json!({"fixture":"ñ😀"})).unwrap();
    let (mut sender,mut reader)=tokio::io::duplex(128);
    let expected=payload.clone();let task=tokio::spawn(async move {
        let mut bytes=1u32.to_le_bytes().to_vec();bytes.extend((payload.len() as u32).to_le_bytes());bytes.extend(payload);
        for byte in bytes {sender.write_all(&[byte]).await.unwrap();tokio::task::yield_now().await;}
    });
    let frame=receive(&mut reader).await;assert_eq!(frame.opcode,1);assert_eq!(frame.bytes,expected);task.await.unwrap();
    for (declared,body) in [(16385u32,vec![]),(3,vec![b'{'])] {
        let mut bytes=1u32.to_le_bytes().to_vec();bytes.extend(declared.to_le_bytes());bytes.extend(body);
        assert!(protocol::read_frame(&mut bytes.as_slice()).await.is_err());
    }
}

#[tokio::test]
async fn handshake_is_mandatory_versioned_and_local_frame_quota_is_bounded() {
    for (opcode,value) in [(1,json!({"cmd":"SET_ACTIVITY","args":{"activity":{}}})),(0,json!({"v":2,"client_id":"100"})),
        (0,json!({"v":1,"client_id":"0"})),(0,json!({"v":1,"client_id":"bad-client"}))] {
        let (server,mut client)=tokio::io::duplex(4096);let (events,mut received)=mpsc::channel(4);
        let task=tokio::spawn(protocol::serve(server,peer(),identity(),1,events));send(&mut client,opcode,value).await;
        assert_eq!(receive(&mut client).await.opcode,2);assert!(task.await.unwrap().is_ok());assert!(received.try_recv().is_err());
    }
    let (server,mut client)=tokio::io::duplex(4096);let (events,mut received)=mpsc::channel(4);
    let task=tokio::spawn(protocol::serve(server,peer(),identity(),1,events));handshake(&mut client).await;
    for _ in 0..32 {send(&mut client,3,json!({})).await;assert_eq!(receive(&mut client).await.opcode,4);}
    send(&mut client,3,json!({})).await;assert_eq!(receive(&mut client).await.opcode,2);
    assert!(task.await.unwrap().is_ok());assert!(received.try_recv().is_err());
}

fn activity(label:&str)->Activity {
    Activity {client_id:"424004941485572097".into(),pid:42,executable:"rpcs3.exe".into(),name:None,details:label.into(),
        state:String::new(),timestamps:None,assets:None,party:None,instance:None}
}
#[test]
fn latest_valid_client_wins_and_clear_or_disconnect_only_removes_that_clients_activity() {
    let mut active=Activities::default();active.set(1,Some(activity("First")));active.set(2,Some(activity("Second")));
    assert_eq!(active.winner().unwrap().details,"Second");active.set(2,None);assert_eq!(active.winner().unwrap().details,"First");
    active.set(2,Some(activity("Replacement")));active.remove(1);assert_eq!(active.winner().unwrap().details,"Replacement");
    active.remove(2);assert!(active.winner().is_none());
    let (listener,updates)=Listener::fake();active.set(1,Some(activity("Visible")));active.publish(&updates,&listener.close);
    assert_eq!(listener.snapshot().activity.unwrap().details,"Visible");
    let revision=listener.snapshot().revision;active.publish(&updates,&listener.close);assert_eq!(listener.snapshot().revision,revision);
    listener.close();active.publish(&updates,&listener.close);assert!(!listener.snapshot().listening);assert!(listener.snapshot().activity.is_none());
}

async fn wait_activity(updates:&mut watch::Receiver<Snapshot>,present:bool) {
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {if updates.borrow().activity.is_some()==present {break;}updates.changed().await.unwrap();}
    }).await.unwrap();
}
#[tokio::test]
async fn private_pipe_skips_owned_indices_verifies_real_local_pid_and_clears_on_disconnect() {
    use tokio::net::windows::named_pipe::ClientOptions;
    let base=format!(r"\\.\pipe\NotDiscord.RpcFixture.{}",uuid::Uuid::new_v4().simple());
    let security=security::Context::current().unwrap();let original=security.pipe(&format!("{base}-0"),true).unwrap();
    let listener=start_under(identity(),&base).await.unwrap();let other=start_under(identity(),&base).await.unwrap();
    assert_eq!(listener.snapshot().pipe_index,Some(1));assert_eq!(other.snapshot().pipe_index,Some(2));
    assert!(security.pipe(&format!("{base}-0"),true).is_err(),"an owned endpoint was never replaced or attached");
    let mut updates=listener.subscribe();
    let mut client=ClientOptions::new().open(format!("{base}-1")).unwrap();handshake(&mut client).await;
    send(&mut client,1,json!({"cmd":"SET_ACTIVITY","nonce":"local","args":{"pid":std::process::id(),"activity":{"details":"Synthetic game"}}})).await;
    assert_eq!(receive_json(&mut client).await["nonce"],"local");wait_activity(&mut updates,true).await;
    let active=listener.snapshot().activity.unwrap();assert_eq!(active.pid,std::process::id());
    assert!(!active.executable.contains(['\\','/']),"only an ephemeral basename is retained");
    assert_eq!(active.details,"Synthetic game");drop(client);wait_activity(&mut updates,false).await;
    assert!(listener.snapshot().listening&&other.snapshot().listening);
    let mut partial=ClientOptions::new().open(format!("{base}-1")).unwrap();partial.write_all(&[0]).await.unwrap();
    listener.shutdown().await;assert!(!updates.borrow().listening);assert!(updates.borrow().activity.is_none());
    let ended=tokio::time::timeout(Duration::from_secs(2),protocol::read_frame(&mut partial)).await.unwrap();
    assert!(matches!(ended,Ok(None)|Err(_)),"shutdown also releases a half-written handshake");
    drop(partial);
    assert!(security.pipe(&format!("{base}-1"),true).is_ok(),"awaited shutdown releases ownership of every instance");
    other.shutdown().await;drop(original);
}
