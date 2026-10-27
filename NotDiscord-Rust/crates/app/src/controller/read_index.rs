//! Small account-generation-local catalogue for reconciling an ACK outside the
//! selected guild. Unknown permissions/catalogue coverage never imply read.
use super::*;
#[derive(Default)]
struct Guild {complete:bool,stamp:u64,channels:HashMap<String,Option<String>>}
#[derive(Default)]
pub(super) struct Index {guilds:HashMap<String,Guild>,clock:u64}
impl Index {
    pub(super) fn clear(&mut self){*self=Self::default();}
    pub(super) fn capture(&mut self,guild:&str,complete:bool,channels:Vec<(String,Option<String>)>){
        if !parsing::valid_id(guild){return;}
        self.clock=self.clock.wrapping_add(1);
        self.guilds.insert(guild.into(),Guild{complete,stamp:self.clock,channels:channels.into_iter().collect()});
        while self.guilds.len()>64 || self.guilds.values().map(|g|g.channels.len()).sum::<usize>()>4000{
            let Some(old)=self.guilds.iter().min_by_key(|(_,g)|g.stamp).map(|(id,_)|id.clone())else{break;};
            self.guilds.remove(&old);
        }
    }
    pub(super) fn guild_for(&self,channel:&str)->Option<String>{self.guilds.iter().find(|(_,g)|g.channels.contains_key(channel)).map(|(id,_)|id.clone())}
    pub(super) fn latest(&self,channel:&str)->Option<&str>{self.guilds.values().find_map(|guild|guild.channels.get(channel).and_then(Option::as_deref)).filter(|id|parsing::valid_id(id))}
    pub(super) fn observe(&mut self,kind:&str,value:&Value,me:&str){
        let guild=if kind.starts_with("GUILD_") && matches!(kind,"GUILD_CREATE"|"GUILD_UPDATE"|"GUILD_DELETE"){s(value,"id")}else{s(value,"guild_id")};
        if kind=="GUILD_DELETE"{self.guilds.remove(&guild);return;}
        if matches!(kind,"CHANNEL_CREATE"|"CHANNEL_UPDATE"|"CHANNEL_DELETE"|"THREAD_CREATE"|"THREAD_UPDATE"|"THREAD_DELETE"|"THREAD_LIST_SYNC"|"GUILD_CREATE"|"GUILD_UPDATE"|"GUILD_ROLE_CREATE"|"GUILD_ROLE_UPDATE"|"GUILD_ROLE_DELETE")
            || kind=="GUILD_MEMBER_UPDATE" && s(&value["user"],"id")==me{
            if let Some(entry)=self.guilds.get_mut(&guild){entry.complete=false;}
            if matches!(kind,"CHANNEL_DELETE"|"THREAD_DELETE") && let Some(entry)=self.guilds.get_mut(&guild){entry.channels.remove(&s(value,"id"));}
        }
        if kind=="MESSAGE_CREATE" && let Some(entry)=self.guilds.get_mut(&guild){
            let channel=s(value,"channel_id");let id=s(value,"id");
            // New/unknown channels invalidate coverage; do not manufacture
            // permission to clear the rest of this guild from this event.
            if !entry.channels.contains_key(&channel){entry.complete=false;return;}
            if s(&value["author"],"id")!=me && parsing::valid_id(&id){
                let last=entry.channels.get_mut(&channel).unwrap();
                if last.as_ref().is_none_or(|last|snowflake_greater(&id,last)){*last=Some(id);}
            }
        }
    }
    pub(super) fn unread(&self,guild:&str,read:&HashMap<String,String>,mentions:&HashMap<String,i32>,incoming:&HashMap<String,String>)->Option<bool>{
        let guild=self.guilds.get(guild)?;let mut known=guild.complete && !guild.channels.is_empty();
        for (channel,last) in &guild.channels{
            if mentions.get(channel).is_some_and(|count|*count>0){return Some(true);}
            let newest=match (last.as_deref(),incoming.get(channel).map(String::as_str)){
                (Some(a),Some(b))=>Some(if snowflake_greater(b,a){b}else{a}), (a,b)=>b.or(a),
            };
            match newest{
                Some("")=>{},Some(last) if !parsing::valid_id(last)=>{known=false;},Some(last)=>if let Some(watermark)=read.get(channel).filter(|id|parsing::valid_id(id)){
                    if snowflake_greater(last,watermark){return Some(true);}
                }else{known=false;},None=>known=false,
            }
        }
        known.then_some(false)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn another_guild_ack_requires_coverage_and_keeps_newer_arrivals(){
        let mut index=Index::default();index.capture("10",true,vec![("20".into(),Some("100".into()))]);
        let mut read=HashMap::from([("20".into(),"100".into())]);
        assert_eq!(index.unread("10",&read,&HashMap::new(),&HashMap::new()),Some(false));
        index.observe("MESSAGE_CREATE",&json!({"guild_id":"10","channel_id":"20","id":"101","author":{"id":"2"}}),"1");
        assert_eq!(index.unread("10",&read,&HashMap::new(),&HashMap::new()),Some(true));
        read.insert("20".into(),"101".into());
        index.observe("GUILD_ROLE_UPDATE",&json!({"guild_id":"10"}),"1");
        assert_eq!(index.unread("10",&read,&HashMap::new(),&HashMap::new()),None);
    }
}
