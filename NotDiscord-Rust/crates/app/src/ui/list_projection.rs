//! Reusable immutable projections. A repaint only compares source revisions;
//! grouping, ancestor walks and prefix heights are rebuilt when data changes.
use super::*;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct VoiceRows {
    pub rows: Vec<(usize, String)>,
    pub height: f32,
    pub games: usize,
}
#[derive(Clone)]
pub(super) struct NavigationProjection {
    account: String,
    guild: String,
    channels: Vec<NavItem>,
    voices: Vec<notdiscord_core::guild_settings::GuildVoiceMember>,
    members: notdiscord_core::models::SharedList<ChannelMember>,
    presence_version: u64,
    own_game: (bool, bool, bool, String, String),
    collapsed: HashSet<String>,
    pub hidden: HashSet<String>,
    pub voice: HashMap<String, VoiceRows>,
}
pub(super) fn navigation(ctx: &egui::Context, state: &AppSnapshot, collapsed: HashSet<String>) -> Arc<NavigationProjection> {
    let key=egui::Id::new("navigation-projection");
    let account=state.current_user.as_ref().map(|user|user.id.as_str()).unwrap_or("");
    let own_game=(state.game_activity_running,state.game_activity_configured,state.game_activity_enabled,
        state.self_presence.clone(),state.custom_activity_label.clone());
    if let Some(previous)=ctx.data(|data|data.get_temp::<Arc<NavigationProjection>>(key))
        && previous.account==account && previous.guild==state.selected_guild_id
        && previous.channels==state.channels && previous.voices==state.guild_voice_members
        && previous.members==state.guild_members && previous.presence_version==state.presence_version
        && previous.own_game==own_game && previous.collapsed==collapsed {return previous;}
    let index=state.channels.iter().map(|channel|(channel.id.as_str(),channel)).collect::<HashMap<_,_>>();
    let mut hidden=HashSet::new();
    for item in &state.channels {
        let mut parent=item.parent_id.as_str();
        for _ in 0..16 {
            if parent.is_empty(){break;}
            if collapsed.contains(parent){hidden.insert(item.id.clone());break;}
            let Some(ancestor)=index.get(parent) else {break;};
            parent=&ancestor.parent_id;
        }
    }
    let members=state.guild_members.iter().map(|member|(member.user.id.as_str(),member)).collect::<HashMap<_,_>>();
    let mut voice:HashMap<String,VoiceRows>=HashMap::new();
    for (position,member) in state.guild_voice_members.iter().enumerate() {
        if member.guild_id!=state.selected_guild_id || !index.contains_key(member.channel_id.as_str()){continue;}
        let cached=members.get(member.user.id.as_str());
        let live=state.user_presences.get(&member.user.id);
        let presence=live.map(|p|p.status.as_str()).or_else(||cached.map(|p|p.presence.as_str())).unwrap_or("");
        let activities=live.map(|p|p.activities.as_slice()).or_else(||cached.map(|p|p.activities.as_slice())).unwrap_or(&[]);
        let game=super::shell::member_game(state,&member.user.id,presence,activities);
        let rows=voice.entry(member.channel_id.clone()).or_insert_with(||VoiceRows{rows:Vec::new(),height:0.,games:0});
        rows.height+=if game.is_empty(){32.}else{48.};
        rows.games+=usize::from(!game.is_empty());
        rows.rows.push((position,game));
    }
    let projection=Arc::new(NavigationProjection{account:account.into(),guild:state.selected_guild_id.clone(),
        channels:state.channels.clone(),voices:state.guild_voice_members.clone(),members:state.guild_members.clone(),
        presence_version:state.presence_version,own_game,collapsed,hidden,voice});
    ctx.data_mut(|data|data.insert_temp(key,projection.clone()));
    projection
}

