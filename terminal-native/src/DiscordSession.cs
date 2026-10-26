using System.Net;
using System.Net.Http;
using System.Net.WebSockets;
using System.Text;
using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed class ServiceException(string message, int status = 0) : Exception(message)
{
    public int Status { get; } = status;
}

// A single authenticated REST transport and Gateway. No message polling, browser,
// telemetry, token logging, automatic sends, or silent CAPTCHA workarounds.
public sealed class DiscordSession : IAsyncDisposable
{
    private readonly HttpClient http;
    private readonly string token;
    private readonly SemaphoreSlim restLock = new(1, 1), sendLock = new(1, 1);
    private readonly CancellationTokenSource lifetime = new();
    private ClientWebSocket? socket;
    private Task? gatewayTask;
    private long? sequence;
    private string? sessionId;
    private Uri gatewayUrl = new("wss://gateway.discord.gg/?v=9&encoding=json");
    private bool heartbeatAck = true;
    private readonly object presenceStateLock = new();
    private readonly SemaphoreSlim presenceChangeLock = new(1, 1);
    private string selfPresence = "";
    private string? ownActivityName;
    private bool? showCurrentGame;
    private string subscribedGuild = "";
    public string SelfPresence { get { lock (presenceStateLock) return selfPresence; } }
    public bool? ShowCurrentGame { get { lock (presenceStateLock) return showCurrentGame; } }
    public event Func<string, JsonElement, Task>? Dispatch;
    public event Action<string>? StatusChanged;

    public DiscordSession(string credential, HttpMessageHandler? handler = null)
    {
        token = credential;
        http = handler == null ? new HttpClient(new HttpClientHandler { AutomaticDecompression = DecompressionMethods.All, AllowAutoRedirect = false }) : new HttpClient(handler);
        http.BaseAddress = new Uri("https://discord.com/api/v9/");
        http.Timeout = TimeSpan.FromSeconds(35);
        http.DefaultRequestHeaders.TryAddWithoutValidation("Authorization", token);
        http.DefaultRequestHeaders.UserAgent.ParseAdd("NotDiscordTerminal/0.1");
    }

    public Task<JsonElement> GetAsync(string path, CancellationToken ct = default) => RequestAsync(HttpMethod.Get, path, null, ct);
    public Task<JsonElement> SendAsync(HttpMethod method, string path, object? body = null, CancellationToken ct = default) => RequestAsync(method, path, body, ct);

