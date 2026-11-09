//! Pure validated stream model. Native and Gateway use different field names;
//! neither projection may open a transport or decide capture/recovery policy.
use crate::protocol::{MediaError,Result};
use serde_json::{Value,json};
struct VideoStream { native:Value }
impl VideoStream {
    fn parse(value:&Value)->Result<Self>{
        let valid=value["type"]=="video" && value["rid"].as_str().is_some_and(|s|s.len()<=64&&!s.chars().any(char::is_control))
            && ["ssrc","rtxSsrc"].iter().all(|key|value[*key].as_u64().is_some_and(|n|n<=u32::MAX as u64))
            && value["active"].is_boolean();
        if !valid{return Err(MediaError::new("VIDEO_PARAMETERS_INVALID","La descripción del flujo de vídeo no es válida."));}
        Ok(Self{native:value.clone()})
    }
    fn gateway(&self,limits:bool,active:Option<bool>)->Value{
        let v=&self.native;
        let mut out=json!({"type":v["type"],"rid":v["rid"],"ssrc":v["ssrc"],"rtx_ssrc":v["rtxSsrc"],"quality":v["quality"],"active":active.map(Value::Bool).unwrap_or_else(||v["active"].clone())});
        if limits {out["max_bitrate"]=v["maxBitrate"].clone();out["max_framerate"]=v["maxFrameRate"].clone();out["max_resolution"]=v["maxResolution"].clone();}
        out
    }
}
pub(super) fn native_ready(audio:u64,soundshare:bool,streams:&[Value])->Result<Vec<Value>>{
    let mut output=vec![json!({"type":"audio","ssrc":audio,"rid":"","maxBitrate":64000,"soundshare":soundshare})];
    for stream in streams {output.push(VideoStream::parse(stream)?.native);}
    Ok(output)
}
pub(super) fn configured(streams:&[Value],width:u32,height:u32,fps:u32,bitrate:u32)->Result<Vec<Value>>{
    streams.iter().enumerate().map(|(i,v)|{
        let mut stream=VideoStream::parse(v)?;stream.native["active"]=json!(i==0);
        stream.native["maxResolution"]=json!({"type":"fixed","width":width,"height":height});
        stream.native["maxFrameRate"]=json!(fps);stream.native["maxBitrate"]=json!(bitrate);Ok(stream.native)
    }).collect()
}
pub(super) fn gateway(streams:&[Value],limits:bool,active:Option<bool>)->Result<Vec<Value>>{
    streams.iter().map(|v|Ok(VideoStream::parse(v)?.gateway(limits,active))).collect()
}
#[cfg(test)] mod tests {
    use super::*;
    fn stream()->Value{json!({"type":"video","rid":"100","ssrc":21,"rtxSsrc":22,"quality":100,"active":false})}
    #[test] fn ready_audio_and_camera_screen_stop_have_literal_independent_projections(){
        assert_eq!(native_ready(20,true,&[stream()]).unwrap(),vec![json!({"type":"audio","ssrc":20,"rid":"","maxBitrate":64000,"soundshare":true}),stream()]);
        let camera=configured(&[stream()],1280,720,30,2500000).unwrap();
        assert_eq!(camera,vec![json!({"type":"video","rid":"100","ssrc":21,"rtxSsrc":22,"quality":100,"active":true,"maxResolution":{"type":"fixed","width":1280,"height":720},"maxFrameRate":30,"maxBitrate":2500000})]);
        assert_eq!(gateway(&camera,true,None).unwrap(),vec![json!({"type":"video","rid":"100","ssrc":21,"rtx_ssrc":22,"quality":100,"active":true,"max_resolution":{"type":"fixed","width":1280,"height":720},"max_framerate":30,"max_bitrate":2500000})]);
        assert_eq!(gateway(&camera,false,Some(false)).unwrap(),vec![json!({"type":"video","rid":"100","ssrc":21,"rtx_ssrc":22,"quality":100,"active":false})]);
    }
    #[test] fn invalid_or_oversized_ssrc_never_produces_a_gateway_payload(){let mut v=stream();v["ssrc"]=json!(u64::MAX);assert!(gateway(&[v],true,None).is_err());}
}