#[derive(Clone)]
pub(super) enum MemberRow {
    Section { label: String, color: i32 },
    Member(usize),
    Missing,
}
#[derive(Clone)]
pub(super) struct MemberProjection {
    account: String,
    guild: String,
    channel: String,
    revision: u64,
    bound: bool,
    total: i32,
    members: notdiscord_core::models::SharedList<ChannelMember>,
    private_members: Vec<ChannelMember>,
    indices: notdiscord_core::models::SharedList<usize>,
    roles: Vec<GuildRole>,
    groups: Vec<GuildMemberGroup>,
    pub rows: Vec<MemberRow>,
    pub prefix: Vec<f32>,
}
impl MemberProjection {
    pub fn visible(&self,min:f32,max:f32)->std::ops::Range<usize>{
        let start=self.prefix.partition_point(|height|*height<min).saturating_sub(1).min(self.rows.len());
        let end=self.prefix.partition_point(|height|*height<=max).min(self.rows.len());
        start..end.max(start)
    }
}
fn group(member:&ChannelMember)->&str {
    match member.presence.as_str(){
        "offline"|"invisible"=>"offline",
        "" if member.hoist_role_id.is_empty()=>"unknown",
        _ if member.hoist_role_id.is_empty()=>"online",
        _=>&member.hoist_role_id,
    }
}
fn section(state:&AppSnapshot,id:&str,count:usize)->MemberRow{
    let supplied=state.guild_member_groups.iter().find(|group|group.id==id);
    let role=state.guild_roles.iter().find(|role|role.id==id);
    let name=supplied.map(|group|group.name.as_str()).filter(|name|!name.is_empty())
        .or_else(||role.map(|role|role.name.as_str())).unwrap_or(match id{"online"=>"En línea","offline"=>"Desconectados",_=>"Estado no disponible"});
    MemberRow::Section{label:format!("{} — {}",name.to_uppercase(),count.max(supplied.map(|group|group.count.max(0) as usize).unwrap_or(0))),
        color:role.map(|role|role.color).or_else(||supplied.map(|group|group.color)).unwrap_or(0)}
}
pub(super) fn members(ctx:&egui::Context,state:&AppSnapshot)->Arc<MemberProjection>{
    let key=egui::Id::new("member-list-projection");
    let account=state.current_user.as_ref().map(|user|user.id.as_str()).unwrap_or("");
    if let Some(previous)=ctx.data(|data|data.get_temp::<Arc<MemberProjection>>(key))
        && previous.account==account && previous.guild==state.selected_guild_id && previous.channel==state.selected_channel_id
        && previous.revision==state.member_context_version && previous.bound==state.guild_member_list_bound
        && previous.total==state.guild_members_total && previous.members==state.guild_members
        && previous.private_members==state.group_members && previous.indices==state.guild_member_indices
        && previous.roles==state.guild_roles && previous.groups==state.guild_member_groups {return previous;}
    let mut rows=Vec::new();
    if state.selected_guild_id.is_empty(){rows.extend((0..state.group_members.len()).map(MemberRow::Member));}
    else if state.guild_member_list_bound {
        let grouped=state.guild_member_groups.iter().fold(0usize,|n,group|n.saturating_add(group.count.max(0) as usize).saturating_add(1));
        let known=state.guild_member_indices.iter().copied().max().map(|n|n+1).unwrap_or(0);
        let count=(if grouped>0{grouped}else{known.max(state.guild_members_total.max(0) as usize)}).min(2000);
        rows.resize(count,MemberRow::Missing);
        let mut slot=0usize;
        for group in &state.guild_member_groups {
            if slot>=rows.len(){break;}
            rows[slot]=section(state,&group.id,group.count.max(0) as usize);
            slot=slot.saturating_add(group.count.max(0) as usize).saturating_add(1);
        }
        for (member,slot) in state.guild_member_indices.iter().copied().enumerate(){
            if member<state.guild_members.len() && slot<rows.len(){rows[slot]=MemberRow::Member(member);}
        }
    } else {
        let mut grouped:HashMap<&str,Vec<usize>>=HashMap::new();
        for (index,member) in state.guild_members.iter().enumerate(){grouped.entry(group(member)).or_default().push(index);}
        let ranks=state.guild_roles.iter().enumerate().map(|(rank,role)|(role.id.as_str(),rank)).collect::<HashMap<_,_>>();
        let mut grouped=grouped.into_iter().collect::<Vec<_>>();
        grouped.sort_by_key(|(key,_)|match *key{"online"=>(1,0),"offline"=>(2,0),"unknown"=>(3,0),_=>(0,ranks.get(key).copied().unwrap_or(usize::MAX))});
        for (key,members) in grouped{rows.push(section(state,key,members.len()));rows.extend(members.into_iter().map(MemberRow::Member));}
    }
    let mut prefix=Vec::with_capacity(rows.len()+1);prefix.push(0.);
    for row in &rows{prefix.push(prefix.last().copied().unwrap_or(0.)+if matches!(row,MemberRow::Section{..}){38.}else{64.});}
    let projection=Arc::new(MemberProjection{account:account.into(),guild:state.selected_guild_id.clone(),channel:state.selected_channel_id.clone(),
        revision:state.member_context_version,bound:state.guild_member_list_bound,total:state.guild_members_total,members:state.guild_members.clone(),
        private_members:state.group_members.clone(),indices:state.guild_member_indices.clone(),roles:state.guild_roles.clone(),groups:state.guild_member_groups.clone(),rows,prefix});
    ctx.data_mut(|data|data.insert_temp(key,projection.clone()));projection
}
