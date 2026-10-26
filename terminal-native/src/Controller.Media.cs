using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Threading;
using System.Runtime.InteropServices;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private MediaBridge? media;
    private string voiceChannel = "", voiceGuild = "", voiceSession = "", voiceEndpoint = "", voiceCredential = "";
    private string streamKey = "", rtcServerId = "", streamChannel = "", streamEndpoint = "", streamCredential = "", sourceId = "";
    private bool voiceConnected, voiceEncryptionReady, voiceConnecting, sharing, sharePaused, shareConnecting, shareTransportStarting, selfMute, selfDeaf, shareSystemAudio;
    private int voiceGeneration, shareGeneration;
    private bool voiceRequestPending;
    private bool cameraAvailable, cameraEnabled, cameraPreparing;
    private string callStatusLabel = "", callChannelName = "";
    private DateTimeOffset? callStartedAt;
    private readonly Dictionary<string, CallParticipant> callParticipants = new();
    private string pendingDmRingChannel = "";
    private int shareHeight = 720, shareFps = 30;
    private readonly SemaphoreSlim audioControls = new(1, 1);
    private MediaPreferences mediaPreferences = new();
    private MediaSettingsDialog? mediaSettingsDialog;
    private bool mediaSettingsOpening;
    private DispatcherTimer? pttTimer;
    private bool pttPressed, pttSending;
    private int pendingVideoGeneration = -1;
    [DllImport("user32.dll")] private static extern short GetAsyncKeyState(int virtualKey);
    private string TerminalRoot => Directory.GetParent(dataDirectory)!.FullName;

    private void LoadAudioPreferences()
    {
        mediaPreferences = MediaPreferences.Load(Path.Combine(dataDirectory, "media-settings.json"));
        string path = Path.Combine(dataDirectory, "audio-settings.json");
        try
        {
            if (!File.Exists(path) || new FileInfo(path).Length > 1024) return;
            using var document = JsonDocument.Parse(File.ReadAllText(path));
            selfMute = Bool(document.RootElement, "mute"); selfDeaf = Bool(document.RootElement, "deaf");
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException) { }
        finally { RefreshMediaControls(); }
    }

    private bool SaveAudioPreferences()
    {
        string path = Path.Combine(dataDirectory, "audio-settings.json");
        try
        {
            File.WriteAllText(path + ".tmp", JsonSerializer.Serialize(new { mute = selfMute, deaf = selfDeaf }));
            File.Move(path + ".tmp", path, overwrite: true); return true;
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException) { return false; }
    }

    private void RefreshMediaControls()
    {
        if (voiceChannel.Length > 0 && state.CurrentUser is Person self)
        {
            callParticipants.TryGetValue(self.Id, out var prior);
            callParticipants[self.Id] = new(self, true, selfMute || selfDeaf, selfDeaf, !(selfMute || selfDeaf) && (prior?.Speaking ?? false), cameraEnabled, sharing);
        }
        foreach (var id in callParticipants.Keys.ToArray())
            if (knownPeople.TryGetValue(id, out var person)) callParticipants[id] = callParticipants[id] with { User = person };
        state = state with { SelfMute = selfMute || selfDeaf, SelfDeaf = selfDeaf, VoiceConnected = voiceConnected, VoiceEncryptionReady = voiceEncryptionReady, VoiceConnecting = voiceChannel.Length > 0 && !voiceConnected, ScreenAvailable = voiceEncryptionReady,
            CallChannelName = callChannelName, CallStatusLabel = callStatusLabel, CallStartedAt = callStartedAt,
            CallParticipants = callParticipants.Values.OrderByDescending(p => p.IsSelf).ThenBy(p => p.User.Name).ToArray(),
            CameraAvailable = voiceEncryptionReady && cameraAvailable, CameraEnabled = cameraEnabled, CameraPreparing = cameraPreparing,
            ScreenSharing = sharing, ScreenPaused = sharePaused, ScreenPreparing = shareConnecting };
        Refresh();
    }

    private CallParticipant? CallPerson(string id)
    {
        if (id.Length is < 1 or > 20 || id.Any(c => c is < '0' or > '9')) return null;
        if (callParticipants.TryGetValue(id, out var found)) return found;
        if (callParticipants.Count >= 256) return null;
        bool self = id == state.CurrentUser?.Id;
        Person person = self ? state.CurrentUser! : knownPeople.TryGetValue(id, out var known) ? known : new Person(id, "Usuario");
        return new(person, self, false, false, false, false, false);
    }

    private void UpdateCallParticipant(JsonElement data)
    {
        string id = Str(data, "user_id");
        if (id.Length == 0) return;
        if (Str(data, "channel_id") != voiceChannel) { callParticipants.Remove(id); RefreshMediaControls(); return; }
        var person = CallPerson(id); if (person == null) return;
        if (data.TryGetProperty("member", out var member) && member.TryGetProperty("user", out var user))
        {
            string name = Str(user, "global_name"); if (name.Length == 0) name = Str(user, "username");
            if (name.Length > 0) person = person with { User = person.User with { Name = name } };
        }
        callParticipants[id] = person with { SelfMute = Bool(data, "self_mute") || Bool(data, "mute"), SelfDeaf = Bool(data, "self_deaf") || Bool(data, "deaf"), SelfVideo = Bool(data, "self_video"), ScreenSharing = Bool(data, "self_stream") };
        RefreshMediaControls();
    }

    private async Task SetSelfAudioAsync(bool? mute = null, bool? deaf = null)
    {
        await audioControls.WaitAsync();
        try
        {
            var own = media; int attempt = voiceGeneration;
            selfMute = mute ?? selfMute; selfDeaf = deaf ?? selfDeaf;
            try
            {
                if (own != null && (voiceConnected || voiceConnecting))
                {
                    await own.CommandAsync("mute", new { value = selfMute || selfDeaf });
                    await own.CommandAsync("deafen", new { value = selfDeaf });
                }
                if (media == own && voiceGeneration == attempt && session != null && voiceChannel.Length > 0)
                    await session.UpdateVoiceAsync(voiceGuild.Length > 0 ? voiceGuild : null, voiceChannel, selfMute || selfDeaf, selfDeaf, cameraEnabled);
                bool saved = SaveAudioPreferences();
                Status(!saved ? "Audio actualizado; no se pudo guardar la preferencia para el próximo inicio" : selfDeaf ? "Audio y micrófono desactivados" : selfMute ? "Micrófono silenciado" : voiceConnected ? "Micrófono y audio activados" : "Preferencia de audio guardada para la próxima llamada");
            }
            catch
            {
                if (media == own && voiceGeneration == attempt)
                {
                    // A command may already have changed the native microphone before
                    // a later command fails. Never roll back just its visible indicator.
                    selfMute = true;
                    SaveAudioPreferences();
                    await StopMediaAsync(true);
                    throw new ServiceException("No se pudo confirmar el cambio de audio. Se cerró la llamada y el micrófono quedó silenciado para el próximo intento.");
                }
                throw;
            }
            finally { RefreshMediaControls(); }
        }
        finally { audioControls.Release(); }
    }

    private async Task ProbeMediaAsync()
    {
        LoadAudioPreferences();
        if (!MediaBridge.IsAvailable(TerminalRoot)) return;
        await using var probe = new MediaBridge(TerminalRoot);
        var capabilities = await probe.CommandAsync("capabilities");
        state = state with { VoiceAvailable = Bool(capabilities, "voice"), ScreenAvailable = false };
        Refresh();
    }

    private async Task EnsureMediaAsync()
    {
        if (media != null) return;
        lastMediaFailureCode = null; lastMediaExitCode = null; lastMediaStage = null;
        media = new MediaBridge(TerminalRoot);
        var own = media;
        media.Event += (type, data) => window.Dispatcher.BeginInvoke(async () =>
        {
            if (disposed || media != own) return;
            if (type == "mediaStage") RememberMediaStage(data);
            else if (type == "voiceState")
            {
                string status = ApplyVoiceReadiness(data);
                UpdatePttTimer();
                RefreshMediaControls();
                Status(status switch { "connected" => "Voz conectada", "waiting-peer" => "En llamada · esperando participantes. El audio se transmite cuando termina la conexión cifrada.", "connecting" => "Conectando voz…", "negotiating" => "Preparando conexión cifrada…", "error" => "La conexión de voz falló: " + ErrorReason(data), _ => "Voz desconectada" });
                if (status is "error" or "disconnected") { voiceConnecting = false; await GuardAsync(() => StopMediaAsync(true)); }
                else if (voiceEncryptionReady && cameraAvailable && pendingVideoGeneration == voiceGeneration)
                {
                    pendingVideoGeneration = -1;
                    await GuardAsync(ToggleCameraAsync);
                }
            }
            else if (type == "micTest")
            {
                double level = data.TryGetProperty("level", out var measured) && measured.TryGetDouble(out double value) && double.IsFinite(value) ? value : 0;
                mediaSettingsDialog?.UpdateMicTest(Bool(data, "active"), level);
            }
            else if (type == "shareState")
            {
                string status = Str(data, "state");
                if (status == "capture-requested") { shareConnecting = shareTransportStarting = sharePaused = false; sharing = true; Status($"Pantalla compartida · {shareHeight}p/{shareFps} FPS"); }
                else if (status == "paused") { shareConnecting = shareTransportStarting = false; sharing = sharePaused = true; Status("Pantalla en pausa mientras se restablece la conexión cifrada."); }
                else if (status == "error") { sharing = false; string reason = ErrorReason(data); await GuardAsync(StopShareAsync); Status("Compartir pantalla falló: " + reason); }
                RefreshMediaControls();
            }
            else if (type == "cameraState")
            {
                string status = Str(data, "state");
                bool synchronizeStopped = cameraEnabled && status is "stopped" or "error";
                int attempt = voiceGeneration; var gateway = session;
                cameraEnabled = status == "active"; cameraPreparing = status == "preparing";
                RefreshMediaControls();
                if (status == "error") Status("Cámara detenida: " + ErrorReason(data));
                if (synchronizeStopped && gateway != null)
                {
                    // A device can disappear after ToggleCameraAsync returned.
                    // Serialize with explicit camera/audio controls and never send
                    // an old stop event into a replacement call or a restarted camera.
                    await audioControls.WaitAsync();
                    try
                    {
                        if (media == own && voiceGeneration == attempt && session == gateway && voiceConnected && !cameraEnabled)
                            await gateway.UpdateVoiceAsync(voiceGuild.Length > 0 ? voiceGuild : null, voiceChannel, selfMute || selfDeaf, selfDeaf, false);
                    }
                    catch
                    {
                        if (media == own && voiceGeneration == attempt && session == gateway && !cameraEnabled)
                            await GuardAsync(() => StopMediaAsync(true));
                    }
                    finally { audioControls.Release(); }
                }
            }
            else if (type is "participant" or "speaking")
            {
                string id = Str(data, "userId"); var person = CallPerson(id); if (person == null) return;
                if (type == "participant" && !Bool(data, "present")) callParticipants.Remove(id);
                else if (type == "speaking") callParticipants[id] = person with { Speaking = Bool(data, "active") };
                else callParticipants[id] = data.TryGetProperty("video", out _) ? person with { SelfVideo = Bool(data, "video") } : person;
                RefreshMediaControls();
            }
            else if (type == "fatal") { RememberMediaFailure(data); await GuardAsync(() => StopMediaAsync(true)); Status("El servicio multimedia se detuvo. Vuelve a entrar en el canal de voz."); }
        });
        try { await own.CommandAsync("capabilities"); await own.CommandAsync("configureAudio", mediaPreferences.AudioParameters()); }
        catch
        {
            if (media == own) await StopMediaAsync(false);
            throw;
        }
    }
    private string ApplyVoiceReadiness(JsonElement data)
    {
        string status = Str(data, "state");
        voiceEncryptionReady = status == "connected" && Bool(data, "encryptionReady");
        voiceConnected = voiceEncryptionReady || Bool(data, "transportReady") && status is ("waiting-peer" or "negotiating");
        callStatusLabel = status switch { "connected" when voiceEncryptionReady => "Conexión cifrada", "waiting-peer" => "Conectado · esperando participantes · micrófono local listo", "connecting" => "Conectando voz…", "negotiating" => voiceConnected ? "Conectado · preparando conexión cifrada…" : "Preparando conexión cifrada…", "error" => "Error de conexión", _ => "Desconectada" };
        cameraAvailable = voiceEncryptionReady && Bool(data, "cameraAvailable");
        if (voiceConnected) { voiceConnecting = false; callStartedAt ??= DateTimeOffset.UtcNow; }
        return status;
    }
    private static string ErrorReason(JsonElement data) => data.TryGetProperty("reason", out var reason) ? reason.ValueKind == JsonValueKind.String ? reason.GetString() ?? "error interno" : Str(reason, "message") : "error interno";

    private async Task<bool> TryHandleMediaActionAsync(UiAction action)
    {
        if (session == null) return false;
        switch (action.Kind)
        {
            case "media-settings":
                await ShowMediaSettingsAsync(action.Text); return true;
            case "toggle-mute":
                await SetSelfAudioAsync(!(selfMute || selfDeaf), selfDeaf ? false : null); return true;
            case "toggle-deafen":
                await SetSelfAudioAsync(deaf: !selfDeaf); return true;
            case "toggle-camera":
                await ToggleCameraAsync(); return true;
            case "call-dm":
                await StartDmVoiceAsync(action.Id); return true;
            case "call-dm-video":
                await StartDmVoiceAsync(action.Id, video: true); return true;
            case "join-call":
                await StartDmVoiceAsync(action.Id, ring: false); return true;
            case "voice":
                if (voiceChannel.Length > 0) { await ShowVoiceSettingsAsync(); return true; }
                if (voiceRequestPending) { Status("La llamada todavía se está preparando…"); return true; }
                if (action.Id.Length == 0 && state.DirectMessages.Any(n => n.Id == currentChannel)) { await StartDmVoiceAsync(currentChannel); return true; }
                var channels = state.Channels.Where(n => n.Kind is "voice" or "stage").ToArray();
                string choice = action.Id;
                if (!channels.Any(n => n.Id == choice)) choice = Choose("Entrar en voz", channels.Select(c => (c.Id, c.Name)).ToArray()) ?? "";
                if (choice.Length == 0) return true;
                if (channels.First(c => c.Id == choice).Kind == "stage") { Status("Los canales de escenario todavía requieren adaptar la solicitud de palabra."); return true; }
                voiceRequestPending = true;
                MediaBridge? joiningMedia = null; int joiningAttempt = -1;
                try
                {
                    string guild = currentGuild;
                    var gateway = session; int login = sessionGeneration;
                    await EnsureMediaAsync();
                    if (!IsCurrent(gateway, login)) return true;
                    voiceGeneration++;
                    joiningMedia = media; joiningAttempt = voiceGeneration;
                    voiceChannel = choice; voiceGuild = guild; pendingDmRingChannel = "";
                    callChannelName = channels.First(c => c.Id == choice).Name; callStatusLabel = "Solicitando acceso al canal…";
                    voiceSession = voiceEndpoint = voiceCredential = ""; voiceConnecting = false;
                    RefreshMediaControls();
                    await gateway.UpdateVoiceAsync(voiceGuild.Length > 0 ? voiceGuild : null, voiceChannel, selfMute || selfDeaf, selfDeaf);
                    Status("Solicitando acceso al canal de voz…"); return true;
                }
                catch
                {
                    if (joiningMedia != null && media == joiningMedia && voiceGeneration == joiningAttempt)
                        await StopMediaAsync(true);
                    throw;
                }
                finally { voiceRequestPending = false; }
            case "disconnect-voice": await StopMediaAsync(true); return true;
            case "share":
            {
                if (sharing)
                {
                    await StopShareAsync(); Status("Pantalla compartida detenida"); return true;
                }
                if (media == null || !voiceEncryptionReady) { Status(voiceConnected ? "Espera a que otro participante complete la conexión cifrada antes de compartir." : "Conéctate a voz antes de compartir."); return true; }
                if (shareConnecting) { Status("La pantalla compartida todavía se está preparando…"); return true; }
                var own = media;
                int attempt = ++shareGeneration;
                bool requested = false;
                shareConnecting = true;
                RefreshMediaControls();
                try
                {
                    var sources = await own.CommandAsync("listSources");
                    if (!IsCurrentShare(own, attempt)) return true;
                    var source = Choose("Compartir una ventana o pantalla", Items(sources).Select(s => (Str(s, "id"), Str(s, "name"))).ToArray());
                    if (source == null || !IsCurrentShare(own, attempt)) return true;
                    string quality = Choose("Calidad de transmisión", new[] { ("720:30", "720p · 30 FPS"), ("1080:30", "1080p · 30 FPS"), ("1080:60", "1080p · 60 FPS"), ("1440:60", "1440p · 60 FPS") }) ?? "";
                    if (quality.Length == 0 || !IsCurrentShare(own, attempt)) return true;
                    string? audio = Choose("Audio de la transmisión", new[] { ("off", "Sin audio de aplicaciones"), ("system", "Incluir todo el audio del sistema") });
                    if (audio == null || !IsCurrentShare(own, attempt)) return true;
                    sourceId = source;
                    shareHeight = int.Parse(quality.Split(':')[0]); shareFps = int.Parse(quality.Split(':')[1]);
                    shareSystemAudio = audio == "system";
                    rtcServerId = streamChannel = streamEndpoint = streamCredential = streamKey = "";
                    await session.CreateStreamAsync(voiceGuild.Length > 0 ? voiceGuild : null, voiceChannel);
                    requested = true;
                    if (!IsCurrentShare(own, attempt))
                    {
                        if (IsSameShareAttempt(own, attempt))
                        {
                            if (streamKey.Length == 0) streamKey = ExpectedStreamKey();
                            await StopShareAsync();
                        }
                        return true;
                    }
                    if (!sharing) Status("Preparando sesión de pantalla compartida…");
                    return true;
                }
                catch
                {
                    if (IsSameShareAttempt(own, attempt)) await StopShareAsync();
                    throw;
                }
                finally
                {
                    if (!requested && IsSameShareAttempt(own, attempt)) shareConnecting = false;
                    RefreshMediaControls();
                }
            }
            default: return false;
        }
    }

    private async Task StartDmVoiceAsync(string channelId, bool video = false, bool ring = true)
    {
        var gateway = session; int login = sessionGeneration;
        if (gateway == null || !state.DirectMessages.Any(n => n.Id == channelId)) return;
        if (!ring && !activeCalls.ContainsKey(channelId)) { Status("La llamada ya terminó. No se inició una llamada nueva."); return; }
        if (voiceChannel.Length > 0)
        {
            if (video && voiceChannel == channelId && voiceConnected) { if (!cameraEnabled) await ToggleCameraAsync(); }
            else await ShowVoiceSettingsAsync();
            return;
        }
        if (voiceRequestPending) { Status("La llamada todavía se está preparando…"); return; }
        voiceRequestPending = true;
        MediaBridge? joiningMedia = null; int joiningAttempt = -1;
        try
        {
            // The original client checks whether a DM is callable before selecting voice.
            var call = await gateway.GetAsync($"channels/{Snowflake(channelId)}/call", sessionWork.Token);
            if (!IsCurrent(gateway, login)) return;
            if (!ring && !activeCalls.ContainsKey(channelId)) { Status("La llamada terminó mientras se comprobaba. No se inició otra llamada."); return; }
            await EnsureMediaAsync();
            if (!IsCurrent(gateway, login)) return;
            if (!ring && !activeCalls.ContainsKey(channelId))
            {
                if (voiceChannel.Length == 0) await StopMediaAsync(false);
                Status("La llamada terminó antes de conectar. No se inició otra llamada."); return;
            }
            voiceGeneration++; voiceChannel = channelId; voiceGuild = "";
            pendingVideoGeneration = video ? voiceGeneration : -1;
            callChannelName = state.DirectMessages.First(n => n.Id == channelId).Name; callStatusLabel = "Conectando llamada privada…";
            joiningMedia = media; joiningAttempt = voiceGeneration;
            voiceSession = voiceEndpoint = voiceCredential = ""; voiceConnecting = false;
            pendingDmRingChannel = ring && Bool(call, "ringable") ? channelId : "";
            RefreshMediaControls();
            await gateway.UpdateVoiceAsync(null, channelId, selfMute || selfDeaf, selfDeaf);
            Status("Conectando llamada privada…");
        }
        catch
        {
            if (joiningMedia != null && media == joiningMedia && voiceGeneration == joiningAttempt)
                await StopMediaAsync(true);
            throw;
        }
        finally { voiceRequestPending = false; }
    }

    private async Task ToggleCameraAsync()
    {
        bool stopRequested = cameraEnabled;
        if (media == null || !voiceEncryptionReady && !stopRequested) { Status(voiceConnected ? "Espera a que se complete la conexión cifrada antes de activar la cámara." : "Entra en una llamada antes de activar la cámara."); return; }
        if (cameraPreparing) { Status("La cámara todavía se está preparando…"); return; }
        if (!cameraAvailable && !stopRequested) { Status("Esta llamada no negoció vídeo de cámara. Vuelve a entrar para habilitarlo."); return; }
        var own = media; var gateway = session; int attempt = voiceGeneration;
        bool current() => media == own && voiceGeneration == attempt && session == gateway && (voiceEncryptionReady || stopRequested);
        bool requested = false;
        cameraPreparing = true; RefreshMediaControls();
        await audioControls.WaitAsync();
        try
        {
            if (!current()) return;
            if (stopRequested)
            {
                requested = true;
                await own.CommandAsync("stopCamera");
                if (!current()) return;
                cameraEnabled = false;
            }
            else
            {
                // Enumerate devices only after the camera button is explicitly clicked.
                var devices = await own.CommandAsync("listCameras");
                if (!current()) return;
                var available = Items(devices).Select(d => (Id: Str(d, "id"), Name: Str(d, "name"))).ToArray();
                string? id = available.Any(d => d.Id == mediaPreferences.CameraId) ? mediaPreferences.CameraId : Choose("Activar cámara · selecciona el dispositivo", available);
                if (id == null || !current()) return;
                mediaPreferences = mediaPreferences with { CameraId = id };
                mediaPreferences.Save(Path.Combine(dataDirectory, "media-settings.json"));
                requested = true;
                var result = await own.CommandAsync("startCamera", new { deviceId = id });
                if (!current()) return;
                cameraEnabled = Str(result, "state") == "active";
            }
            if (gateway != null && current()) await gateway.UpdateVoiceAsync(voiceGuild.Length > 0 ? voiceGuild : null, voiceChannel, selfMute || selfDeaf, selfDeaf, cameraEnabled);
            if (current()) Status(cameraEnabled ? "Cámara activada · sin previsualización local" : "Cámara desactivada");
        }
        catch
        {
            if (requested && current())
            {
                try
                {
                    await own.CommandAsync("stopCamera");
                    if (current())
                    {
                        cameraEnabled = false;
                        if (gateway != null) await gateway.UpdateVoiceAsync(voiceGuild.Length > 0 ? voiceGuild : null, voiceChannel, selfMute || selfDeaf, selfDeaf, false);
                    }
                }
                catch { if (current()) await StopMediaAsync(true); }
            }
            throw;
        }
        finally
        {
            if (current()) { cameraPreparing = false; RefreshMediaControls(); }
            audioControls.Release();
        }
    }

    private async Task<bool> HandleMediaDispatchAsync(string type, JsonElement data)
    {
        if (type == "CALL_CREATE" && voiceGuild.Length == 0 && voiceChannel.Length > 0 && Str(data, "channel_id") == voiceChannel)
        {
            if (data.TryGetProperty("voice_states", out var states)) foreach (var entry in Items(states)) UpdateCallParticipant(entry);
            if (session != null && pendingDmRingChannel == voiceChannel)
            {
                string channel = pendingDmRingChannel; pendingDmRingChannel = "";
                await session.SendAsync(System.Net.Http.HttpMethod.Post, $"channels/{Snowflake(channel)}/call/ring", new { recipients = (string[]?)null }, sessionWork.Token);
            }
            return true;
        }
        if (type == "CALL_DELETE" && voiceGuild.Length == 0 && voiceChannel.Length > 0 && Str(data, "channel_id") == voiceChannel)
        { await StopMediaAsync(false); return true; }
        if (type == "VOICE_STATE_UPDATE" && voiceChannel.Length > 0)
        {
            UpdateCallParticipant(data);
            if (Str(data, "user_id") != state.CurrentUser?.Id) return true;
            if (Str(data, "channel_id").Length == 0) { await StopMediaAsync(false); return true; }
            if (Str(data, "channel_id") != voiceChannel) return true;
            voiceSession = Str(data, "session_id"); await TryConnectVoiceAsync(); return true;
        }
        if (type == "VOICE_SERVER_UPDATE" && voiceChannel.Length > 0)
        {
            if (voiceGuild.Length > 0 && Str(data, "guild_id") != voiceGuild) return true;
            if (voiceGuild.Length == 0 && Str(data, "channel_id").Length > 0 && Str(data, "channel_id") != voiceChannel) return true;
            voiceEndpoint = Str(data, "endpoint"); voiceCredential = Str(data, "token"); await TryConnectVoiceAsync(); return true;
        }
        if (type == "STREAM_CREATE" && sourceId.Length > 0)
        {
            string key = Str(data, "stream_key"); if (key != ExpectedStreamKey()) return true;
            streamKey = key; rtcServerId = Str(data, "rtc_server_id"); streamChannel = Str(data, "stream_channel_id"); await TryStartShareAsync(); return true;
        }
        if (type == "STREAM_SERVER_UPDATE" && sourceId.Length > 0)
        {
            string key = Str(data, "stream_key"); if (key != ExpectedStreamKey()) return true;
            if (streamKey.Length == 0) streamKey = key;
            if (streamKey != key) return true;
            streamEndpoint = Str(data, "endpoint"); streamCredential = Str(data, "token"); await TryStartShareAsync(); return true;
        }
        if (type == "STREAM_DELETE" && streamKey.Length > 0 && Str(data, "stream_key") == streamKey)
        {
            await StopShareAsync(false); Status("Transmisión finalizada"); return true;
        }
        return false;
    }
    private async Task TryConnectVoiceAsync()
    {
        if (voiceConnected || voiceConnecting || media == null || voiceSession.Length == 0 || voiceEndpoint.Length == 0 || voiceCredential.Length == 0) return;
        var own = media;
        int attempt = voiceGeneration;
        voiceConnecting = true;
        try
        {
            await own.CommandAsync("connectVoice", new { userId = state.CurrentUser!.Id, guildId = voiceGuild.Length > 0 ? voiceGuild : null, channelId = voiceChannel, sessionId = voiceSession, voiceToken = voiceCredential, endpoint = voiceEndpoint, selfMute = selfMute || selfDeaf, selfDeaf });
            if (media == own && voiceGeneration == attempt) voiceCredential = "";
        }
        catch
        {
            if (own.LastFailure is JsonElement failure) RememberMediaFailure(failure);
            if (media == own && voiceGeneration == attempt) await StopMediaAsync(true);
            throw;
        }
    }
    private string ExpectedStreamKey() => voiceGuild.Length > 0 ? $"guild:{voiceGuild}:{voiceChannel}:{state.CurrentUser?.Id}" : $"call:{voiceChannel}:{state.CurrentUser?.Id}";
    private bool IsSameShareAttempt(MediaBridge own, int attempt) => media == own && shareGeneration == attempt;
    private bool IsCurrentShare(MediaBridge own, int attempt) => IsSameShareAttempt(own, attempt) && voiceEncryptionReady;
    private Task StopShareAsync() => StopShareAsync(true);
    private async Task StopShareAsync(bool notifyGateway)
    {
        var own = media;
        var gateway = session;
        int stopped = ++shareGeneration;
        string key = streamKey;
        streamKey = rtcServerId = streamChannel = streamEndpoint = streamCredential = sourceId = "";
        sharing = sharePaused = shareConnecting = shareTransportStarting = shareSystemAudio = false;
        RefreshMediaControls();
        try { if (own != null) await own.CommandAsync("stopShare"); }
        finally { if (notifyGateway && gateway != null && key.Length > 0 && media == own && shareGeneration == stopped) await gateway.DeleteStreamAsync(key); }
    }
    private async Task TryStartShareAsync()
    {
        if (media == null || !shareConnecting || shareTransportStarting || sharing || rtcServerId.Length == 0 || streamEndpoint.Length == 0 || streamCredential.Length == 0) return;
        var own = media;
        int attempt = shareGeneration;
        if (!voiceEncryptionReady) { await StopShareAsync(); return; }
        shareTransportStarting = true;
        try
        {
            await own.CommandAsync("startShare", new { userId = state.CurrentUser!.Id, channelId = voiceChannel, rtcServerId, streamChannelId = streamChannel.Length > 0 ? streamChannel : null, sessionId = voiceSession, voiceToken = streamCredential, endpoint = streamEndpoint, sourceId, quality = new { height = shareHeight, fps = shareFps }, audio = new { enabled = shareSystemAudio, loopback = shareSystemAudio, pid = 0 } });
            if (!IsCurrentShare(own, attempt))
            {
                if (IsSameShareAttempt(own, attempt)) await StopShareAsync();
                return;
            }
            streamCredential = "";
            if (session != null) await session.UnpauseStreamAsync(streamKey);
        }
        catch
        {
            if (IsSameShareAttempt(own, attempt)) await StopShareAsync();
            throw;
        }
    }
    private async Task StopMediaAsync(bool leaveGateway)
    {
        var own = media; media = null;
        var gateway = session;
        string key = streamKey, channel = voiceChannel, guild = voiceGuild;
        int stopped = ++voiceGeneration;
        pendingVideoGeneration = -1; pttTimer?.Stop(); pttPressed = false;
        shareGeneration++;
        voiceConnected = voiceEncryptionReady = voiceConnecting = sharing = sharePaused = shareConnecting = shareTransportStarting = shareSystemAudio = false;
        cameraAvailable = cameraEnabled = cameraPreparing = false;
        callStartedAt = null; callChannelName = callStatusLabel = ""; callParticipants.Clear();
        pendingDmRingChannel = "";
        voiceChannel = voiceGuild = voiceSession = voiceEndpoint = voiceCredential = streamKey = rtcServerId = streamChannel = streamEndpoint = streamCredential = sourceId = "";
        RefreshMediaControls();
        try
        {
            // Stop microphone/capture locally before waiting for a remote leave ACK.
            if (own != null) await own.DisposeAsync();
        }
        finally
        {
            if (leaveGateway && gateway != null && channel.Length > 0 && media == null && voiceGeneration == stopped)
            {
                try
                {
                    if (key.Length > 0) await gateway.DeleteStreamAsync(key);
                    if (media == null && voiceGeneration == stopped) await gateway.UpdateVoiceAsync(guild.Length > 0 ? guild : null, null);
                }
                catch (Exception error) when (error is ServiceException or System.Net.WebSockets.WebSocketException or OperationCanceledException or IOException) { }
            }
            Refresh();
        }
    }

    private Task ShowVoiceSettingsAsync() => ShowMediaSettingsAsync("");

    private async Task ShowMediaSettingsAsync(string focus)
    {
        if (mediaSettingsDialog != null) { mediaSettingsDialog.Activate(); return; }
        if (mediaSettingsOpening) return;
        mediaSettingsOpening = true;
        try { await EnsureMediaAsync(); }
        catch { mediaSettingsOpening = false; throw; }
        var own = media!;
        try
        {
            var audio = await own.CommandAsync("listDevices");
            var cameras = await own.CommandAsync("listCameras");
            if (media != own || disposed) return;
            MediaDevice[] Devices(JsonElement value) => Items(value).Take(128).Select(d => new MediaDevice(Str(d, "id"), Str(d, "name"))).Where(d => d.Id.Length > 0).ToArray();
            var inventory = new MediaDeviceInventory(audio.TryGetProperty("input", out var inputs) ? Devices(inputs) : [], audio.TryGetProperty("output", out var outputs) ? Devices(outputs) : [], Devices(cameras));
            mediaSettingsDialog = new MediaSettingsDialog(window, mediaPreferences, inventory,
                async (preferences, persist) =>
                {
                    if (media != own) throw new ServiceException("La llamada o el servicio de audio cambió. Abre de nuevo los ajustes.");
                    bool restartCamera = false; int cameraAttempt = voiceGeneration;
                    await audioControls.WaitAsync();
                    try
                    {
                        var normalized = preferences.Normalize();
                        try { await own.CommandAsync("configureAudio", normalized.AudioParameters()); }
                        catch
                        {
                            // Native configuration can fail after changing input mode.
                            // Do not leave a formerly PTT microphone transmitting as VAD.
                            if (media == own && voiceChannel.Length > 0)
                            {
                                selfMute = true; SaveAudioPreferences(); await StopMediaAsync(true);
                            }
                            throw;
                        }
                        if (media != own) return;
                        if (persist)
                        {
                            restartCamera = cameraEnabled && normalized.CameraId.Length > 0 && normalized.CameraId != mediaPreferences.CameraId;
                            mediaPreferences = normalized; pttPressed = false; UpdatePttTimer();
                            if (!normalized.Save(Path.Combine(dataDirectory, "media-settings.json"))) throw new ServiceException("Se aplicaron los cambios, pero no fue posible guardarlos.");
                        }
                    }
                    finally { audioControls.Release(); }
                    if (restartCamera && media == own && voiceGeneration == cameraAttempt && voiceConnected && cameraEnabled)
                    {
                        await ToggleCameraAsync();
                        if (media == own && voiceGeneration == cameraAttempt && voiceConnected && !cameraEnabled) await ToggleCameraAsync();
                    }
                },
                async enabled =>
                {
                    if (media != own)
                    {
                        if (enabled) throw new ServiceException("El servicio de audio cambió. Abre de nuevo los ajustes.");
                        return;
                    }
                    try { await own.CommandAsync(enabled ? "startMicTest" : "stopMicTest"); }
                    catch
                    {
                        // A failed local-stop ACK must not leave loopback/capture alive.
                        if (!enabled && media == own) await StopMediaAsync(true);
                        throw;
                    }
                },
                async preferences =>
                {
                    if (media != own || !voiceConnected) throw new ServiceException("La llamada ya no está conectada. Entra de nuevo antes de probar vídeo.");
                    if (!cameraEnabled)
                    {
                        mediaPreferences = mediaPreferences with { CameraId = preferences.Normalize().CameraId };
                        mediaPreferences.Save(Path.Combine(dataDirectory, "media-settings.json"));
                    }
                    await ToggleCameraAsync();
                }, voiceChannel.Length == 0, voiceConnected && cameraAvailable, focus);
            mediaSettingsDialog.ShowDialog();
        }
        finally
        {
            mediaSettingsOpening = false;
            mediaSettingsDialog = null;
            if (media == own && voiceChannel.Length == 0) await StopMediaAsync(false);
        }
    }

    private void UpdatePttTimer()
    {
        if (!voiceConnected || !mediaPreferences.PushToTalk || media == null) { pttTimer?.Stop(); pttPressed = false; return; }
        if (pttTimer == null)
        {
            pttTimer = new DispatcherTimer(DispatcherPriority.Background, window.Dispatcher) { Interval = TimeSpan.FromMilliseconds(30) };
            pttTimer.Tick += async (_, _) =>
            {
                var own = media; int attempt = voiceGeneration;
                if (pttSending || own == null || !voiceConnected || !mediaPreferences.PushToTalk) return;
                pttSending = true;
                await audioControls.WaitAsync();
                try
                {
                    if (media != own || voiceGeneration != attempt || !voiceConnected || !mediaPreferences.PushToTalk) return;
                    // Read only the configured single key, only while PTT voice is active.
                    bool pressed = (GetAsyncKeyState(mediaPreferences.PttVirtualKey) & 0x8000) != 0;
                    if (pressed == pttPressed) return;
                    await own.CommandAsync("ptt", new { active = pressed }); if (media == own && voiceGeneration == attempt) pttPressed = pressed;
                }
                catch
                {
                    if (media == own && voiceGeneration == attempt)
                    {
                        await GuardAsync(() => StopMediaAsync(true));
                        Status("La transmisión PTT no pudo confirmarse. Se cerró la llamada.");
                    }
                }
                finally { pttSending = false; audioControls.Release(); }
            };
        }
        pttTimer.Start();
    }
    private string? Choose(string title, (string Id, string Name)[] choices)
    {
        if (choices.Length == 0) { Status("No hay opciones disponibles."); return null; }
        var dialog = new Window { Title = title, Owner = window, Width = 540, Height = 390, WindowStartupLocation = WindowStartupLocation.CenterOwner, Background = UiTheme.Panel, Foreground = TerminalWindow.Ink };
        var panel = new DockPanel { Margin = new Thickness(16) };
        var button = TerminalWindow.Button("Seleccionar", () => { dialog.DialogResult = true; }); button.Margin = new Thickness(0, 12, 0, 0); button.Padding = new Thickness(12, 6, 12, 6); DockPanel.SetDock(button, Dock.Bottom); panel.Children.Add(button);
        var list = new ListBox { ItemsSource = choices.Select(c => c.Name).ToArray(), Background = UiTheme.Input, Foreground = TerminalWindow.Ink, BorderBrush = UiTheme.Line, SelectedIndex = 0 }; panel.Children.Add(list); dialog.Content = panel;
        return dialog.ShowDialog() == true && list.SelectedIndex >= 0 ? choices[list.SelectedIndex].Id : null;
    }
}
