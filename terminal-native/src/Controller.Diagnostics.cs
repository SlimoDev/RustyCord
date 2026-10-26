namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private string? lastMediaFailureCode;
    private int? lastMediaExitCode;
    private string? lastMediaStage;
    private void RememberMediaStage(System.Text.Json.JsonElement data)
    {
        string stage = Str(data, "stage"), kind = Str(data, "kind");
        if (stage.Length is > 0 and <= 64 && stage.All(c => c is >= 'a' and <= 'z' or '-') && kind is "native" or "voice" or "share") lastMediaStage = kind + ":" + stage;
    }
    private void RememberMediaFailure(System.Text.Json.JsonElement data)
    {
        string code = Str(data, "reasonCode");
        if (code.Length == 0 && data.TryGetProperty("error", out var error)) code = Str(error, "code");
        // Diagnostic codes have a restricted alphabet, never arbitrary native text.
        if (code.Length == 0 || code.Length > 80 || code.Any(c => !(c is >= 'A' and <= 'Z' or >= '0' and <= '9' or '_'))) code = "MEDIA_UNKNOWN_FAILURE";
        lastMediaFailureCode = code;
        lastMediaExitCode = data.TryGetProperty("exitCode", out var value) && value.ValueKind == System.Text.Json.JsonValueKind.Number && value.TryGetInt32(out int exitCode) ? exitCode : null;
    }
    // One explicit diagnostic sample, never an audio/capture or connection command.
    public async Task<object> MediaStatisticsAsync()
    {
        var own = media;
        if (own == null) return new { sanitized = true, sampledAtUtc = DateTimeOffset.UtcNow, active = false };
        return await own.CommandAsync("stats");
    }
    // Aggregate local readiness only, opt-in through --diagnostics. No tokens,
    // user/channel identifiers, window titles, or message text are serialized.
    public object Readiness() => new
    {
        utc = DateTimeOffset.UtcNow,
        variant = "terminal-native-wpf",
        authenticated = state.SignedIn,
        gatewayConnected = state.Connection == "En línea",
        channelSelected = currentChannel.Length > 0,
        loadedMessages = state.Messages.Count,
        knownPresences = presences.Count,
        directMessagesWithPresence = state.DirectMessages.Count(n => n.Presence.Length > 0),
        groupIconsLoaded = state.DirectMessages.Count(n => n.Kind == "group" && n.IconUrl?.Contains("/channel-icons/", StringComparison.Ordinal) == true),
        callLogEntriesLoaded = state.Messages.Count(m => m.MessageType == 3),
        animatedMediaEntriesLoaded = state.Messages.Sum(m => m.Attachments.Count(a => a.Animated)),
        activityVisibilityKnown = state.GameActivityAvailable,
        cachedMessages = cache.Count,
        cachedChannels = cache.ChannelCount,
        cacheEstimatedTextBytes = cache.EstimatedTextBytes,
        voiceTransportConnected = voiceConnected,
        voiceEncryptionReady,
        screenCaptureRequested = sharing,
        screenCapturePaused = state.ScreenPaused,
        mediaProcessId = media?.ProcessId,
        lastMediaFailureCode,
        lastMediaExitCode,
        lastMediaStage,
        liveAudioVerified = false,
        liveScreenVerified = false,
        acceptance25Percent = false
    };
}
