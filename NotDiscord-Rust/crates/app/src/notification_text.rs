//! Pure toast presentation. Names are supplied by the message's origin, never
//! implicitly taken from the currently selected server.
use std::collections::HashMap;
#[derive(Default)]
pub struct Names {pub users:HashMap<String,String>,pub roles:HashMap<String,String>,pub channels:HashMap<String,String>,pub games:HashMap<String,String>}
pub fn readable(source:&str,names:&Names)->String{
    let projected=notdiscord_core::message_atoms::project(source,|kind|match kind{
        notdiscord_core::message_atoms::Kind::Timestamp(stamp)=>crate::ui::timestamps::display(*stamp),
        notdiscord_core::message_atoms::Kind::Game(id)=>names.games.get(id).cloned().unwrap_or_else(||"@game · Juego no disponible".into()),
    });let source=projected.as_str();
    let mut out=String::with_capacity(source.len().min(512));let mut cursor=0;let mut code=0usize;
    while cursor<source.len(){
        let rest=&source[cursor..];let ch=rest.chars().next().unwrap();
        if ch=='\\'{out.push(ch);cursor+=1;if let Some(next)=source[cursor..].chars().next(){out.push(next);cursor+=next.len_utf8();}continue;}
        if ch=='`'{let run=rest.bytes().take_while(|b|*b==b'`').count();if code==0{code=run}else if code==run{code=0}out.push_str(&rest[..run]);cursor+=run;continue;}
        if code==0 && ch=='<' && let Some(end)=rest.find('>').filter(|end|*end<=128){
            let token=&rest[1..end];
            let mention=if let Some(id)=token.strip_prefix("@&"){Some((id,&names.roles,"@rol","@"))}
                else if let Some(id)=token.strip_prefix("@!").or_else(||token.strip_prefix('@')){Some((id,&names.users,"@usuario","@"))}
                else{token.strip_prefix('#').map(|id|(id,&names.channels,"#canal","#"))};
            if let Some((id,catalog,fallback,prefix))=mention && notdiscord_core::parsing::valid_id(id){
                if let Some(name)=catalog.get(id).filter(|name|!name.trim().is_empty()){
                    out.push_str(prefix);out.extend(name.chars().filter(|ch|!ch.is_control()).take(128));
                }else{out.push_str(fallback);}
                cursor+=end+1;continue;
            }
        }
        out.push(ch);cursor+=ch.len_utf8();
    }
    out
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn mentions_use_explicit_origin_and_keep_literal_code(){
        let mut names=Names::default();names.users.insert("123".into(),"Nick del origen".into());
        assert_eq!(readable("Hola <@!123> y <@456>",&names),"Hola @Nick del origen y @usuario");
        assert_eq!(readable(r"`<@123>` \<@123>",&names),r"`<@123>` \<@123>");
    }
}
