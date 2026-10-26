namespace NotDiscord;

/// <summary>
/// Defensive network rules derived from the retained public Discord web bundle.
/// This policy inspects only the request URI and method; it never reads request
/// bodies, headers, credentials, or responses. No rule means allow unchanged.
/// Evidence: evidence/public-web/telemetry-rules.json.
/// </summary>
internal static class TelemetryPolicy
{
    // WebView2 URI patterns are a coarse prefilter. IsBlocked must always make
    // the final decision because these patterns also admit suffixes and queries.
    internal static IReadOnlyList<string> Filters { get; } = Array.AsReadOnly(new[]
    {
        "https://discord.com/api/science*",
        "https://discord.com/api/v*/science*",
        "https://discord.com/api/metrics*",
        "https://discord.com/api/v*/metrics*",
        "https://discord.com/error-reporting-proxy/web*"
    });

    internal static bool IsBlocked(string? url, string? method) => GetBlockedRule(url, method) != null;

    // Returns fixed categories suitable for aggregate counters, never the URL.
    internal static string? GetBlockedRule(string? url, string? method)
    {
        if (!string.Equals(method, "POST", StringComparison.Ordinal) ||
            !Uri.TryCreate(url, UriKind.Absolute, out var uri) ||
            uri.Scheme != Uri.UriSchemeHttps || uri.Port != 443 ||
            !uri.Host.Equals("discord.com", StringComparison.OrdinalIgnoreCase) ||
            !string.IsNullOrEmpty(uri.UserInfo)) return null;

        string path = uri.AbsolutePath;
        if (path == "/error-reporting-proxy/web") return "sentry-web";
        if (!path.StartsWith("/api/", StringComparison.Ordinal)) return null;
        string endpoint = path[5..];
        if (endpoint.StartsWith('v'))
        {
            int slash = endpoint.IndexOf('/');
            if (slash <= 1) return null;
            for (int i = 1; i < slash; i++)
                if (endpoint[i] < '0' || endpoint[i] > '9') return null;
            endpoint = endpoint[(slash + 1)..];
        }

        return endpoint switch
        {
            "science" => "science",
            "metrics" => "metrics",
            "metrics/v2" => "metrics-v2",
            _ => null
        };
    }

