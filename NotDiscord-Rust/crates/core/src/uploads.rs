//! Upload entitlements from the user's existing session and selected guild.
//! Sources checked 2026-10-26: Discord API reference /uploading-files, User
//! premium types, File Attachments FAQ and Server Boosting. No probe uploads.
use serde::{Deserialize,Serialize};
pub const MIB:u64=1024*1024;
pub const BASE_FILE_BYTES:u64=20*MIB;
pub const MAX_ATTACHMENTS:usize=10;
// A bounded local send queue, independent of the per-file Discord entitlement.
pub const LOCAL_BATCH_BYTES:u64=1024*MIB;

#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq)]
#[serde(default,rename_all="PascalCase")]
pub struct UploadLimit { pub per_file:u64, pub premium_type:Option<i32>, pub guild_tier:Option<i32> }
impl Default for UploadLimit {
    fn default()->Self {Self {per_file:BASE_FILE_BYTES,premium_type:None,guild_tier:None}}
}
impl UploadLimit {
    pub fn from_entitlements(premium:Option<i32>,guild_tier:Option<i32>)->Self {
        let account=match premium {Some(1|3)=>50*MIB,Some(2)=>1024*MIB,_=>BASE_FILE_BYTES};
        let guild=match guild_tier {Some(2)=>50*MIB,Some(3)=>100*MIB,_=>BASE_FILE_BYTES};
        Self {per_file:account.max(guild),premium_type:premium,guild_tier}
    }
    pub fn label(&self)->String {
        let plan=match self.premium_type {Some(0)=>"Sin Nitro",Some(1)=>"Nitro Classic",Some(2)=>"Nitro",Some(3)=>"Nitro Basic",_=>"Suscripción sin confirmar"};
        format!("{} MiB por archivo · {plan}{}",self.per_file/MIB,
            self.guild_tier.filter(|tier|*tier>=2).map_or(String::new(),|tier|format!(" · servidor nivel {tier}")))
    }
    pub fn validate_file(&self,bytes:u64)->Result<(),String> {
        if bytes>self.per_file {Err(format!("El archivo supera el límite de {} MiB en este destino. El borrador se conserva.",self.per_file/MIB))}else{Ok(())}
    }
}