    private async Task<JsonElement> RequestAsync(HttpMethod method, string path, object? body, CancellationToken ct)
    {
        if (path.StartsWith('/') || path.Contains(':') || path.Contains('\\') || path.Contains('\r') || path.Contains('\n') || path.Split('?')[0].Contains("..", StringComparison.Ordinal)) throw new ArgumentException("Ruta API inválida");
        var endpoint = new Uri(http.BaseAddress!, path);
        if (endpoint.Scheme != "https" || endpoint.Host != "discord.com" || !endpoint.IsDefaultPort || endpoint.UserInfo.Length != 0 || !endpoint.AbsolutePath.StartsWith("/api/v9/", StringComparison.Ordinal)) throw new ArgumentException("Ruta API inválida");
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct, lifetime.Token);
        await restLock.WaitAsync(linked.Token);
        try
        {
            for (int attempt = 0; attempt < 3; attempt++)
            {
                using var request = new HttpRequestMessage(method, endpoint);
                if (body != null) request.Content = new StringContent(JsonSerializer.Serialize(body), Encoding.UTF8, "application/json");
                using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, linked.Token);
                var data = await ReadBoundedAsync(response.Content, 16 * 1024 * 1024, linked.Token);
                if (response.StatusCode == HttpStatusCode.TooManyRequests)
                {
                    double delay = 5;
                    try { using var rate = JsonDocument.Parse(data); delay = rate.RootElement.GetProperty("retry_after").GetDouble(); } catch (JsonException) { }
                    if (delay > 120 || attempt == 2) throw new ServiceException("Discord ha limitado las peticiones. Espera antes de volver a intentarlo.", 429);
                    await Task.Delay(TimeSpan.FromSeconds(Math.Clamp(delay, .5, 120)), linked.Token);
                    continue;
                }
                if (!response.IsSuccessStatusCode) throw new ServiceException(response.StatusCode switch
                {
                    HttpStatusCode.Unauthorized => "La sesión caducó. Inicia sesión de nuevo.",
                    HttpStatusCode.Forbidden => "Discord no permite esta acción con los permisos actuales.",
                    HttpStatusCode.NotFound => "El contenido ya no está disponible o no tienes acceso.",
                    HttpStatusCode.BadRequest => "Discord rechazó la operación. Puede requerir datos adicionales o una verificación en el cliente oficial.",
                    _ => "No se pudo completar la operación con Discord."
                }, (int)response.StatusCode);
                if (data.Length == 0) return JsonSerializer.SerializeToElement(new { });
                using var document = JsonDocument.Parse(data);
                return document.RootElement.Clone();
            }
            throw new ServiceException("Se agotaron los intentos permitidos.");
        }
        finally { restLock.Release(); }
    }

    public async Task<JsonElement> UploadAsync(string channelId, string path, string content, CancellationToken ct = default)
    {
        if (!channelId.All(char.IsAsciiDigit) || channelId.Length == 0) throw new ArgumentException("Canal inválido");
        var file = new FileInfo(path);
        if (!file.Exists || file.Length > 25 * 1024 * 1024) throw new ServiceException("El adjunto supera el límite local de 25 MiB o no existe. El límite de tu cuenta también se aplica.");
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct, lifetime.Token);
        await restLock.WaitAsync(linked.Token);
        try
        {
            using var form = new MultipartFormDataContent();
            form.Add(new StringContent(JsonSerializer.Serialize(new { content, allowed_mentions = new { parse = Array.Empty<string>() } }), Encoding.UTF8, "application/json"), "payload_json");
            using var stream = file.OpenRead();
            form.Add(new StreamContent(stream), "files[0]", file.Name);
            using var request = new HttpRequestMessage(HttpMethod.Post, $"channels/{channelId}/messages") { Content = form };
            using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, linked.Token);
            if (!response.IsSuccessStatusCode) throw new ServiceException("Discord rechazó el adjunto. Comprueba tamaño y permisos.", (int)response.StatusCode);
            using var document = JsonDocument.Parse(await ReadBoundedAsync(response.Content, 1024 * 1024, linked.Token));
            return document.RootElement.Clone();
        }
        finally { restLock.Release(); }
    }
    public async Task<JsonElement> UploadManyAsync(string channelId, IReadOnlyList<string> paths, string content, string replyId, string nonce, CancellationToken ct = default)
    {
        if (channelId.Length == 0 || !channelId.All(char.IsAsciiDigit) || paths.Count is < 1 or > 10 || nonce.Length is < 1 or > 25) throw new ArgumentException("Envío preparado inválido.");
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct, lifetime.Token);
        await restLock.WaitAsync(linked.Token);
        try
        {
            using var form = new MultipartFormDataContent();
            var body = new Dictionary<string, object?> { ["content"] = content, ["nonce"] = nonce, ["enforce_nonce"] = true, ["allowed_mentions"] = new { parse = new[] { "users", "roles" }, replied_user = false } };
            if (replyId.Length > 0) body["message_reference"] = new { message_id = replyId, channel_id = channelId, fail_if_not_exists = true };
            body["attachments"] = paths.Select((path, index) => new { id = index.ToString(), filename = Path.GetFileName(path) }).ToArray();
            form.Add(new StringContent(JsonSerializer.Serialize(body), Encoding.UTF8, "application/json"), "payload_json");
            long total = 0;
            foreach (var (path, index) in paths.Select((path, index) => (path, index)))
            {
                var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, 32768, FileOptions.Asynchronous | FileOptions.SequentialScan);
                total += stream.Length;
                if (stream.Length > 25 * 1024 * 1024 || total > 100 * 1024 * 1024) { stream.Dispose(); throw new ServiceException("Los adjuntos superan el límite de 25 MiB por archivo o 100 MiB por envío."); }
                form.Add(new StreamContent(stream), $"files[{index}]", Path.GetFileName(path));
            }
            using var request = new HttpRequestMessage(HttpMethod.Post, $"channels/{channelId}/messages") { Content = form };
            using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, linked.Token);
            if (!response.IsSuccessStatusCode) throw new ServiceException("Discord rechazó el envío preparado. El borrador se conserva; comprueba tamaño y permisos.", (int)response.StatusCode);
            using var document = JsonDocument.Parse(await ReadBoundedAsync(response.Content, 1024 * 1024, linked.Token)); return document.RootElement.Clone();
        }
        finally { restLock.Release(); }
    }

    private static async Task<byte[]> ReadBoundedAsync(HttpContent content, int maxBytes, CancellationToken ct)
    {
        if (content.Headers.ContentLength > maxBytes) throw new ServiceException("La respuesta supera el límite de memoria configurado.");
        using var stream = await content.ReadAsStreamAsync(ct);
        using var output = new MemoryStream();
        var buffer = new byte[32768];
        int n;
        while ((n = await stream.ReadAsync(buffer, ct)) > 0)
        {
            if (output.Length + n > maxBytes) throw new ServiceException("La respuesta supera el límite de memoria configurado.");
            output.Write(buffer, 0, n);
        }
        return output.ToArray();
    }

    public void StartGateway() => gatewayTask ??= Task.Run(GatewayLoopAsync);
    public Task UpdateVoiceAsync(string? guildId, string? channelId, bool mute = false, bool deaf = false, bool selfVideo = false) => GatewaySendAsync(new { op = 4, d = new { guild_id = guildId, channel_id = channelId, self_mute = mute, self_deaf = deaf, self_video = selfVideo } }, lifetime.Token);
    public Task CreateStreamAsync(string? guildId, string channelId) => GatewaySendAsync(new { op = 18, d = new { type = guildId == null ? "call" : "guild", guild_id = guildId, channel_id = channelId, preferred_region = (string?)null } }, lifetime.Token);
    public Task DeleteStreamAsync(string streamKey) => GatewaySendAsync(new { op = 19, d = new { stream_key = streamKey } }, lifetime.Token);
    public Task UnpauseStreamAsync(string streamKey) => GatewaySendAsync(new { op = 22, d = new { stream_key = streamKey, paused = false } }, lifetime.Token);
    public async Task SubscribeGuildAsync(string guildId, string channelId = "", int rangeEnd = 99)
    {
        if (guildId.Length > 0 && !guildId.All(char.IsAsciiDigit) || channelId.Length > 0 && !channelId.All(char.IsAsciiDigit)) throw new ArgumentException("Suscripción no válida.");
        var subscriptions = new Dictionary<string, object>();
        if (subscribedGuild.Length > 0 && subscribedGuild != guildId) subscriptions[subscribedGuild] = new { typing = false, activities = false, threads = false, channels = new Dictionary<string, int[][]>() };
        if (guildId.Length > 0) subscriptions[guildId] = new { typing = true, activities = true, threads = false,
            channels = channelId.Length == 0 ? new Dictionary<string, int[][]>() : new Dictionary<string, int[][]> { [channelId] = new[] { new[] { 0, Math.Clamp(rangeEnd, 99, 1999) } } } };
        if (subscriptions.Count > 0) await GatewaySendAsync(new { op = 37, d = new { subscriptions } }, lifetime.Token);
        subscribedGuild = guildId;
    }
    public async Task SetActivityAsync(string? activityName)
    {
        await presenceChangeLock.WaitAsync(lifetime.Token);
        try
        {
            string status = SelfPresence;
            if (!ValidPresence(status)) throw new ServiceException("No se conoce tu estado de presencia. Elige un estado en tu menú de usuario antes de publicar actividad.");
            await GatewaySendAsync(PresencePayload(status, ShowCurrentGame == false ? null : activityName), lifetime.Token);
            lock (presenceStateLock) ownActivityName = activityName;
        }
        finally { presenceChangeLock.Release(); }
    }
    public async Task SetPresenceAsync(string status)
    {
        if (!ValidPresence(status)) throw new ServiceException("El estado de presencia no es válido.");
        await presenceChangeLock.WaitAsync(lifetime.Token);
        try
        {
            string? activity; lock (presenceStateLock) activity = ownActivityName;
            await GatewaySendAsync(PresencePayload(status, ShowCurrentGame == false ? null : activity), lifetime.Token);
            lock (presenceStateLock) selfPresence = status;
        }
        finally { presenceChangeLock.Release(); }
    }
    private static bool ValidPresence(string status) => status is "online" or "idle" or "dnd" or "invisible";
    private static object PresencePayload(string status, string? activityName) => new { op = 3, d = new { since = 0, status, afk = false,
        activities = activityName == null ? Array.Empty<object>() : new object[] { new { name = activityName, type = 0 } } } };
    private void RememberSelfPresence(JsonElement settings)
    {
        string status = JsonString(settings, "status");
        if (ValidPresence(status)) lock (presenceStateLock) selfPresence = status;
    }
    public async Task InitializePresenceAsync(CancellationToken ct = default)
    {
        // Keep only the chosen status; settings payloads are never logged or retained.
        try { RememberSelfPresence(await GetAsync("users/@me/settings", ct)); }
        catch (ServiceException) { } // Unknown remains explicit until READY or a user selection.
        catch (Exception error) when (error is HttpRequestException or IOException or JsonException) { }
        catch (OperationCanceledException) when (!ct.IsCancellationRequested && !lifetime.IsCancellationRequested) { }
        try { await RefreshActivityVisibilityAsync(ct); }
        catch (Exception error) when (error is ServiceException or HttpRequestException or IOException or JsonException or ArgumentException) { }
        catch (OperationCanceledException) when (!ct.IsCancellationRequested && !lifetime.IsCancellationRequested) { }
    }
    public async Task RefreshActivityVisibilityAsync(CancellationToken ct = default)
    {
        var response = await GetAsync("users/@me/settings-proto/1", ct);
        if (!response.TryGetProperty("settings", out var settings) || settings.ValueKind != JsonValueKind.String) throw new ServiceException("Discord no devolvió el ajuste de visibilidad de actividad.");
        RememberProtoSettings(settings.GetString() ?? "", fullSettings: true);
    }
    public async Task SetActivityVisibilityAsync(bool visible, CancellationToken ct = default)
    {
        var result = await SendAsync(HttpMethod.Patch, "users/@me/settings-proto/1", new { settings = DiscordUserSettings.VisibilityDelta(visible) }, ct);
        if (result.TryGetProperty("out_of_date", out var outdated) && outdated.ValueKind == JsonValueKind.True)
            throw new ServiceException("Los ajustes cambiaron en otro cliente. Vuelve a intentarlo.");
        lock (presenceStateLock) showCurrentGame = null;
        await RefreshActivityVisibilityAsync(ct);
        if (ShowCurrentGame != visible) throw new ServiceException("Discord no confirmó el cambio de visibilidad. Se mantiene el estado leído del servidor.");
        string? activity; lock (presenceStateLock) activity = ownActivityName;
        if (activity != null) await SetActivityAsync(activity);
    }
    private void RememberProtoSettings(string encoded, bool fullSettings)
    {
        var values = DiscordUserSettings.Read(encoded, fullSettings);
        lock (presenceStateLock)
        {
            if (values.Status != null) selfPresence = values.Status;
            if (values.ShowCurrentGame.HasValue) showCurrentGame = values.ShowCurrentGame;
        }
    }
    private bool RememberProtoEvent(JsonElement data)
    {
        if (!data.TryGetProperty("settings", out var settings) || settings.ValueKind != JsonValueKind.Object || !settings.TryGetProperty("type", out var type) || type.ValueKind != JsonValueKind.Number || !type.TryGetInt32(out int kind) || kind != 1) return false;
        if (!settings.TryGetProperty("proto", out var proto) || proto.ValueKind != JsonValueKind.String) return false;
        bool partial = data.TryGetProperty("partial", out var p) && p.ValueKind == JsonValueKind.True;
        try { RememberProtoSettings(proto.GetString() ?? "", fullSettings: !partial); return true; }
        catch (ArgumentException) { return false; }
    }

    private static string JsonString(JsonElement value, string key) => value.ValueKind == JsonValueKind.Object && value.TryGetProperty(key, out var p) && p.ValueKind == JsonValueKind.String ? p.GetString() ?? "" : "";
    private static IEnumerable<JsonElement> JsonArray(JsonElement value) => value.ValueKind == JsonValueKind.Array ? value.EnumerateArray() : Enumerable.Empty<JsonElement>();
    private static JsonElement ReadyMetadata(JsonElement data)
    {
        var presences = data.TryGetProperty("presences", out var p) ? JsonArray(p) : Enumerable.Empty<JsonElement>();
        if (data.TryGetProperty("merged_presences", out var merged) && merged.ValueKind == JsonValueKind.Object)
        {
            if (merged.TryGetProperty("friends", out var friends)) presences = presences.Concat(JsonArray(friends));
            if (merged.TryGetProperty("guilds", out var guildPresences)) presences = presences.Concat(JsonArray(guildPresences).SelectMany(JsonArray));
        }
        var smallPresences = presences.Take(2000).Where(item => item.ValueKind == JsonValueKind.Object).Select(item => new
        {
            user = new { id = item.TryGetProperty("user", out var u) ? JsonString(u, "id") : JsonString(item, "user_id") },
            status = JsonString(item, "status"),
            activities = item.TryGetProperty("activities", out var activities) ? JsonArray(activities).Take(8).Where(a => a.ValueKind == JsonValueKind.Object).Select(a => new
            {
                name = SmallString(a, "name", 160), state = SmallString(a, "state", 300), details = SmallString(a, "details", 300), application_id = SmallString(a, "application_id", 24),
                type = a.TryGetProperty("type", out var t) && t.ValueKind == JsonValueKind.Number && t.TryGetInt32(out int number) ? number : 0,
                emoji = a.TryGetProperty("emoji", out var emoji) && emoji.ValueKind == JsonValueKind.Object ? new { name = SmallString(emoji, "name", 80), id = SmallString(emoji, "id", 24), animated = emoji.TryGetProperty("animated", out var animated) && animated.ValueKind == JsonValueKind.True } : null,
                assets = a.TryGetProperty("assets", out var assets) && assets.ValueKind == JsonValueKind.Object ? new { large_image = SmallString(assets, "large_image", 200) } : null,
                timestamps = a.TryGetProperty("timestamps", out var times) && times.ValueKind == JsonValueKind.Object ? new { start = SmallTimestamp(times, "start"), end = SmallTimestamp(times, "end") } : null
            }).ToArray() : Array.Empty<object>()
        }).ToArray();
        var settings = new List<object>(); int overrideBudget = 2000;
        if (data.TryGetProperty("user_guild_settings", out var source))
        {
            if (source.ValueKind == JsonValueKind.Object && source.TryGetProperty("entries", out var entries)) source = entries;
            foreach (var setting in JsonArray(source).Take(500))
            {
                var channelOverrides = setting.TryGetProperty("channel_overrides", out var overrides) ? JsonArray(overrides).Take(overrideBudget).Select(c => new
                { channel_id = JsonString(c, "channel_id"), muted = c.TryGetProperty("muted", out var cm) && cm.ValueKind == JsonValueKind.True }).ToArray() : Array.Empty<object>();
                overrideBudget -= channelOverrides.Length;
                settings.Add(new { guild_id = JsonString(setting, "guild_id"), muted = setting.TryGetProperty("muted", out var m) && m.ValueKind == JsonValueKind.True, channel_overrides = channelOverrides });
            }
        }
        var readStates = data.TryGetProperty("read_state", out var reads) ? reads : default;
        if (readStates.ValueKind == JsonValueKind.Object && readStates.TryGetProperty("entries", out var readEntries)) readStates = readEntries;
        var smallReadStates = JsonArray(readStates).Take(2000).Select(r => new { channel_id = JsonString(r, "id"), last_message_id = JsonString(r, "last_message_id"), mention_count = r.TryGetProperty("mention_count", out var mc) && mc.ValueKind == JsonValueKind.Number && mc.TryGetInt32(out var count) ? Math.Clamp(count, 0, 9999) : 0 }).ToArray();
        string selfStatus = data.TryGetProperty("user_settings", out var ownSettings) ? JsonString(ownSettings, "status") : "";
        return JsonSerializer.SerializeToElement(new { presences = smallPresences, settings, read_states = smallReadStates, self_status = ValidPresence(selfStatus) ? selfStatus : "" });
    }
    private static string SmallString(JsonElement value, string key, int max)
    { string text = JsonString(value, key); return text.Length > max ? text[..max] : text; }
    private static long? SmallTimestamp(JsonElement value, string key) => value.ValueKind == JsonValueKind.Object && value.TryGetProperty(key, out var property) && property.ValueKind == JsonValueKind.Number && property.TryGetInt64(out long number) && number >= 0 && number <= 253402300799999L ? number : null;
    private async Task EmitDispatchAsync(string type, JsonElement data)
    {
        var callbacks = Dispatch;
        if (callbacks != null)
            foreach (Func<string, JsonElement, Task> callback in callbacks.GetInvocationList()) await callback(type, data.Clone());
    }

    private async Task GatewayLoopAsync()
    {
        int failures = 0;
        while (!lifetime.IsCancellationRequested)
        {
            using var connection = CancellationTokenSource.CreateLinkedTokenSource(lifetime.Token);
            Task? heartbeats = null;
            bool fatal = false;
            using var ws = new ClientWebSocket();
            socket = ws;
            ws.Options.KeepAliveInterval = TimeSpan.Zero;
            try
            {
                StatusChanged?.Invoke("Conectando");
                await ws.ConnectAsync(gatewayUrl, lifetime.Token);
                var buffer = new byte[32768];
                while (ws.State == WebSocketState.Open && !connection.IsCancellationRequested)
                {
                    using var packet = new MemoryStream();
                    WebSocketReceiveResult part;
                    do
                    {
                        part = await ws.ReceiveAsync(buffer, connection.Token);
                        if (part.MessageType == WebSocketMessageType.Close)
                        {
                            int code = (int)(part.CloseStatus ?? 0);
                            fatal = code is 4004 or 4011 or 4013 or 4014;
                            if (code is 4007 or 4009) { sessionId = null; sequence = null; }
                            throw new WebSocketException("Conexión cerrada");
                        }
                        if (part.MessageType != WebSocketMessageType.Text || packet.Length + part.Count > 32 * 1024 * 1024) throw new ServiceException("Evento no compatible con el límite de memoria.");
                        packet.Write(buffer, 0, part.Count);
                    } while (!part.EndOfMessage);
                    using var document = JsonDocument.Parse(packet.GetBuffer().AsMemory(0, (int)packet.Length));
                    var root = document.RootElement;
                    if (root.TryGetProperty("s", out var seq) && seq.ValueKind == JsonValueKind.Number) sequence = seq.GetInt64();
                    int op = root.GetProperty("op").GetInt32();
                    var data = root.TryGetProperty("d", out var d) ? d : default;
                    if (op == 10)
                    {
                        heartbeatAck = true;
                        int interval = Math.Clamp(data.GetProperty("heartbeat_interval").GetInt32(), 1000, 120000);
                        heartbeats = HeartbeatLoopAsync(interval, ws, connection.Token);
                        if (sessionId != null)
                            await GatewaySendAsync(new { op = 6, d = new { token, session_id = sessionId, seq = sequence } }, connection.Token);
                        else
                        {
                            var identify = new Dictionary<string, object?> { ["token"] = token, ["capabilities"] = 0, ["compress"] = false,
                                ["properties"] = new { os = "Windows", browser = "NotDiscordTerminal", device = "NotDiscordTerminal", system_locale = "es-ES" } };
                            string knownStatus = SelfPresence;
                            if (ValidPresence(knownStatus)) identify["presence"] = new { status = knownStatus, since = 0, activities = Array.Empty<object>(), afk = false };
                            await GatewaySendAsync(new { op = 2, d = identify }, connection.Token);
                        }
                    }
                    else if (op == 11) heartbeatAck = true;
                    else if (op == 1) await GatewaySendAsync(new { op = 1, d = sequence }, connection.Token);
                    else if (op == 7) break;
                    else if (op == 9)
                    {
                        if (data.ValueKind != JsonValueKind.True) { sessionId = null; sequence = null; }
                        break;
                    }
                    else if (op == 0)
                    {
                        string type = root.GetProperty("t").GetString() ?? "";
                        if (type == "READY")
                        {
                            sessionId = data.GetProperty("session_id").GetString();
                            if (data.TryGetProperty("user_settings", out var ownSettings)) RememberSelfPresence(ownSettings);
                            if (JsonString(data, "user_settings_proto") is { Length: > 0 } protoSettings)
                                try { RememberProtoSettings(protoSettings, fullSettings: true); } catch (ArgumentException) { }
                            if (data.TryGetProperty("resume_gateway_url", out var resume) && Uri.TryCreate(resume.GetString(), UriKind.Absolute, out var address) && address.Scheme == "wss" && (address.Host == "discord.gg" || address.Host.EndsWith(".discord.gg", StringComparison.OrdinalIgnoreCase)))
                                gatewayUrl = new Uri(address.GetLeftPart(UriPartial.Path) + "?v=9&encoding=json");
                        }
                        if (type == "USER_SETTINGS_UPDATE") { RememberSelfPresence(data); await EmitDispatchAsync("SELF_PRESENCE", JsonSerializer.SerializeToElement(new { status = SelfPresence })); }
                        if (type == "USER_SETTINGS_PROTO_UPDATE")
                        {
                            if (RememberProtoEvent(data))
                            {
                                string? activity; lock (presenceStateLock) activity = ownActivityName;
                                if (activity != null) try { await SetActivityAsync(activity); } catch (ServiceException) { }
                                await EmitDispatchAsync("SELF_PRESENCE", JsonSerializer.SerializeToElement(new { status = SelfPresence }));
                            }
                        }
                        if (type is "READY" or "RESUMED") { failures = 0; StatusChanged?.Invoke("En línea"); }
                        if (type is "READY" or "READY_SUPPLEMENTAL") await EmitDispatchAsync("READY_METADATA", ReadyMetadata(data));
                        // Keep only events used by the native interface; READY can be very large.
                        if (type is "MESSAGE_CREATE" or "MESSAGE_UPDATE" or "MESSAGE_DELETE" or "MESSAGE_REACTION_ADD" or "MESSAGE_REACTION_REMOVE" or "MESSAGE_REACTION_REMOVE_ALL" or "MESSAGE_REACTION_REMOVE_EMOJI" or "RELATIONSHIP_ADD" or "RELATIONSHIP_UPDATE" or "RELATIONSHIP_REMOVE" or "CHANNEL_CREATE" or "CHANNEL_UPDATE" or "CHANNEL_DELETE" or "CHANNEL_RECIPIENT_ADD" or "CHANNEL_RECIPIENT_REMOVE" or "CHANNEL_PINS_UPDATE" or "VOICE_STATE_UPDATE" or "VOICE_SERVER_UPDATE" or "STREAM_CREATE" or "STREAM_SERVER_UPDATE" or "STREAM_DELETE" or "PRESENCE_UPDATE" or "TYPING_START" or "GUILD_MEMBER_LIST_UPDATE" or "GUILD_MEMBER_UPDATE" or "GUILD_ROLE_CREATE" or "GUILD_ROLE_UPDATE" or "GUILD_ROLE_DELETE" or "USER_GUILD_SETTINGS_UPDATE" or "MESSAGE_ACK" or "USER_UPDATE" or "CALL_CREATE" or "CALL_UPDATE" or "CALL_DELETE") await EmitDispatchAsync(type, data);
                    }
                }
            }
            catch (OperationCanceledException) { }
            catch (Exception ex) when (ex is WebSocketException or HttpRequestException or JsonException or ServiceException or InvalidOperationException)
            { StatusChanged?.Invoke(fatal ? "Sesión rechazada; vuelve a iniciar sesión" : "Reconectando"); }
            finally
            {
                connection.Cancel();
                if (heartbeats != null) try { await heartbeats; } catch (Exception e) when (e is OperationCanceledException or WebSocketException or ServiceException) { }
                socket = null;
            }
            if (fatal || lifetime.IsCancellationRequested) break;
            try { await Task.Delay(TimeSpan.FromSeconds(Math.Min(60, Math.Pow(2, Math.Min(++failures, 5))) + Random.Shared.NextDouble()), lifetime.Token); }
            catch (OperationCanceledException) { break; }
        }
    }

    private async Task HeartbeatLoopAsync(int interval, ClientWebSocket ws, CancellationToken ct)
    {
        try
        {
            await Task.Delay((int)(interval * Random.Shared.NextDouble()), ct);
            while (!ct.IsCancellationRequested && ws.State == WebSocketState.Open)
            {
                if (!heartbeatAck) { ws.Abort(); return; }
                heartbeatAck = false;
                await GatewaySendAsync(new { op = 1, d = sequence }, ct);
                await Task.Delay(interval, ct);
            }
        }
        catch (WebSocketException) { ws.Abort(); }
        catch (ServiceException) { ws.Abort(); }
        catch (OperationCanceledException) { }
    }

    private async Task GatewaySendAsync(object payload, CancellationToken ct)
    {
        await sendLock.WaitAsync(ct);
        try
        {
            if (socket?.State != WebSocketState.Open) throw new ServiceException("La conexión aún no está preparada.");
            var bytes = JsonSerializer.SerializeToUtf8Bytes(payload);
            await socket.SendAsync(bytes, WebSocketMessageType.Text, true, ct);
        }
        finally { sendLock.Release(); }
    }

    public async ValueTask DisposeAsync()
    {
        lifetime.Cancel(); socket?.Abort();
        if (gatewayTask != null) try { await gatewayTask; } catch (Exception) { }
        http.Dispose(); lifetime.Dispose();
    }
}
