using System.Net.Http;
using System.Text.Json;
using System.Text.RegularExpressions;
using System.Security.Cryptography;
using System.Windows;
using System.Windows.Threading;

namespace NotDiscord.Terminal;

public sealed partial class Controller : IAsyncDisposable
{
    public event Action<DesktopNotice>? DesktopNoticeRequested;
    private readonly TerminalWindow window;
    private readonly string dataDirectory;
    private readonly NativeFeatures features;
    private readonly MessageWindowCache cache = new();
    private readonly DispatcherTimer redraw;
    private readonly Dictionary<string, string> drafts = new();
    private DiscordSession? session;
    private AppSnapshot state = new();
    private CancellationTokenSource channelLoad = new();
    private int navigationVersion;
    private int sessionGeneration, sidebarVersion, relationshipsVersion;
    private readonly SemaphoreSlim authenticationGate = new(1, 1);
    private CancellationTokenSource sessionWork = new();
    private bool sessionChanging;
    private PendingChannelLoad? pendingChannelLoad;
    private bool disposed;
    private string credentialPath => Path.Combine(dataDirectory, "session.dpapi");
    private string currentGuild = "";
    private string currentChannel = "";
    private int premiumType;
    private IReadOnlyList<CustomEmoji> guildEmojis = Array.Empty<CustomEmoji>();
    public Controller(TerminalWindow window, string dataDirectory)
    {
        this.window = window; this.dataDirectory = dataDirectory;
        features = new NativeFeatures(dataDirectory);
        redraw = new DispatcherTimer(TimeSpan.FromMilliseconds(60), DispatcherPriority.Background, (_, _) => { redraw!.Stop(); if (!disposed) window.Apply(state); }, window.Dispatcher);
        redraw.Stop();
        window.ActionRequested += OnAction;
        window.AttachController(this, dataDirectory);
        window.Apply(state);
    }

    private void Refresh() { if (!redraw.IsEnabled && !disposed) redraw.Start(); }
    private void Status(string text) { if (disposed) return; state = state with { Status = text, Busy = false }; Refresh(); }
    private bool IsCurrent(DiscordSession candidate, int generation) => !disposed && session == candidate && generation == sessionGeneration;
    private void ResetNavigation()
    {
        ++navigationVersion; pendingChannelLoad = null;
        state = state with { FocusMessageId = "" };
        ClearTyping();
        channelLoad.Cancel(); channelLoad.Dispose(); channelLoad = new();
    }
    private int BeginSessionTransition()
    {
        sessionChanging = true; ++sessionGeneration; ResetNavigation();
        sessionWork.Cancel(); sessionWork.Dispose(); sessionWork = new();
        return sessionGeneration;
    }
    private void ClearSessionView()
    {
        cache.Clear(); currentGuild = currentChannel = ""; premiumType = 0;
        ClearNavigationMetadata();
        ResetPickerAccount();
        guildEmojis = Array.Empty<CustomEmoji>(); drafts.Clear(); state = new AppSnapshot(); Refresh();
    }
    public async Task InitializeAsync()
    {
        var credential = SecretVault.Load(credentialPath);
        if (credential != null) await GuardAsync(() => ConnectAsync(credential));
    }

    private async void OnAction(UiAction action) => await GuardAsync(() => ActAsync(action));
    private async Task GuardAsync(Func<Task> work)
    {
        int generation = sessionGeneration;
        try { await work(); }
        catch (OperationCanceledException) { }
        catch (ServiceException e) { if (generation == sessionGeneration) Status(e.Message); }
        catch (Exception e) when (e is HttpRequestException or IOException or JsonException or InvalidOperationException or ArgumentException or UnauthorizedAccessException)
        { if (generation == sessionGeneration) Status("No se pudo completar la operación. No se ha registrado contenido privado en los diagnósticos."); }
    }

    private async Task ConnectAsync(string credential)
    {
        int generation = BeginSessionTransition();
        await authenticationGate.WaitAsync();
        try
        {
            if (disposed || generation != sessionGeneration) return;
            await StopGameActivityAsync();
            await StopMediaAsync(false);
            var previousSession = session; session = null;
            if (previousSession != null) await previousSession.DisposeAsync();
            if (gameActivity != null) { await gameActivity.DisposeAsync(); gameActivity = null; }
            if (disposed || generation != sessionGeneration) return;
            ClearSessionView(); File.Delete(credentialPath);
            var activeSession = new DiscordSession(credential); session = activeSession;
            Status("Comprobando sesión…");
            try
            {
                var me = await activeSession.GetAsync("users/@me", sessionWork.Token);
                if (!IsCurrent(activeSession, generation)) return;
                SecretVault.Save(credentialPath, credential);
                premiumType = Int(me, "premium_type");
                state = state with { CurrentUser = ParsePerson(me), SignedIn = true, View = "friends", Connection = "Conectando", Status = "Cargando conversaciones…", GameActivityEnabled = false };
                RememberPeople(new[] { state.CurrentUser });
                Refresh();
                activeSession.StatusChanged += status => window.Dispatcher.BeginInvoke(async () => await GuardAsync(() => ConnectionStatusAsync(activeSession, generation, status)));
                activeSession.Dispatch += (type, data) => window.Dispatcher.InvokeAsync(async () => { if (!IsCurrent(activeSession, generation)) return; await GuardAsync(() => HandleDispatchAsync(type, data)); }).Task.Unwrap();
                await activeSession.InitializePresenceAsync(sessionWork.Token);
                if (!IsCurrent(activeSession, generation)) return;
                state = state with { SelfPresence = activeSession.SelfPresence };
                activeSession.StartGateway();
                try { await ProbeMediaAsync(); }
                catch (Exception error) when (error is ServiceException or IOException or HttpRequestException)
                { if (IsCurrent(activeSession, generation)) state = state with { VoiceAvailable = false, ScreenAvailable = false }; }
                if (!IsCurrent(activeSession, generation)) return;
                sessionChanging = false;
                int restoreVersion = navigationVersion;
                await LoadSidebarAsync();
                if (IsCurrent(activeSession, generation) && currentChannel.Length == 0 && restoreVersion == navigationVersion) await RestoreNavigationAsync(activeSession, generation);
                if (IsCurrent(activeSession, generation)) Status("Cuenta conectada · interfaz nativa experimental");
            }
            catch (Exception error) when (error is ServiceException or HttpRequestException or IOException or JsonException or OperationCanceledException)
            {
                if (IsCurrent(activeSession, generation))
                {
                    Status(error is ServiceException service ? service.Message : "No se pudo completar la conexión. Puedes volver a iniciar sesión.");
                    if (!state.SignedIn) { session = null; await activeSession.DisposeAsync(); }
                }
            }
        }
        finally { if (generation == sessionGeneration) sessionChanging = false; authenticationGate.Release(); }
    }

