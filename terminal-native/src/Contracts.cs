namespace NotDiscord.Terminal;

public sealed record NavItem(string Id, string Name, string Kind, string ParentId = "", string? IconUrl = null, bool Unread = false,
    bool IsPinned = false, string LastMessageId = "", string Subtitle = "", string Presence = "", string RecipientId = "",
    bool IsMuted = false, bool CanInvite = false, bool CanManage = false, int MentionCount = 0, IReadOnlyList<string>? RecipientAvatars = null,
    bool CanPin = false, string CustomStatus = "", StatusEmoji? CustomStatusEmoji = null, string ActivityText = "", IReadOnlyList<PresenceActivity>? Activities = null,
    bool HasActiveCall = false, bool IncomingCallRinging = false, ChannelAccess? Access = null);
public sealed record ChannelAccess(bool Known, bool CanView, bool CanReadHistory, bool CanSend, bool CanAttach, bool CanReact, bool CanViewMembers, string Reason = "", bool CanEmbed = false)
{
    public static readonly ChannelAccess Unknown = new(false, false, false, false, false, false, false, "No se han podido confirmar los permisos del canal.");
    public static readonly ChannelAccess Private = new(true, true, true, true, true, true, true, CanEmbed: true);
}
public sealed record GuildRole(string Id, string Name, int Position, int Color, bool Hoist = false);
public sealed record GuildMemberGroup(string Id, string Name, int Count, int Color = 0);
public sealed record MessageReaction(StatusEmoji Emoji, int Count, bool Me, bool? NormalMe = null, bool BurstMe = false);
public sealed record Person(string Id, string Name, string? AvatarUrl = null, string Username = "", string DisplayName = "", bool IsBot = false);
public sealed record StatusEmoji(string Name, string Id = "", bool Animated = false)
{
    public string? ImageUrl => Id.Length > 0 ? $"https://cdn.discordapp.com/emojis/{Id}.{(Animated ? "gif" : "png")}?size=32" : null;
}
public sealed record PresenceActivity(int Type, string Name, string Details = "", string State = "", StatusEmoji? Emoji = null,
    string ApplicationId = "", string LargeImageUrl = "", DateTimeOffset? StartedAt = null, DateTimeOffset? EndsAt = null);
public sealed record UserPresence(string Status, string CustomStatus, StatusEmoji? CustomStatusEmoji, IReadOnlyList<PresenceActivity> Activities);
public sealed record ChannelMember(Person User, string Presence = "", string CustomStatus = "", StatusEmoji? CustomStatusEmoji = null,
    IReadOnlyList<PresenceActivity>? Activities = null, bool IsOwner = false, IReadOnlyList<GuildRole>? Roles = null, string Nickname = "", int RoleColor = 0, string HoistRoleId = "");
public sealed record ProfileDetails(Person User, DateTimeOffset? CreatedAt, DateTimeOffset? FriendsSince, string Bio,
    string Presence, string CustomStatus, StatusEmoji? CustomStatusEmoji, IReadOnlyList<PresenceActivity> Activities,
    IReadOnlyList<Person> MutualFriends, IReadOnlyList<NavItem> MutualGuilds, bool MutualFriendsAvailable = true, bool MutualGuildsAvailable = true,
    string GuildId = "", string GuildName = "", IReadOnlyList<GuildRole>? GuildRoles = null, string Nickname = "");
public sealed record ChatAttachment(string Id, string Name, string Url, string? ContentType = null, int Width = 0, int Height = 0, long Size = 0, bool Animated = false, string? PreviewUrl = null);
public sealed record MessageReplyPreview(string MessageId, string ChannelId, Person? Author, string Content,
    bool Deleted = false, bool Unavailable = false, IReadOnlyList<Person>? Mentions = null);
public sealed record ChatMessage(string Id, string ChannelId, Person Author, string Content, DateTimeOffset Timestamp, IReadOnlyList<ChatAttachment> Attachments, bool Edited = false, bool Deleted = false, string? ReplyTo = null,
    int MessageType = 0, DateTimeOffset? CallStartedAt = null, DateTimeOffset? CallEndedAt = null, IReadOnlyList<string>? CallParticipantIds = null,
    bool Pinned = false, bool Forwarded = false, string WebhookId = "", IReadOnlyList<MessageReaction>? Reactions = null,
    IReadOnlyList<Person>? Mentions = null, MessageReplyPreview? ReplyPreview = null);
public sealed record CallParticipant(Person User, bool IsSelf, bool SelfMute, bool SelfDeaf, bool Speaking, bool SelfVideo, bool ScreenSharing);
public sealed record Relationship(Person User, int Type, DateTimeOffset? Since = null);
public sealed record CustomEmoji(string Id, string Name, bool Animated, string GuildId);
public sealed record UiAction(string Kind, string Id = "", string Text = "", string Extra = "", string ChannelId = "");
public sealed record DesktopNotice(string ChannelId, string Title, string Body, bool IsCall = false, string? AvatarUrl = null);
public sealed record IncomingCall(string ChannelId, string ChannelName, string Kind, string? IconUrl, Person? Caller,
    DateTimeOffset StartedAt, IReadOnlyList<Person> Participants, bool Ringing, bool Unread);

