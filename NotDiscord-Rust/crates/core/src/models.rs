use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
#[path = "shared_list.rs"]
mod shared_list;
pub use shared_list::SharedList;

pub type Timestamp = chrono::DateTime<chrono::FixedOffset>;
pub fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct NavItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub parent_id: String,
    pub icon_url: Option<String>,
    pub unread: bool,
    pub is_pinned: bool,
    pub last_message_id: String,
    pub message_preview: Option<String>,
    pub subtitle: String,
    pub presence: String,
    pub recipient_id: String,
    pub is_muted: bool,
    pub notifications_disabled: bool,
    pub guild_notifications: crate::notifications::GuildNotifications,
    pub can_invite: bool,
    pub can_manage: bool,
    pub mention_count: i32,
    #[serde(deserialize_with = "null_default")]
    pub recipient_avatars: Vec<String>,
    pub can_pin: bool,
    pub custom_status: String,
    pub custom_status_emoji: Option<StatusEmoji>,
    pub activity_text: String,
    #[serde(deserialize_with = "null_default")]
    pub activities: Vec<PresenceActivity>,
    pub has_active_call: bool,
    pub incoming_call_ringing: bool,
    pub access: Option<ChannelAccess>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct ChannelAccess {
    pub known: bool,
    pub can_view: bool,
    pub can_read_history: bool,
    pub can_send: bool,
    pub can_attach: bool,
    pub can_react: bool,
    pub can_view_members: bool,
    pub reason: String,
    pub can_embed: bool,
    pub can_mention_everyone: bool,
}
impl Default for ChannelAccess {
    fn default() -> Self {
        Self {
            known: false,
            can_view: false,
            can_read_history: false,
            can_send: false,
            can_attach: false,
            can_react: false,
            can_view_members: false,
            reason: "No se han podido confirmar los permisos del canal.".into(),
            can_embed: false,
            can_mention_everyone: false,
        }
    }
}
impl ChannelAccess {
    pub fn private() -> Self {
        Self {
            known: true,
            can_view: true,
            can_read_history: true,
            can_send: true,
            can_attach: true,
            can_react: true,
            can_view_members: true,
            reason: String::new(),
            can_embed: true,
            can_mention_everyone: false,
        }
    }
    pub fn allows(&self, capability: &str) -> bool {
        self.known
            && match capability {
                "view" => self.can_view,
                "history" => self.can_read_history,
                "send" => self.can_send,
                "attach" => self.can_attach,
                "react" => self.can_react,
                "members" => self.can_view_members,
                "embed" => self.can_embed,
                "mention-everyone" => self.can_mention_everyone,
                _ => false,
            }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct GuildRole {
    pub id: String,
    pub name: String,
    pub position: i32,
    pub color: i32,
    pub hoist: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct GuildMemberGroup {
    pub id: String,
    pub name: String,
    pub count: i32,
    pub color: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct MessageReaction {
    pub emoji: StatusEmoji,
    pub count: i32,
    pub me: bool,
    pub normal_me: Option<bool>,
    pub burst_me: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct Person {
    pub id: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub username: String,
    pub display_name: String,
    pub is_bot: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct StatusEmoji {
    pub name: String,
    pub id: String,
    pub animated: bool,
}
impl StatusEmoji {
    pub fn image_url(&self) -> Option<String> {
        (!self.id.is_empty()).then(|| {
            format!(
                "https://cdn.discordapp.com/emojis/{}.webp?size=32{}",
                self.id,
                if self.animated { "&animated=true" } else { "" }
            )
        })
    }
    pub fn reaction_key(&self) -> String {
        if self.id.is_empty() {
            self.name.clone()
        } else {
            format!("{}:{}", self.name, self.id)
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct PresenceActivity {
    #[serde(rename = "Type")]
    pub kind: i32,
    pub name: String,
    pub details: String,
    pub state: String,
    pub emoji: Option<StatusEmoji>,
    pub application_id: String,
    pub large_image_url: String,
    pub started_at: Option<Timestamp>,
    pub ends_at: Option<Timestamp>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct UserPresence {
    pub status: String,
    pub custom_status: String,
    pub custom_status_emoji: Option<StatusEmoji>,
    pub activities: Vec<PresenceActivity>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct ChannelMember {
    pub user: Person,
    pub presence: String,
    pub custom_status: String,
    pub custom_status_emoji: Option<StatusEmoji>,
    #[serde(deserialize_with = "null_default")]
    pub activities: Vec<PresenceActivity>,
    pub is_owner: bool,
    #[serde(deserialize_with = "null_default")]
    pub roles: Vec<GuildRole>,
    pub nickname: String,
    pub role_color: i32,
    pub hoist_role_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct ProfileDetails {
    pub user: Person,
    /// Profile-only CDN metadata. Kept out of people/member snapshots.
    pub banner_hash: String,
    pub banner_color: Option<u32>,
    pub guild_banner_hash: String,
    pub guild_banner_color: Option<u32>,
    pub is_webhook: bool,
    pub created_at: Option<Timestamp>,
    pub friends_since: Option<Timestamp>,
    pub bio: String,
    pub presence: String,
    pub custom_status: String,
    pub custom_status_emoji: Option<StatusEmoji>,
    pub activities: Vec<PresenceActivity>,
    pub mutual_friends: Vec<Person>,
    pub mutual_guilds: Vec<NavItem>,
    pub mutual_friends_available: bool,
    pub mutual_guilds_available: bool,
    pub guild_id: String,
    pub guild_name: String,
    #[serde(deserialize_with = "null_default")]
    pub guild_roles: Vec<GuildRole>,
    pub nickname: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct ChatAttachment {
    pub id: String,
    pub name: String,
    pub url: String,
    pub content_type: Option<String>,
    pub width: i32,
    pub height: i32,
    pub size: i64,
    pub animated: bool,
    pub preview_url: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct MessageReplyPreview {
    pub message_id: String,
    pub channel_id: String,
    pub author: Option<Person>,
    pub content: String,
    pub deleted: bool,
    pub unavailable: bool,
    #[serde(deserialize_with = "null_default")]
    pub mentions: Vec<Person>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct ChatSticker {
    pub id: String,
    pub name: String,
    pub format_type: i32,
}
impl ChatSticker {
    pub fn code(&self) -> String {
        let name = self.name.trim().trim_matches(':');
        if name.is_empty() {
            format!(":sticker_{}:", self.id)
        } else {
            format!(":{name}:")
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct ChatEmbedField {
    pub name: String,
    pub value: String,
    pub inline: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct ChatEmbed {
    pub title: String,
    pub description: String,
    pub url: String,
    pub author: String,
    pub author_url: String,
    pub provider: String,
    pub footer: String,
    pub color: i32,
    #[serde(deserialize_with = "null_default")]
    pub fields: Vec<ChatEmbedField>,
}
/// Bounded presentation of a received component tree. It contains no custom
/// interaction IDs/tokens and cannot submit bot commands.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct ChatComponent {
    pub kind: i32,
    pub text: String,
    pub url: String,
    pub depth: u8,
    pub spoiler: bool,
    pub color: i32,
    pub from_embed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct ChatMessage {
    pub id: String,
    pub channel_id: String,
    pub author: Person,
    pub content: String,
    pub timestamp: Timestamp,
    pub attachments: Vec<ChatAttachment>,
    #[serde(deserialize_with = "null_default")]
    pub embeds: Vec<ChatEmbed>,
    #[serde(deserialize_with = "null_default")]
    pub components: Vec<ChatComponent>,
    pub components_v2: bool,
    #[serde(deserialize_with = "null_default")]
    pub stickers: Vec<ChatSticker>,
    pub edited: bool,
    /// Bounded versions actually observed in this session; never inferred.
    #[serde(deserialize_with = "null_default")]
    pub previous_contents: Vec<String>,
    pub mentions_everyone: bool,
    pub deleted: bool,
    pub reply_to: Option<String>,
    pub message_type: i32,
    pub call_started_at: Option<Timestamp>,
    pub call_ended_at: Option<Timestamp>,
    pub call_outcome: String,
    pub call_participants_known: bool,
    #[serde(deserialize_with = "null_default")]
    pub call_participant_ids: Vec<String>,
    pub pinned: bool,
    pub forwarded: bool,
    pub webhook_id: String,
    #[serde(deserialize_with = "null_default")]
    pub reactions: Vec<MessageReaction>,
    #[serde(deserialize_with = "null_default")]
    pub mentions: Vec<Person>,
    pub reply_preview: Option<MessageReplyPreview>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct CallParticipant {
    pub user: Person,
    pub is_self: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub server_mute: bool,
    pub server_deaf: bool,
    pub speaking: bool,
    pub self_video: bool,
    pub screen_sharing: bool,
    pub volume_percent: f32,
}
impl Default for CallParticipant {
    fn default() -> Self { Self {user:Person::default(),is_self:false,self_mute:false,self_deaf:false,server_mute:false,server_deaf:false,speaking:false,self_video:false,screen_sharing:false,volume_percent:100.} }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct Relationship {
    pub user: Person,
    #[serde(rename = "Type")]
    pub kind: i32,
    pub since: Option<Timestamp>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct CustomEmoji {
    pub id: String,
    pub name: String,
    pub animated: bool,
    pub guild_id: String,
    /// Unknown legacy snapshots cannot prove that an emoji is sendable.
    pub usable: bool,
    #[serde(default = "emoji_available")]
    pub available: bool,
    pub roles: Vec<String>,
}
fn emoji_available() -> bool { true }
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct UiAction {
    pub kind: String,
    pub id: String,
    pub text: String,
    pub extra: String,
    pub channel_id: String,
}
impl UiAction {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            ..Self::default()
        }
    }
    pub fn id(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
            ..Self::default()
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct DesktopNotice {
    pub channel_id: String,
    pub guild_id: String,
    pub message_id: String,
    pub title: String,
    pub body: String,
    pub is_call: bool,
    pub avatar_url: Option<String>,
    pub image_path: Option<String>,
    pub notification_mentions: Option<crate::notifications::MentionFlags>,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct IncomingCall {
    pub channel_id: String,
    pub channel_name: String,
    pub kind: String,
    pub icon_url: Option<String>,
    pub caller: Option<Person>,
    pub started_at: Timestamp,
    pub participants: Vec<Person>,
    pub call_participants: Vec<CallParticipant>,
    pub instance_id: u64,
    pub muted: bool,
    pub ringing: bool,
    pub unread: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct CallVideo {
    pub view_requested: bool,
    pub id: u64,
    pub user_id: String,
    pub label: String,
    pub kind: String,
    pub local: bool,
    pub active: bool,
    pub stream_key: String,
    pub status: String,
    pub preview: bool,
    pub source_id: String,
    pub error: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageWindowMode {
    #[default]
    Latest,
    History,
    Jump,
}
/// The REST window, independent of locally retained deletion markers. Cursors
/// always refer to loaded server rows, so tombstones cannot skip pagination.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "PascalCase")]
pub struct MessageWindow {
    pub channel_id: String,
    pub mode: MessageWindowMode,
    pub oldest_id: String,
    pub newest_id: String,
    pub has_newer: bool,
    pub contiguous: bool,
    /// Changes only when message payloads change, not on a read/LRU touch.
    pub revision: u64,
}
impl Default for MessageWindow {
    fn default() -> Self {
        Self { channel_id: String::new(), mode: MessageWindowMode::Latest,
            oldest_id: String::new(), newest_id: String::new(), has_newer: false,
            contiguous: true, revision: 0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default, rename_all = "PascalCase")]
pub struct AppSnapshot {
    pub network_safety: crate::safety::SafetyState,
    pub network_safety_message: String,
    pub latest_messages_version: u64,
    pub signed_in: bool,
    pub demo: bool,
    pub connection: String,
    pub self_presence: String,
    pub notification_epoch: u64,
    pub notification_generation: u64,
    pub status: String,
    pub current_user: Option<Person>,
    pub selected_guild_id: String,
    pub selected_channel_id: String,
    pub channel_name: String,
    pub view: String,
    pub guilds: Vec<NavItem>,
    pub channels: Vec<NavItem>,
    pub direct_messages: Vec<NavItem>,
    pub messages: SharedList<ChatMessage>,
    pub message_window: MessageWindow,
    pub relationships: Vec<Relationship>,
    pub emojis: SharedList<CustomEmoji>,
    pub voice_available: bool,
    pub screen_available: bool,
    pub has_older_messages: bool,
    pub busy: bool,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub voice_connected: bool,
    pub voice_encryption_ready: bool,
    pub voice_connecting: bool,
    pub outgoing_call_ringing: bool,
    pub call_channel_name: String,
    pub call_channel_id: String,
    pub call_guild_id: String,
    pub call_instance_id: u64,
    pub call_status_label: String,
    pub voice_error: String,
    pub call_started_at: Option<Timestamp>,
    pub call_participants: Vec<CallParticipant>,
    pub call_pending_invitees: Vec<Person>,
    pub call_invite_candidates: Vec<Person>,
    pub call_invite_busy_ids: Vec<String>,
    pub call_videos: Vec<CallVideo>,
    #[serde(default = "default_stream_volume_percent")]
    pub stream_volume_percent: f32,
    pub call_move_version: u64,
    pub screen_sharing: bool,
    pub screen_paused: bool,
    pub screen_preparing: bool,
    pub camera_available: bool,
    pub camera_enabled: bool,
    pub camera_preparing: bool,
    pub unread_message_count: i32,
    pub new_message_notice: String,
    pub game_activity_enabled: bool,
    pub game_activity_available: bool,
    pub game_activity_configured: bool,
    pub game_activity_label: String,
    pub game_activity_running: bool,
    pub game_detection_note: String,
    pub custom_activity_label: String,
    pub typing_text: String,
    pub profile_user: Option<Person>,
    pub profile_subtitle: String,
    pub profile_details: Option<ProfileDetails>,
    pub selected_channel_kind: String,
    pub can_pin_messages: bool,
    pub pinned_messages: Vec<ChatMessage>,
    pub group_members: Vec<ChannelMember>,
    pub group_owner_id: String,
    pub group_member_removal_pending_ids: Vec<String>,
    pub group_recipient_candidates: Vec<Person>,
    pub user_presences: HashMap<String, UserPresence>,
    pub presence_version: u64,
    pub incoming_calls: Vec<IncomingCall>,
    pub channel_access: ChannelAccess,
    pub guild_members: SharedList<ChannelMember>,
    /// Original member-list protocol positions, parallel to `guild_members`.
    /// Headers and unloaded slots are deliberately not flattened away.
    pub guild_member_indices: SharedList<usize>,
    pub guild_member_list_bound: bool,
    pub guild_roles: Vec<GuildRole>,
    pub guild_member_groups: Vec<GuildMemberGroup>,
    pub guild_settings: Option<crate::guild_settings::GuildSettings>,
    pub member_moderation: Option<crate::member_moderation::MemberModeration>,
    pub guild_voice_members: Vec<crate::guild_settings::GuildVoiceMember>,
    pub guild_nicknames: HashMap<String, String>,
    pub search_people: Vec<Person>,
    pub guild_members_partial: bool,
    pub guild_members_note: String,
    pub member_context_version: u64,
    pub guild_members_has_more: bool,
    pub guild_members_total: i32,
    pub focus_message_id: String,
    pub focus_message_version: u64,
}
fn default_stream_volume_percent() -> f32 {100.}
impl AppSnapshot {
    pub fn guild_display_name<'a>(&'a self, person: &'a Person) -> &'a str {
        let nickname = if self.selected_guild_id.is_empty() { "" }
            else { self.guild_nicknames.get(&person.id).map(String::as_str).unwrap_or("") };
        crate::guild_settings::visible_name(person, nickname)
    }
    pub fn welcome() -> Self {
        Self {
            connection: "Sin conexión".into(),
            status: "Inicia sesión para conectar tu cuenta".into(),
            channel_name: "Inicio".into(),
            view: "welcome".into(),
            stream_volume_percent: 100.,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_emojis_use_webp_for_all_original_formats_and_request_animation_explicitly() {
        assert!(StatusEmoji::default().image_url().is_none());
        let mut emoji = StatusEmoji { id: "459361793035206666".into(), name: "MoniChibiSpin".into(), animated: false };
        assert_eq!(emoji.image_url().unwrap(), "https://cdn.discordapp.com/emojis/459361793035206666.webp?size=32");
        emoji.animated = true;
        assert_eq!(emoji.image_url().unwrap(), "https://cdn.discordapp.com/emojis/459361793035206666.webp?size=32&animated=true");
    }
    #[test]
    fn legacy_pascal_case_and_null_lists() {
        let m:ChatMessage=serde_json::from_str(r#"{"Id":"1","ChannelId":"2","Author":{"Id":"3","Name":"A"},"Timestamp":"2026-10-20T10:30:00-05:00","Attachments":[],"Mentions":null,"Reactions":null,"ReplyPreview":null}"#).unwrap();
        assert_eq!(m.id, "1");
        assert_eq!(m.timestamp.offset().local_minus_utc(), -18000);
        assert!(m.mentions.is_empty());
        let v = serde_json::to_value(m).unwrap();
        assert_eq!(v["ChannelId"], "2");
    }
    #[test]
    fn unknown_denies_private_allows() {
        assert!(!ChannelAccess::default().allows("send"));
        assert!(ChannelAccess::private().allows("attach"));
    }
    #[test]
    fn shared_snapshot_lists_preserve_wire_ids_and_isolate_message_and_member_edits() {
        let messages = vec![ChatMessage {
            id: "synthetic-message:1".into(),
            channel_id: "preview-channel".into(),
            content: "Original 日本語🙂".into(),
            ..Default::default()
        }];
        let members = vec![ChannelMember {
            user: Person {
                id: "preview-person".into(),
                name: "Original member".into(),
                ..Default::default()
            },
            ..Default::default()
        }];
        let mut current = AppSnapshot {
            messages: messages.clone().into(),
            guild_members: members.clone().into(),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_vec(&current.messages).unwrap(),
            serde_json::to_vec(&messages).unwrap()
        );
        assert_eq!(
            serde_json::to_vec(&current.guild_members).unwrap(),
            serde_json::to_vec(&members).unwrap()
        );
        let wire = serde_json::to_value(&current).unwrap();
        let decoded: AppSnapshot = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded, current);
        assert_eq!(wire["Messages"][0]["Id"], "synthetic-message:1");
        assert_eq!(wire["GuildMembers"][0]["User"]["Id"], "preview-person");
        let old = current.clone();
        current.messages[0].content = "Edited".into();
        current.guild_members[0].user.name = "Edited member".into();
        assert_eq!(old.messages[0].content, "Original 日本語🙂");
        assert_eq!(old.guild_members[0].user.name, "Original member");
        current.messages.clear();
        current.guild_members.clear();
        assert_eq!(old.messages.len(), 1);
        assert_eq!(old.guild_members.len(), 1);
        let defaults: AppSnapshot = serde_json::from_str("{}").unwrap();
        assert!(defaults.messages.is_empty() && defaults.guild_members.is_empty());
    }
}