    private async Task LoadSidebarAsync()
    {
        var active = session; int generation = sessionGeneration, version = ++sidebarVersion;
        if (active == null) return;
        var guilds = await active.GetAsync("users/@me/guilds?limit=200", sessionWork.Token);
        if (!IsCurrent(active, generation) || version != sidebarVersion) return;
        state = state with { Guilds = Items(guilds).Take(500).Select(ParseGuild).Select(g => g with { IsMuted = mutedGuilds.GetValueOrDefault(g.Id) }).ToArray() };
        Refresh();
        var dms = await active.GetAsync("users/@me/channels", sessionWork.Token);
        if (!IsCurrent(active, generation) || version != sidebarVersion) return;
        RememberRecipients(dms);
        var sidebarDms = Items(dms).Take(2000).Select(ParseChannel).ToArray();
        var sidebarIds = sidebarDms.Select(n => n.Id).ToHashSet(StringComparer.Ordinal);
        state = state with { DirectMessages = OrderDms(sidebarDms.Concat(state.DirectMessages.Where(n => !sidebarIds.Contains(n.Id) && latestObservedMessages.ContainsKey(n.Id)))).Take(2000).ToArray() };
        RefreshUnreadState();
        Refresh();
        await RefreshRelationshipsAsync();
    }

    private IEnumerable<NavItem> OrderDms(IEnumerable<NavItem> items) => NavigationOrder.DirectMessages(items.Select(DecorateDm));
    private async Task RefreshRelationshipsAsync()
    {
        var active = session; int generation = sessionGeneration, version = ++relationshipsVersion;
        if (active == null) return;
        var relations = await active.GetAsync("users/@me/relationships", sessionWork.Token);
        if (!IsCurrent(active, generation) || version != relationshipsVersion) return;
        state = state with { Relationships = Items(relations).Take(3000).Where(x => x.TryGetProperty("user", out _)).Select(ParseRelationship).ToArray() };
        RememberPeople(state.Relationships.Select(r => r.User));
        Refresh();
    }

    private async Task SelectGuildAsync(string id)
    {
        var active = session; int generation = sessionGeneration;
        if (active == null) return;
        ResetNavigation(); int version = navigationVersion;
        currentGuild = id; currentChannel = "";
        ClearGuildMemberView(); selectedChannelMetadata.Clear();
        guildEmojis = Array.Empty<CustomEmoji>();
        CancellationToken navigationToken = channelLoad.Token;
        ClearChannelSocialView();
        state = state with { SelectedGuildId = id, SelectedChannelId = "", Channels = Array.Empty<NavItem>(), Messages = Array.Empty<ChatMessage>(), Emojis = Array.Empty<CustomEmoji>(), View = "chat", ChannelName = "Selecciona un canal", Status = "Cargando canales…" };
        Refresh();
        var channels = await active.GetAsync($"guilds/{Snowflake(id)}/channels", navigationToken);
        if (!IsCurrent(active, generation) || version != navigationVersion) return;
        selectedGuildAccess = await FetchGuildAccessAsync(active, generation, id, navigationToken);
        if (!IsCurrent(active, generation) || version != navigationVersion) return;
        RememberSelectedChannelMetadata(channels);
        state = state with { Channels = Items(channels).Take(2000).Where(c => AccessFor(c, selectedGuildAccess).CanView).OrderBy(x => Int(x, "position")).Select(c => DecorateChannel(c, selectedGuildAccess)).ToArray(), Status = selectedGuildAccess == null ? ChannelAccess.Unknown.Reason : "Canales cargados", GuildRoles = selectedGuildAccess?.Roles ?? Array.Empty<GuildRole>(),
            Guilds = state.Guilds.Select(g => g.Id == id && selectedGuildAccess != null ? g with { CanInvite = (selectedGuildAccess.BasePermissions & 1) != 0, CanManage = (selectedGuildAccess.BasePermissions & 32) != 0 } : g).ToArray() };
        Refresh();
        await SubscribeVisibleGuildAsync(active, generation);
        if (!IsCurrent(active, generation) || version != navigationVersion) return;
        var emojis = await active.GetAsync($"guilds/{Snowflake(id)}/emojis", navigationToken);
        if (!IsCurrent(active, generation) || version != navigationVersion) return;
        guildEmojis = Items(emojis).Select(e => new CustomEmoji(Str(e, "id"), Str(e, "name"), Bool(e, "animated"), id)).ToArray();
        state = state with { Emojis = guildEmojis }; Refresh();
    }

