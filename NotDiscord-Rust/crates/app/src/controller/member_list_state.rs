//! One reset path for list bindings, protocol slots and visible-list flags.
use super::*;

impl Controller {
    pub(super) fn reset_member_list(&mut self) {
        self.state.guild_members.clear();
        self.state.guild_member_indices.clear();
        self.state.guild_member_list_bound=false;
        self.guild_member_slots=Vec::new();
        self.member_list_id.clear();
        self.state.guild_member_groups=Vec::new();
        self.state.guild_members_total=0;
        self.state.guild_members_partial=false;
        self.state.guild_members_has_more=false;
        self.state.guild_members_note.clear();
        self.member_range_start=0;
        self.member_range=99;
    }
    pub(super) fn project_member_slots(&mut self) {
        let mut seen=HashSet::new();
        let mut members=Vec::new();
        let mut indices=Vec::new();
        for (index,member) in self.guild_member_slots.iter().enumerate(){
            if let Some(member)=member && seen.insert(member.user.id.clone()) {
                indices.push(index);members.push(member.clone());
            }
        }
        self.state.guild_members=members.into();
        self.state.guild_member_indices=indices.into();
        self.state.guild_member_list_bound=!self.member_list_id.is_empty();
    }
}