public sealed record AppSnapshot
{
    public bool SignedIn { get; init; }
    public bool Demo { get; init; }
    public string Connection { get; init; } = "Sin conexión";
    public string SelfPresence { get; init; } = "";
    public string Status { get; init; } = "Inicia sesión para conectar tu cuenta";
    public Person? CurrentUser { get; init; }
    public string SelectedGuildId { get; init; } = "";
    public string SelectedChannelId { get; init; } = "";
    public string ChannelName { get; init; } = "Inicio";
    public string View { get; init; } = "welcome";
    public IReadOnlyList<NavItem> Guilds { get; init; } = Array.Empty<NavItem>();
    public IReadOnlyList<NavItem> Channels { get; init; } = Array.Empty<NavItem>();
    public IReadOnlyList<NavItem> DirectMessages { get; init; } = Array.Empty<NavItem>();
    public IReadOnlyList<ChatMessage> Messages { get; init; } = Array.Empty<ChatMessage>();
    public IReadOnlyList<Relationship> Relationships { get; init; } = Array.Empty<Relationship>();
    public IReadOnlyList<CustomEmoji> Emojis { get; init; } = Array.Empty<CustomEmoji>();
    public bool VoiceAvailable { get; init; }
    public bool ScreenAvailable { get; init; }
    public bool HasOlderMessages { get; init; }
    public bool Busy { get; init; }
    public bool SelfMute { get; init; }
    public bool SelfDeaf { get; init; }
    public bool VoiceConnected { get; init; }
    public bool VoiceEncryptionReady { get; init; }
    public bool VoiceConnecting { get; init; }
    public string CallChannelName { get; init; } = "";
    public string CallStatusLabel { get; init; } = "";
    public DateTimeOffset? CallStartedAt { get; init; }
    public IReadOnlyList<CallParticipant> CallParticipants { get; init; } = Array.Empty<CallParticipant>();
    public bool ScreenSharing { get; init; }
    public bool ScreenPaused { get; init; }
    public bool ScreenPreparing { get; init; }
    public bool CameraAvailable { get; init; }
    public bool CameraEnabled { get; init; }
    public bool CameraPreparing { get; init; }
    public int UnreadMessageCount { get; init; }
    public string NewMessageNotice { get; init; } = "";
    public bool GameActivityEnabled { get; init; }
    public bool GameActivityAvailable { get; init; }
    public bool GameActivityConfigured { get; init; }
    public string GameActivityLabel { get; init; } = "";
    public bool GameActivityRunning { get; init; }
    public string CustomActivityLabel { get; init; } = "";
    public string TypingText { get; init; } = "";
    public Person? ProfileUser { get; init; }
    public string ProfileSubtitle { get; init; } = "";
    public ProfileDetails? ProfileDetails { get; init; }
    public string SelectedChannelKind { get; init; } = "";
    public bool CanPinMessages { get; init; }
    public IReadOnlyList<ChatMessage> PinnedMessages { get; init; } = Array.Empty<ChatMessage>();
    public IReadOnlyList<ChannelMember> GroupMembers { get; init; } = Array.Empty<ChannelMember>();
    public IReadOnlyList<Person> GroupRecipientCandidates { get; init; } = Array.Empty<Person>();
    public IReadOnlyDictionary<string, UserPresence> UserPresences { get; init; } = new Dictionary<string, UserPresence>();
    public long PresenceVersion { get; init; }
    public IReadOnlyList<IncomingCall> IncomingCalls { get; init; } = Array.Empty<IncomingCall>();
    public ChannelAccess ChannelAccess { get; init; } = ChannelAccess.Unknown;
    public IReadOnlyList<ChannelMember> GuildMembers { get; init; } = Array.Empty<ChannelMember>();
    public IReadOnlyList<GuildRole> GuildRoles { get; init; } = Array.Empty<GuildRole>();
    public IReadOnlyList<GuildMemberGroup> GuildMemberGroups { get; init; } = Array.Empty<GuildMemberGroup>();
    public IReadOnlyList<Person> SearchPeople { get; init; } = Array.Empty<Person>();
    public bool GuildMembersPartial { get; init; }
    public bool GuildMembersHasMore { get; init; }
    public int GuildMembersTotal { get; init; }
    public string FocusMessageId { get; init; } = "";
    public long FocusMessageVersion { get; init; }
}

// UI events: login/logout, guild(Id), channel(Id), view(Id: friends/requests/history),
// send(Text, Extra: reply message id), edit(Id,Text), delete(Id), older, attach(Text: absolute path),
// react(Id,Text: emoji), pin-dm(Id), accept-friend(Id), remove-friend(Id), add-friend(Text),
// open-dm(Id), invite-preview(Text), invite-accept(Text), create-invite,
// search(Text), voice(Id), share, disconnect-voice, settings, status(Text).
// External mutations are triggered only by a corresponding explicit UI action.
