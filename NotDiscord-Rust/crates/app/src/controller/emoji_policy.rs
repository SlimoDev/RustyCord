use super::*;

impl Controller {
    pub(super) fn can_send_emoji(&self,emoji:&CustomEmoji)->bool {
        if !emoji.available || emoji.guild_id.is_empty() { return false; }
        let premium = matches!(self.premium,1..=3);
        if emoji.animated && !premium { return false; }
        if emoji.guild_id == self.state.selected_guild_id {
            return self.guild_acl.as_ref().is_some_and(|acl|
                acl.guild_id == emoji.guild_id && !acl.timed_out
                && (emoji.roles.is_empty() || emoji.roles.iter().any(|role|
                    role == &emoji.guild_id || acl.has_role(role))));
        }
        // Nitro, Classic and Basic permit external emoji, but cannot override
        // a destination channel's USE_EXTERNAL_EMOJIS permission or a source
        // emoji's role restrictions. A received image alone grants nothing.
        if !premium || !self.state.guilds.iter().any(|guild|guild.id == emoji.guild_id) { return false; }
        if !emoji.roles.is_empty() && !emoji.roles.iter().any(|role| role == &emoji.guild_id) {
            // The standard edition only retains validated member roles for
            // its current guild. Do not infer access in a different source
            // guild from an old emoji catalogue or issue a new request here.
            // Unknown external role restrictions remain denied.
            return false;
        }
        if self.state.selected_guild_id.is_empty() {
            return matches!(self.state.selected_channel_kind.as_str(),"dm"|"group");
        }
        let Some(acl) = self.guild_acl.as_ref().filter(|acl|
            acl.guild_id == self.state.selected_guild_id && !acl.timed_out) else { return false; };
        let Some(channel) = self.channels.get(&self.state.selected_channel_id) else { return false; };
        let effective = if matches!(i(channel,"type"),10..=12) {
            let Some(parent) = self.channels.get(&s(channel,"parent_id")) else { return false; };
            parent
        } else { channel };
        acl.channel_permissions(effective) & (1 << 18) != 0
    }
    pub(super) fn emoji_by_id_allowed(&self, id: &str) -> bool {
        self.state.emojis.iter().any(|emoji|emoji.id == id && self.can_send_emoji(emoji))
            || self.emoji_cache.values().filter(|(at,_)| at.elapsed() < Duration::from_secs(300))
                .flat_map(|(_,list)|list).any(|emoji|emoji.id == id && self.can_send_emoji(emoji))
    }
    pub(super) fn refresh_emoji_access(&mut self) {
        let mut emojis=std::mem::take(&mut self.state.emojis);
        for emoji in &mut emojis {emoji.usable=self.can_send_emoji(emoji);}
        self.state.emojis=emojis;
    }
    pub(super) fn store_emoji_catalog(&mut self,guild:&str,values:&[Value])->Vec<CustomEmoji> {
        let mut list=values.iter().take(500).map(|value|CustomEmoji {
            id:s(value,"id"),name:parsing::limited(&s(value,"name"),64),animated:b(value,"animated"),guild_id:guild.into(),
            available:value.get("available").is_none_or(|available|available.as_bool()==Some(true))
                && value.get("roles").is_none_or(Value::is_array),
            roles:items(&value["roles"]).iter().take(500).filter_map(Value::as_str).map(str::to_owned).collect(),
            usable:false,
        }).filter(|emoji|parsing::valid_id(&emoji.id)&&!emoji.name.is_empty()).collect::<Vec<_>>();
        for emoji in &mut list {emoji.usable=self.can_send_emoji(emoji);}
        if self.emoji_cache.len()>=3&&!self.emoji_cache.contains_key(guild) {
            if let Some(old)=self.emoji_cache.iter().min_by_key(|(_, (at,_))|*at).map(|(guild,_)|guild.clone()) {self.emoji_cache.remove(&old);}
        }
        self.emoji_cache.insert(guild.into(),(Instant::now(),list.clone()));
        if guild==self.state.selected_guild_id {self.state.emojis=list.clone();}
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn catalog_rechecks_roles_availability_entitlement_and_current_guild_at_send() {
        let (mut c,_)=Controller::test_fixture();c.state.current_user=Some(Person{id:"1".into(),..Default::default()});
        c.state.selected_guild_id="50".into();
        c.guild_raw=json!({"id":"50","roles":[{"id":"50","permissions":"3072"},{"id":"9","permissions":"0"}]});
        c.self_member=json!({"roles":["9"]});c.guild_acl=Some(GuildAccess::from_values(&c.guild_raw,&c.self_member,"1"));
        let list=c.store_emoji_catalog("50",&[json!({"id":"401","name":"Free"}),json!({"id":"402","name":"Locked","roles":["9"]}),
            json!({"id":"403","name":"Unavailable","available":false}),json!({"id":"404","name":"Animated","animated":true})]);
        assert_eq!(list.iter().map(|emoji|emoji.usable).collect::<Vec<_>>(),[true,true,false,false]);
        assert_eq!(c.checked_content("<:Locked:402>").unwrap(),"<:Locked:402>");
        c.dispatch("GUILD_MEMBER_UPDATE",json!({"guild_id":"50","user":{"id":"1"},"roles":[]})).await.unwrap();
        assert!(!c.state.emojis.iter().find(|emoji|emoji.id=="402").unwrap().usable);
        assert!(c.checked_content("<:Locked:402>").is_err());
        c.premium=2;c.refresh_emoji_access();assert!(c.state.emojis.iter().find(|emoji|emoji.id=="404").unwrap().usable);
        c.state.selected_guild_id="51".into();c.refresh_emoji_access();assert!(c.state.emojis.iter().all(|emoji|!emoji.usable));
        assert!(c.checked_content("<:Free:401>").is_err());assert!(c.api.is_none());
    }
    #[tokio::test]
    async fn gateway_unavailability_cannot_be_undone_by_a_late_emoji_catalog_reply() {
        let (mut c,_)=Controller::test_fixture();c.state.selected_guild_id="50".into();
        c.state.guilds=vec![NavItem{id:"50".into(),..Default::default()}];
        let pending=c.next_request("emojis");
        let catalogue=c.next_request("emoji-catalog/50");
        c.dispatch("GUILD_EMOJIS_UPDATE",json!({"guild_id":"50","emojis":[{"id":"401","name":"Paused","available":false}]})).await.unwrap();
        assert!(c.request_current("emojis",pending),"Gateway invalidates this guild's catalogue, not an unrelated picker's global request");
        let mut action=UiAction::id("__emojis","50");action.extra=pending.to_string();action.text=catalogue.to_string();
        c.complete(action,json!([{"id":"401","name":"Paused","available":true}])).await.unwrap();
        assert_eq!(c.state.emojis.len(),1);assert!(!c.state.emojis[0].available);assert!(!c.state.emojis[0].usable);assert!(c.api.is_none());
    }
}