    private async Task SelectChannelAsync(string id, bool connectVoice = true, string restoreMessageId = "")
    {
        var active = session; int generation = sessionGeneration;
        if (active == null) return;
        string restoreAnchor = restoreMessageId.Length > 0 ? Snowflake(restoreMessageId) : "";
        var item = state.Channels.Concat(state.DirectMessages).FirstOrDefault(x => x.Id == id);
        if (item == null) throw new ServiceException("El canal ya no está disponible en la navegación autorizada.");
        if (item.Kind is not ("dm" or "group") && !(item.Access ?? ChannelAccess.Unknown).CanView) throw new ServiceException("No tienes permiso para ver este canal.");
        if (item?.Kind == "category") return;
        if (connectVoice && (item?.Kind is "voice" or "stage")) { await TryHandleMediaActionAsync(new UiAction("voice", id)); return; }
        ResetNavigation();
        if (currentChannel != id) ClearGuildMemberView();
        currentChannel = id;
        if (item?.Kind is "dm" or "group") { currentGuild = ""; guildEmojis = Array.Empty<CustomEmoji>(); selectedGuildAccess = null; state = state with { Channels = Array.Empty<NavItem>(), GuildRoles = Array.Empty<GuildRole>() }; }
        var loading = new PendingChannelLoad(id, navigationVersion); pendingChannelLoad = loading;
        CancellationToken navigationToken = channelLoad.Token;
        ClearChannelSocialView();
        var initialMessages = restoreAnchor.Length > 0 && cache.Get(id, restoreAnchor) == null ? Array.Empty<ChatMessage>() : ReadContextMessages(id);
        state = state with { SelectedGuildId = currentGuild, SelectedChannelId = id, Emojis = guildEmojis, ChannelName = item?.Name ?? "Conversación", SelectedChannelKind = item?.Kind ?? "text", CanPinMessages = item?.Kind is "dm" or "group" || item?.CanPin == true, View = "chat", Messages = initialMessages, Status = "Cargando mensajes…", HasOlderMessages = false };
        RefreshGroupMemberView();
        ApplySelectedAccess();
        Refresh();
        if (!state.ChannelAccess.CanReadHistory) { cache.Replace(id, Array.Empty<ChatMessage>()); state = state with { Messages = Array.Empty<ChatMessage>(), Status = "Puedes ver el canal, pero no tienes permiso para consultar su historial." }; Refresh(); await SubscribeVisibleGuildAsync(active, generation); return; }
        try
        {
            string route = $"channels/{Snowflake(id)}/messages?limit=50";
            var messages = await active.GetAsync(route + (restoreAnchor.Length > 0 ? "&around=" + restoreAnchor : ""), navigationToken);
            if (!IsCurrent(active, generation) || loading.Version != navigationVersion || pendingChannelLoad != loading) return;
            var parsed = Items(messages).Select(ParseMessage).ToArray();
            bool missingAnchor = restoreAnchor.Length > 0 && !parsed.Any(m => m.Id == restoreAnchor);
            if (missingAnchor)
            {
                RequirePermission("history");
                messages = await active.GetAsync(route, navigationToken);
                if (!IsCurrent(active, generation) || loading.Version != navigationVersion || pendingChannelLoad != loading) return;
                parsed = Items(messages).Select(ParseMessage).ToArray();
            }
            RequirePermission("history");
            foreach (var message in parsed) RememberLatestMessage(id, message.Id);
            RememberPeople(parsed.Select(m => m.Author));
            // Keep the bounded older page while refreshing the latest messages, so
            // revisiting a chat can restore a viewport above the newest 50 items.
            // An explicit viewport restore uses its own page, including when the
            // original disappeared, so a stale cached anchor cannot reappear.
            cache.Replace(id, MergeLoadedMessages(restoreAnchor.Length > 0 ? parsed : cache.Read(id).Concat(parsed), loading));
            state = state with { Messages = ReadContextMessages(id), HasOlderMessages = parsed.Length == 50, Status = missingAnchor ? "La posición anterior ya no está disponible. Mostrando mensajes recientes." : "Mensajes cargados" };
            UpdateSearchPeople();
            SaveNavigation();
            Refresh();
            await SubscribeVisibleGuildAsync(active, generation);
            if (IsCurrent(active, generation) && loading.Version == navigationVersion && state.SelectedChannelKind == "group" && !groupRecipients.ContainsKey(id))
            {
                var group = await active.GetAsync($"channels/{Snowflake(id)}", navigationToken);
                if (IsCurrent(active, generation) && loading.Version == navigationVersion) { RememberChannelRecipients(group); RefreshGroupMemberView(); Refresh(); }
            }
        }
        finally { if (pendingChannelLoad == loading) pendingChannelLoad = null; }
    }

    private sealed record SavedNavigation(string AccountId, string GuildId, string ChannelId);
    private void SaveNavigation()
    {
        if (!state.SignedIn || state.CurrentUser == null || currentChannel.Length == 0) return;
        byte[] data = JsonSerializer.SerializeToUtf8Bytes(new SavedNavigation(state.CurrentUser.Id, currentGuild, currentChannel));
        if (data.Length > 4096) return;
        try { NativeFeatures.AtomicWrite(Path.Combine(dataDirectory, "navigation.json"), data); }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        { Status("Canal cargado; no se pudo guardar la vista para el próximo inicio."); }
    }
    private async Task RestoreNavigationAsync(DiscordSession active, int generation)
    {
        string path = Path.Combine(dataDirectory, "navigation.json");
        try
        {
            var file = new FileInfo(path);
            if (!file.Exists || file.Length is < 1 or > 4096) return;
            byte[] data;
            using (var stream = File.OpenRead(path))
            {
                if (stream.Length is < 1 or > 4096) return;
                data = new byte[(int)stream.Length]; stream.ReadExactly(data);
            }
            var saved = JsonSerializer.Deserialize<SavedNavigation>(data);
            if (saved == null || saved.AccountId != state.CurrentUser?.Id || !IsCurrent(active, generation)) return;
            if (string.IsNullOrEmpty(saved.ChannelId) || saved.GuildId is null) return;
            Snowflake(saved.ChannelId);
            int expectedNavigation = navigationVersion;
            if (saved.GuildId.Length > 0)
            {
                Snowflake(saved.GuildId);
                if (!state.Guilds.Any(g => g.Id == saved.GuildId)) return;
                expectedNavigation++;
                await SelectGuildAsync(saved.GuildId);
                if (!IsCurrent(active, generation) || expectedNavigation != navigationVersion || currentChannel.Length > 0) return;
                if (!state.Channels.Any(c => c.Id == saved.ChannelId)) return;
            }
            else if (!state.DirectMessages.Any(c => c.Id == saved.ChannelId)) return;
            if (IsCurrent(active, generation) && expectedNavigation == navigationVersion) await SelectChannelAsync(saved.ChannelId);
        }
        catch (Exception error) when (error is IOException or JsonException or UnauthorizedAccessException or ServiceException)
        { if (IsCurrent(active, generation)) Status("No se pudo restaurar la vista anterior. Puedes seleccionar otro canal."); }
    }