    internal static IReadOnlyDictionary<string, bool> SelfTestResults() => new Dictionary<string, bool>
    {
        ["telemetryScienceUnversioned"] = IsBlocked("https://discord.com/api/science", "POST"),
        ["telemetryScienceVersioned"] = IsBlocked("https://discord.com/api/v9/science", "POST"),
        ["telemetryMetrics"] = IsBlocked("https://discord.com/api/v10/metrics", "POST"),
        ["telemetryMetricsV2"] = IsBlocked("https://discord.com/api/v9/metrics/v2", "POST"),
        ["telemetryMetricsV2Unversioned"] = IsBlocked("https://discord.com/api/metrics/v2", "POST"),
        ["telemetryIgnoresQueryValues"] = IsBlocked("https://discord.com/api/v9/science?test=1", "POST"),
        ["telemetryAcceptsCanonicalOrigin"] = IsBlocked("https://DISCORD.COM:443/api/v9/science", "POST"),
        ["telemetryRejectsNull"] = !IsBlocked(null, "POST") && !IsBlocked("https://discord.com/api/science", null),
        ["telemetryAllowsGet"] = !IsBlocked("https://discord.com/api/v9/science", "GET"),
        ["telemetryAllowsOptions"] = !IsBlocked("https://discord.com/api/v9/science", "OPTIONS"),
        ["telemetryAllowsPut"] = !IsBlocked("https://discord.com/api/v9/science", "PUT"),
        ["telemetryDoesNotMatchHttp"] = !IsBlocked("http://discord.com/api/v9/science", "POST"),
        ["telemetryDoesNotMatchWrongPort"] = !IsBlocked("https://discord.com:8443/api/v9/science", "POST"),
        ["telemetryDoesNotMatchUserInfo"] = !IsBlocked("https://name@discord.com/api/v9/science", "POST"),
        ["telemetryDoesNotMatchSuffixDomain"] = !IsBlocked("https://discord.com.example.test/api/v9/science", "POST"),
        ["telemetryDoesNotMatchSubdomain"] = !IsBlocked("https://cdn.discord.com/api/v9/science", "POST"),
        ["telemetryDoesNotMatchArbitraryVersion"] = !IsBlocked("https://discord.com/api/version/science", "POST"),
        ["telemetryDoesNotMatchMissingVersion"] = !IsBlocked("https://discord.com/api/v/science", "POST"),
        ["telemetryDoesNotMatchSuffixPath"] = !IsBlocked("https://discord.com/api/v9/science-extra", "POST"),
        ["telemetryDoesNotMatchNestedPath"] = !IsBlocked("https://discord.com/api/v9/metrics/v2/extra", "POST"),
        ["telemetryDoesNotMatchRootPath"] = !IsBlocked("https://discord.com/science", "POST"),
        ["telemetryDoesNotMatchQueryOnly"] = !IsBlocked("https://discord.com/api/v9/other?path=/science", "POST"),
        ["telemetryAllowsMessages"] = !IsBlocked("https://discord.com/api/v9/channels/1/messages", "POST"),
        ["telemetryAllowsAttachments"] = !IsBlocked("https://discord.com/api/v9/channels/1/attachments", "POST"),
        ["telemetryAllowsVoiceState"] = !IsBlocked("https://discord.com/api/v9/guilds/1/voice-states/@me", "POST"),
        ["telemetryAllowsGateway"] = !IsBlocked("wss://gateway.discord.gg/", "POST"),
        ["telemetryAllowsUploadHost"] = !IsBlocked("https://discord-attachments-uploads-prd.storage.googleapis.com/example", "POST"),
        ["telemetryAllowsCdn"] = !IsBlocked("https://cdn.discordapp.com/attachments/1/example", "POST"),
        ["telemetryDoesNotBlockCommerce"] = !IsBlocked("https://discord.com/api/v9/collectibles-shop", "POST"),
        ["telemetryDoesNotBlockSentry"] = !IsBlocked("https://o64374.ingest.sentry.io/api/146342/envelope/", "POST"),
        ["telemetryCounterHasFixedCategory"] = GetBlockedRule("https://discord.com/api/v9/metrics/v2?test=1", "POST") == "metrics-v2",
        ["telemetrySentryWebTunnel"] = IsBlocked("https://discord.com/error-reporting-proxy/web", "POST"),
        ["telemetrySentryWebTunnelQuery"] = IsBlocked("https://discord.com/error-reporting-proxy/web?test=1", "POST"),
        ["telemetrySentryCounterHasFixedCategory"] = GetBlockedRule("https://discord.com/error-reporting-proxy/web", "POST") == "sentry-web",
        ["telemetrySentryDoesNotMatchGet"] = !IsBlocked("https://discord.com/error-reporting-proxy/web", "GET"),
        ["telemetrySentryDoesNotMatchOptions"] = !IsBlocked("https://discord.com/error-reporting-proxy/web", "OPTIONS"),
        ["telemetrySentryDoesNotMatchSuffix"] = !IsBlocked("https://discord.com/error-reporting-proxy/web-extra", "POST"),
        ["telemetrySentryDoesNotMatchNested"] = !IsBlocked("https://discord.com/error-reporting-proxy/web/extra", "POST"),
        ["telemetrySentryDoesNotMatchOtherTunnel"] = !IsBlocked("https://discord.com/error-reporting-proxy/native", "POST"),
        ["telemetrySentryDoesNotMatchOtherHost"] = !IsBlocked("https://discord.com.example.test/error-reporting-proxy/web", "POST")
    };
}