    private sealed class PendingChannelLoad(string channelId, int version)
    {
        public string ChannelId { get; } = channelId;
        public int Version { get; } = version;
        public long Bytes;
        public List<(string Type, JsonElement Data)> Deltas { get; } = new();
    }
    private void RememberLoadingDelta(string type, JsonElement data)
    {
        var loading = pendingChannelLoad;
        if (loading == null || Str(data, "channel_id") != loading.ChannelId) return;
        long bytes = data.GetRawText().Length * 2L;
        if (loading.Deltas.Count >= 256 || loading.Bytes + bytes > 1024 * 1024)
        {
            pendingChannelLoad = null; channelLoad.Cancel();
            Status("Hay demasiados cambios durante la carga. Los mensajes recibidos se conservan; vuelve a abrir el canal para cargar su historial.");
            return;
        }
        loading.Deltas.Add((type, data)); loading.Bytes += bytes;
    }
    private static ChatMessage ApplyMessageUpdate(ChatMessage previous, JsonElement data)
    {
        bool hasContent = data.TryGetProperty("content", out var content) && content.ValueKind == JsonValueKind.String;
        var files = data.TryGetProperty("attachments", out var attachments) ? ParseAttachments(attachments) : previous.Attachments.Where(a => !a.Id.StartsWith("embed-", StringComparison.Ordinal));
        var embeds = data.TryGetProperty("embeds", out var embedData) ? ParseEmbeds(embedData) : previous.Attachments.Where(a => a.Id.StartsWith("embed-", StringComparison.Ordinal));
        var updated = previous with { Content = hasContent ? content.GetString() ?? "" : previous.Content,
            Attachments = files.Concat(embeds).Take(50).ToArray(),
            Pinned = data.TryGetProperty("pinned", out var pinned) && pinned.ValueKind is JsonValueKind.True or JsonValueKind.False ? pinned.ValueKind == JsonValueKind.True : previous.Pinned,
            Edited = previous.Edited || (previous.MessageType != 3 && hasContent && content.GetString() != previous.Content) || Str(data, "edited_timestamp").Length > 0,
            Reactions = data.TryGetProperty("reactions", out var reactionData) ? ParseReactions(reactionData) : previous.Reactions,
            Mentions = data.TryGetProperty("mentions", out _) ? ParseMessageMentions(data) : previous.Mentions };
        updated = ApplyReplyUpdate(updated, data);
        return data.TryGetProperty("call", out var call) ? ApplyCallMetadata(updated, call) : updated;
    }
    private static IEnumerable<ChatMessage> MergeLoadedMessages(IEnumerable<ChatMessage> messages, PendingChannelLoad loading)
    {
        var merged = messages.GroupBy(m => m.Id).ToDictionary(g => g.Key, g => g.Last(), StringComparer.Ordinal);
        foreach (var (type, data) in loading.Deltas)
        {
            string id = Str(data, "id");
            if (type == "MESSAGE_CREATE") merged[id] = ParseMessage(data);
            else if (type == "MESSAGE_DELETE") merged.Remove(id);
            else if (type == "MESSAGE_UPDATE" && merged.TryGetValue(id, out var previous)) merged[id] = ApplyMessageUpdate(previous, data);
        }
        return merged.Values.OrderBy(m => m.Timestamp);
    }

    private string PrepareContent(string text) => features.PrepareOutgoing(text, id => premiumType > 0 || guildEmojis.Any(e => e.Id == id && !e.Animated && e.GuildId == currentGuild));

    private async Task ActAsync(UiAction action)
    {
        if (action.Kind == "login")
        {
            var dialog = new LoginWindow(dataDirectory) { Owner = window };
            if (dialog.ShowDialog() == true && dialog.Credential != null) await ConnectAsync(dialog.Credential);
            return;
        }
        if (action.Kind == "logout")
        {
            int logoutGeneration = BeginSessionTransition();
            await authenticationGate.WaitAsync();
            try
            {
                if (disposed || logoutGeneration != sessionGeneration) return;
                await StopGameActivityAsync();
                await StopMediaAsync(false);
                var oldSession = session; session = null;
                if (oldSession != null) await oldSession.DisposeAsync();
                if (disposed || logoutGeneration != sessionGeneration) return;
                File.Delete(credentialPath); ClearSessionView();
                Status("Sesión local cerrada. El historial local permanece cifrado.");
            }
            finally { if (logoutGeneration == sessionGeneration) sessionChanging = false; authenticationGate.Release(); }
            return;
        }
        if (action.Kind == "pin-dm")
        {
            features.TogglePin(Snowflake(action.Id));
            state = state with { DirectMessages = OrderDms(state.DirectMessages).ToArray() }; Refresh(); return;
        }
        var active = session; int generation = sessionGeneration;
        if (active == null || !state.SignedIn) { Status("Inicia sesión primero."); return; }
        if (sessionChanging) { Status("Espera a que termine el cambio de sesión."); return; }
        if (await TryHandleMediaActionAsync(action)) return;
        if (!IsCurrent(active, generation)) return;
        if (await TryHandleNavigationActionAsync(action, active, generation)) return;
        if (!IsCurrent(active, generation)) return;
        if (await TryHandleSocialActionAsync(action, active, generation)) return;
        if (!IsCurrent(active, generation)) return;
        string actionChannel = currentChannel;
        if (action.Kind is "send" or "send-prepared" or "edit" or "delete" or "react" or "react-toggle" or "attach" or "create-invite" or "older")
        {
            if (state.View != "chat" || actionChannel.Length == 0 || action.ChannelId != actionChannel || state.SelectedChannelId != actionChannel)
                throw new ServiceException("La vista cambió. Abre el canal y repite la acción; no se ha enviado nada.");
            if ((action.Kind is "edit" or "delete" or "react" or "react-toggle") && cache.Get(actionChannel, action.Id) == null)
                throw new ServiceException("El mensaje ya no está en la vista del canal. Vuelve a cargarlo antes de actuar.");
        }
        switch (action.Kind)
        {
            case "return-chat":
                state = state with { View = currentChannel.Length > 0 ? "chat" : "friends", Messages = ReadContextMessages(currentChannel), ProfileUser = null, ProfileDetails = null }; ApplySelectedAccess(); Refresh(); break;
            case "members-more": await LoadMoreMembersAsync(); break;
            case "guild":
                if (action.Id.Length == 0)
                {
                    ResetNavigation(); currentGuild = currentChannel = ""; guildEmojis = Array.Empty<CustomEmoji>();
                    ClearGuildMemberView(); selectedGuildAccess = null;
                    state = state with { SelectedGuildId = "", SelectedChannelId = "", Channels = Array.Empty<NavItem>(), Messages = Array.Empty<ChatMessage>(), Emojis = Array.Empty<CustomEmoji>(), View = "friends", ChannelName = "Amigos", ChannelAccess = ChannelAccess.Unknown, SearchPeople = Array.Empty<Person>(), GuildRoles = Array.Empty<GuildRole>() }; Refresh();
                }
                else await SelectGuildAsync(Snowflake(action.Id));
                break;
            case "channel": await SelectChannelAsync(Snowflake(action.Id), restoreMessageId: action.Extra); break;
            case "latest-messages":
                if (action.ChannelId != currentChannel || state.View != "chat") throw new ServiceException("Abre el chat para volver a sus mensajes recientes.");
                RequirePermission("view"); RequirePermission("history");
                await SelectChannelAsync(currentChannel, connectVoice: false); break;
            case "view":
                ResetNavigation();
                int viewVersion = navigationVersion;
                if (action.Id == "history" && currentChannel.Length > 0)
                {
                    string historyChannel = currentChannel;
                    var history = await features.ReadHistoryAsync(historyChannel, ct: sessionWork.Token);
                    if (!IsCurrent(active, generation) || viewVersion != navigationVersion || historyChannel != currentChannel) return;
                    state = state with { View = "history", Messages = history, Status = "Historial local de cambios observados por este cliente" };
                }
                else state = state with { View = action.Id, Status = action.Id == "requests" ? "Solicitudes de amistad entrantes y salientes" : "Amigos y conversaciones" };
                Refresh(); break;
            case "send":
                RequirePermission("send");
                if (string.IsNullOrWhiteSpace(action.Text)) return;
                if (action.Text.Length > 4000) throw new ServiceException("El mensaje supera el límite local de 4000 caracteres.");
                var body = new Dictionary<string, object?> { ["content"] = PrepareContent(action.Text), ["nonce"] = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds().ToString(), ["enforce_nonce"] = true, ["allowed_mentions"] = new { parse = new[] { "users", "roles" }, replied_user = false } };
                if (action.Extra.Length > 0) body["message_reference"] = new { message_id = Snowflake(action.Extra), channel_id = actionChannel, fail_if_not_exists = true };
                string sentTo = actionChannel;
                var sent = await active.SendAsync(HttpMethod.Post, $"channels/{Snowflake(sentTo)}/messages", body, sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                cache.Upsert(ParseMessage(sent)); UpdateMessages(); if (currentChannel == sentTo) window.ClearComposerIfMatches(action.Text, action.Extra); Status("Mensaje enviado"); break;
            case "send-prepared":
                await SendPreparedAsync(action, active, generation); break;
            case "edit":
                RequirePermission("send");
                if (cache.Get(actionChannel, action.Id)?.Forwarded == true) throw new ServiceException("Los mensajes reenviados no se pueden editar.");
                await active.SendAsync(HttpMethod.Patch, $"channels/{Snowflake(actionChannel)}/messages/{Snowflake(action.Id)}", new { content = PrepareContent(action.Text) }, sessionWork.Token);
                if (IsCurrent(active, generation)) Status("Mensaje editado"); break;
            case "delete":
                await active.SendAsync(HttpMethod.Delete, $"channels/{Snowflake(actionChannel)}/messages/{Snowflake(action.Id)}", ct: sessionWork.Token);
                if (IsCurrent(active, generation)) Status("Mensaje eliminado"); break;
            case "react":
            case "react-toggle":
                await ToggleReactionAsync(action, active, generation, action.Kind == "react-toggle"); break;
            case "attach":
                RequirePermission("send"); RequirePermission("attach");
                var uploaded = await active.UploadAsync(actionChannel, action.Text, PrepareContent(action.Extra), sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                cache.Upsert(ParseMessage(uploaded)); UpdateMessages(); Status("Adjunto enviado"); break;
            case "older":
                RequirePermission("history");
                if (currentChannel.Length == 0 || state.Messages.Count == 0) return;
                string before = state.Messages.First().Id, target = currentChannel;
                ResetNavigation();
                var olderLoad = new PendingChannelLoad(target, navigationVersion); pendingChannelLoad = olderLoad;
                var retainedPage = cache.Read(target).Take(50).ToArray();
                try
                {
                    var older = await active.GetAsync($"channels/{Snowflake(target)}/messages?limit=50&before={Snowflake(before)}", channelLoad.Token);
                    if (!IsCurrent(active, generation) || pendingChannelLoad != olderLoad || olderLoad.Version != navigationVersion) return;
                    var old = Items(older).Select(ParseMessage).OrderBy(m => m.Timestamp).ToArray();
                    cache.Replace(target, MergeLoadedMessages(old.Concat(retainedPage), olderLoad));
                    UpdateMessages(); state = state with { HasOlderMessages = old.Length == 50 }; Refresh();
                }
                finally { if (pendingChannelLoad == olderLoad) pendingChannelLoad = null; }
                break;
            case "accept-friend":
                await active.SendAsync(HttpMethod.Put, $"users/@me/relationships/{Snowflake(action.Id)}", new { }, sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                await RefreshRelationshipsAsync(); if (IsCurrent(active, generation)) Status("Solicitud aceptada"); break;
            case "remove-friend":
                await active.SendAsync(HttpMethod.Delete, $"users/@me/relationships/{Snowflake(action.Id)}", ct: sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                await RefreshRelationshipsAsync(); if (IsCurrent(active, generation)) Status("Relación actualizada"); break;
            case "add-friend":
                await AddFriendAsync(action, active, generation); break;
            case "open-dm":
                var dm = ParseChannel(await active.SendAsync(HttpMethod.Post, "users/@me/channels", new { recipients = new[] { Snowflake(action.Id) } }, sessionWork.Token));
                if (!IsCurrent(active, generation)) return;
                state = state with { DirectMessages = OrderDms(state.DirectMessages.Where(n => n.Id != dm.Id).Append(dm)).ToArray() }; await SelectChannelAsync(dm.Id); break;
            case "invite-preview":
                string code = InviteCode(action.Text);
                var invite = await active.GetAsync($"invites/{code}?with_counts=true", sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                string guildName = invite.TryGetProperty("guild", out var guild) ? Str(guild, "name") : "servidor";
                if (MessageBox.Show(window, "¿Unirte a «" + guildName + "» mediante esta invitación?", "NotDiscord · Invitación", MessageBoxButton.YesNo, MessageBoxImage.Question) == MessageBoxResult.Yes)
                {
                    if (!IsCurrent(active, generation)) return;
                    await active.SendAsync(HttpMethod.Post, $"invites/{code}", new { }, sessionWork.Token);
                    if (!IsCurrent(active, generation)) return;
                    await LoadSidebarAsync(); if (IsCurrent(active, generation)) Status("Invitación aceptada");
                }
                break;
            case "invite-accept":
                // Acceptance is intentionally routed through preview and explicit confirmation.
                await ActAsync(action with { Kind = "invite-preview" }); break;
            case "create-invite":
                var created = await active.SendAsync(HttpMethod.Post, $"channels/{Snowflake(actionChannel)}/invites", new { max_age = 86400, max_uses = 0, temporary = false, unique = false }, sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                Clipboard.SetText("https://discord.gg/" + Str(created, "code")); Status("Invitación copiada · caduca en 24 h"); break;
            case "search":
                RequirePermission("history"); UpdateSearchPeople();
                string scope = currentGuild.Length > 0 ? "guilds/" + Snowflake(currentGuild) : "channels/" + Snowflake(currentChannel);
                ParsedSearch parsedSearch;
                try { parsedSearch = SearchQuery.Parse(action.Text, new SearchContext(state.CurrentUser?.Id ?? "", state.SearchPeople, (currentGuild.Length > 0 ? state.Channels.Where(c => c.Access?.CanView == true) : Enumerable.Empty<NavItem>()).Concat(state.DirectMessages.Where(c => c.Id == currentChannel)).ToArray())); }
                catch (ArgumentException error) { throw new ServiceException(error.Message); }
                if (parsedSearch.Filters.Any(f => f.Key is "author_id" or "mentions" && !state.SearchPeople.Any(p => p.Id == f.Value))) throw new ServiceException("Selecciona una persona del contexto visible de esta conversación.");
                if (parsedSearch.Filters.Any(f => f.Key == "channel_id" && (currentGuild.Length == 0 || !state.Channels.Any(c => c.Id == f.Value && c.Access?.CanView == true)) && f.Value != currentChannel)) throw new ServiceException("El canal del filtro no está en tu lista de canales visibles.");
                if (parsedSearch.AuthorType.Length > 0 && parsedSearch.Filters.Count == 0 && parsedSearch.Content.Length == 0)
                {
                    ResetNavigation();
                    var localAuthorMatches = cache.Read(currentChannel).Where(parsedSearch.Matches).Take(100).ToArray();
                    state = state with { View = "search", Messages = localAuthorMatches, Status = localAuthorMatches.Length + " resultados · tipo de autor filtrado sólo en los mensajes cargados de este canal" }; Refresh(); break;
                }
                ResetNavigation(); int searchVersion = navigationVersion;
                var search = await active.GetAsync(scope + "/messages/search?" + parsedSearch.QueryString, channelLoad.Token);
                if (!IsCurrent(active, generation) || searchVersion != navigationVersion) return;
                if (search.TryGetProperty("indexed", out var indexed) && indexed.ValueKind == JsonValueKind.False) { Status("Discord está preparando el índice de búsqueda. Vuelve a intentarlo cuando termine."); break; }
                var found = search.TryGetProperty("messages", out var results) ? Items(results).SelectMany(Items).Where(m => Bool(m, "hit")).Select(ParseMessage).Where(parsedSearch.Matches).Take(100).ToArray() : Array.Empty<ChatMessage>();
                state = state with { View = "search", Messages = found, Status = found.Length + " resultados" + (parsedSearch.AuthorType.Length > 0 ? " · tipo de autor filtrado en esta página de resultados" : "") + " · abre el canal para actuar sobre ellos" }; Refresh(); break;
            case "status":
                await active.SetPresenceAsync(action.Text);
                if (!IsCurrent(active, generation)) return;
                state = state with { SelfPresence = active.SelfPresence }; SyncActivityState();
                if (IsCurrent(active, generation)) Status("Estado de presencia enviado al Gateway."); break;
        }
    }

    private async Task HandleDispatchAsync(string type, JsonElement data)
    {
        var active = session; int generation = sessionGeneration;
        if (active == null) return;
        ObserveIncomingCallDispatch(type, data, active, generation);
        if (HandleReactionDispatch(type, data)) return;
        if (await HandleAccessDispatchAsync(type, data)) return;
        if (await HandleMediaDispatchAsync(type, data)) return;
        if (!IsCurrent(active, generation)) return;
        if (await HandlePresenceDispatchAsync(type, data)) return;
        if (!IsCurrent(active, generation)) return;
        if (await HandleSocialDispatchAsync(type, data)) return;
        if (!IsCurrent(active, generation)) return;
        if (type is "MESSAGE_CREATE" or "MESSAGE_UPDATE" or "MESSAGE_DELETE") RememberLoadingDelta(type, data);
        if (type == "MESSAGE_CREATE")
        {
            string channel = Str(data, "channel_id");
            if (Str(data, "guild_id") == currentGuild && currentGuild.Length > 0 && selectedChannelMetadata.TryGetValue(channel, out var knownChannel) && !AccessFor(knownChannel, selectedGuildAccess).CanView) return;
            NoteIncomingMessage(data);
            if (Str(data, "guild_id").Length == 0 && !state.DirectMessages.Any(n => n.Id == channel))
                _ = GuardAsync(() => EnsureIncomingPrivateChannelAsync(channel, active, generation));
            if (data.TryGetProperty("author", out var user)) { var author = ParsePerson(user); RememberPeople(new[] { author }); if (channel == currentChannel && typingPeople.Remove(author.Id)) UpdateTypingText(); }
            string messageId = Str(data, "id");
            bool updateDmOrder = state.DirectMessages.Any(n => n.Id == channel && SnowflakeOrder(messageId) > SnowflakeOrder(n.LastMessageId));
            bool updateChannelLast = state.Channels.Any(n => n.Id == channel && SnowflakeOrder(messageId) > SnowflakeOrder(n.LastMessageId));
            if (updateDmOrder) state = state with { DirectMessages = OrderDms(state.DirectMessages.Select(n => n.Id == channel ? n with { LastMessageId = messageId } : n)).ToArray() };
            if (updateChannelLast) state = state with { Channels = state.Channels.Select(n => n.Id == channel ? n with { LastMessageId = messageId } : n).ToArray() };
            if (updateDmOrder) Refresh();
            if ((channel != currentChannel || SelectedAccess().CanView) && (channel == currentChannel || cache.ContainsChannel(channel))) { cache.Upsert(ParseMessage(data)); UpdateMessages(); }
        }
        else if (type is "MESSAGE_UPDATE" or "MESSAGE_DELETE")
        {
            string id = Str(data, "id"), channel = Str(data, "channel_id");
            var previous = cache.Get(channel, id);
            if (previous == null) return;
            ChatMessage? updated = type == "MESSAGE_DELETE" ? null : ApplyMessageUpdate(previous, data);
            if (updated == null) { cache.Remove(channel, id); MarkReplyReferencesDeleted(channel, id); } else cache.Upsert(updated);
            UpdateMessages();
            bool contentChanged = previous.MessageType != 3 && data.TryGetProperty("content", out var changedContent) && changedContent.ValueKind == JsonValueKind.String && changedContent.GetString() != previous.Content;
            bool filesChanged = updated != null && !previous.Attachments.Where(a => !a.Id.StartsWith("embed-", StringComparison.Ordinal)).Select(a => a.Id).SequenceEqual(updated.Attachments.Where(a => !a.Id.StartsWith("embed-", StringComparison.Ordinal)).Select(a => a.Id));
            try { if (type == "MESSAGE_DELETE" || contentChanged || filesChanged) await features.RecordChangeAsync(previous, updated); }
            catch (Exception error) when (error is IOException or CryptographicException or UnauthorizedAccessException)
            { if (IsCurrent(active, generation)) Status("El mensaje se actualizó, pero no pudo guardarse su versión anterior. Comprueba el límite y espacio del historial local."); }
        }
        else if (type is "RELATIONSHIP_ADD" or "RELATIONSHIP_UPDATE" or "RELATIONSHIP_REMOVE") { await RefreshRelationshipsAsync(); RefreshGroupMemberView(); }
        else if (type is "CHANNEL_CREATE" or "CHANNEL_UPDATE" && Int(data, "type") is 1 or 3)
        {
            RememberChannelRecipients(data);
            var dm = ParseChannel(data); state = state with { DirectMessages = OrderDms(state.DirectMessages.Where(n => n.Id != dm.Id).Append(dm)).ToArray() }; Refresh();
        }
        else if (type == "CHANNEL_DELETE")
        {
            string id = Str(data, "id");
            state = state with { Channels = state.Channels.Where(n => n.Id != id).ToArray(), DirectMessages = state.DirectMessages.Where(n => n.Id != id).ToArray() };
            if (currentChannel == id) { ResetNavigation(); currentChannel = ""; state = state with { SelectedChannelId = "", Messages = Array.Empty<ChatMessage>(), ChannelName = "Canal no disponible" }; }
            Refresh();
        }
    }

    private void UpdateMessages()
    {
        if (state.View == "chat") { state = state with { Messages = SelectedAccess().CanView ? ReadContextMessages(currentChannel) : Array.Empty<ChatMessage>() }; UpdateSearchPeople(); Refresh(); }
    }

    public static Person ParsePerson(JsonElement user)
    {
        string id = Str(user, "id"), avatar = Str(user, "avatar");
        string name = Str(user, "global_name"); if (name.Length == 0) name = Str(user, "username");
        string? avatarUrl = avatar.Length > 0 ? $"https://cdn.discordapp.com/avatars/{id}/{avatar}.png?size=64" : null;
        if (avatarUrl == null && ulong.TryParse(id, out ulong snowflake))
        {
            int index = int.TryParse(Str(user, "discriminator"), out int discriminator) && discriminator > 0 ? discriminator % 5 : (int)((snowflake >> 22) % 6);
            avatarUrl = $"https://cdn.discordapp.com/embed/avatars/{index}.png";
        }
        return new(id, name.Length > 0 ? name : "Usuario", avatarUrl, Str(user, "username"), Str(user, "global_name"), Bool(user, "bot"));
    }
    private static string? GuildIcon(JsonElement guild) => Str(guild, "icon") is { Length: > 0 } hash ? $"https://cdn.discordapp.com/icons/{Str(guild, "id")}/{hash}.png?size=64" : null;
    public static NavItem ParseChannel(JsonElement channel)
    {
        int type = Int(channel, "type"); string name = Str(channel, "name"); string? icon = null; string recipientId = "";
        string[] avatars = Array.Empty<string>();
        if (channel.TryGetProperty("recipients", out var recipients))
        {
            var people = Items(recipients).Take(100).Select(ParsePerson).ToArray();
            if (name.Length == 0) name = string.Join(", ", people.Take(6).Select(p => p.Name));
            avatars = people.Select(p => p.AvatarUrl).Where(url => url != null).Cast<string>().Take(4).ToArray();
            if (type == 1) { recipientId = people.FirstOrDefault()?.Id ?? ""; icon = people.FirstOrDefault()?.AvatarUrl; }
        }
        if (type == 3 && Str(channel, "icon") is { Length: > 0 } groupIcon) icon = $"https://cdn.discordapp.com/channel-icons/{Str(channel, "id")}/{groupIcon}.png?size=64";
        return new(Str(channel, "id"), name.Length == 0 ? "Conversación" : name, type switch { 1 => "dm", 3 => "group", 2 => "voice", 13 => "stage", 4 => "category", 15 => "forum", 10 or 11 or 12 => "thread", _ => "text" }, Str(channel, "parent_id"), icon, LastMessageId: Str(channel, "last_message_id"), RecipientId: recipientId, RecipientAvatars: avatars);
    }
    public static ChatMessage ParseMessage(JsonElement message)
    {
        var author = message.TryGetProperty("author", out var a) ? ParsePerson(a) : new Person("", "Usuario");
        DateTimeOffset.TryParse(Str(message, "timestamp"), out var timestamp);
        bool forwarded = message.TryGetProperty("message_reference", out var reference) && Int(reference, "type") == 1;
        JsonElement display = message;
        if (forwarded && message.TryGetProperty("message_snapshots", out var snapshots))
        {
            var snapshot = Items(snapshots).FirstOrDefault();
            if (snapshot.ValueKind == JsonValueKind.Object && snapshot.TryGetProperty("message", out var original) && original.ValueKind == JsonValueKind.Object) display = original;
        }
        var attachments = display.TryGetProperty("attachments", out var files) ? ParseAttachments(files).ToList() : new List<ChatAttachment>();
        if (display.TryGetProperty("embeds", out var embeds)) attachments.AddRange(ParseEmbeds(embeds));
        string? replyId = !forwarded && reference.ValueKind == JsonValueKind.Object && ValidMessageId(Str(reference, "message_id")) ? Str(reference, "message_id") : null;
        var parsed = new ChatMessage(Str(message, "id"), Str(message, "channel_id"), author, Str(display, "content"), timestamp, attachments.Take(50).ToArray(), Str(message, "edited_timestamp").Length > 0, false, replyId, MessageType: Int(message, "type"), Pinned: Bool(message, "pinned"), Forwarded: forwarded, WebhookId: Str(message, "webhook_id"), Mentions: ParseMessageMentions(display), ReplyPreview: ParseReplyPreview(message));
        if (message.TryGetProperty("reactions", out var reactions)) parsed = parsed with { Reactions = ParseReactions(reactions) };
        if (parsed.MessageType == 3) return ApplyCallMetadata(parsed, message.TryGetProperty("call", out var call) ? call : default);
        return parsed.Content.Length == 0 && parsed.MessageType is 1 or 2 or 4 or 5 or 6 or 7 ? parsed with { Content = parsed.MessageType switch { 1 => "Añadió participantes al grupo", 2 => "Salió un participante del grupo", 4 => "Cambió el nombre del grupo", 5 => "Cambió la imagen del grupo", 6 => "Fijó un mensaje", _ => "Se unió al servidor" } } : parsed;
    }
    public static IEnumerable<JsonElement> Items(JsonElement e) => e.ValueKind == JsonValueKind.Array ? e.EnumerateArray() : Enumerable.Empty<JsonElement>();
    public static string Str(JsonElement e, string name) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var p) && p.ValueKind == JsonValueKind.String ? p.GetString() ?? "" : "";
    public static int Int(JsonElement e, string name) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var p) && p.ValueKind == JsonValueKind.Number && p.TryGetInt32(out int value) ? value : 0;
    public static bool Bool(JsonElement e, string name) => e.ValueKind == JsonValueKind.Object && e.TryGetProperty(name, out var p) && p.ValueKind == JsonValueKind.True;
    private static string Snowflake(string id) => id.Length is >= 1 and <= 24 && id.All(char.IsAsciiDigit) ? id : throw new ServiceException("El identificador no es válido.");
    private static string InviteCode(string value)
    {
        string code = value.Trim();
        if (Uri.TryCreate(code, UriKind.Absolute, out var uri))
        {
            if (uri.Scheme != "https" || !(uri.Host == "discord.gg" || uri.Host == "discord.com" || uri.Host == "www.discord.com")) throw new ServiceException("Usa una invitación de Discord válida.");
            code = uri.AbsolutePath.Split('/', StringSplitOptions.RemoveEmptyEntries).LastOrDefault() ?? "";
        }
        return Regex.IsMatch(code, "^[A-Za-z0-9_-]{2,100}$") ? code : throw new ServiceException("El código de invitación no es válido.");
    }

    public async ValueTask DisposeAsync()
    {
        if (disposed) return;
        BeginSessionTransition(); disposed = true; redraw.Stop(); window.ActionRequested -= OnAction;
        await authenticationGate.WaitAsync();
        try
        {
            await StopGameActivityAsync();
            await StopMediaAsync(false);
            var oldSession = session; session = null;
            if (oldSession != null) await oldSession.DisposeAsync();
            await features.DisposeAsync(); channelLoad.Dispose(); sessionWork.Dispose();
            if (gameActivity != null) await gameActivity.DisposeAsync();
        }
        finally { authenticationGate.Release(); }
    }
}
