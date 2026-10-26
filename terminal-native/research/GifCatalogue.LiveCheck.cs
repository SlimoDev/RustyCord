using System.IO;
using System.Net.Http;
using System.Text.Json;
using NotDiscord.Terminal;

internal static class GifCatalogueLiveCheck
{
    static async Task<int> Main(string[] args)
    {
        // Explicitly authorized, read-only catalogue check. The credential remains
        // inside this short-lived process and is never printed or serialized.
        var checks = new List<object>(); bool credentialAvailable = false;
        try
        {
            string? credential = SecretVault.Load(args[0]); credentialAvailable = credential != null;
            if (credential != null)
            {
                var transport = new ErrorFieldsHandler();
                await using var session = new DiscordSession(credential, transport);
                var queries = args.Contains("--large-limits") ? new[] { new GifQuery("trending", Limit: 50), new GifQuery("trending", Limit: 100) } : new[] { new GifQuery("trending", Limit: 20), new GifQuery("search", "cat", 20), new GifQuery("categories", Limit: 20), new GifQuery("favorites", Limit: 20) };
                foreach (var query in queries.Where(q => !args.Contains("--only-search") || q.Mode is "search" or "trending"))
                {
                    using var cancellation = new CancellationTokenSource(TimeSpan.FromSeconds(40));
                    try
                    {
                        string endpoint = DiscordGifCatalog.Endpoint(query);
                        var response = await session.GetAsync(endpoint, cancellation.Token);
                        var page = DiscordGifCatalog.Parse(response, query);
                        string[] hosts = page.Items.SelectMany(g => new[] { g.Url, g.ThumbnailUrl }).Concat(page.Categories?.Select(c => c.ThumbnailUrl) ?? Enumerable.Empty<string>())
                            .Where(url => Uri.TryCreate(url, UriKind.Absolute, out _)).Select(url => new Uri(url).Host).Distinct().Order().Take(12).ToArray();
                        var extensions = page.Items.Select(g => Uri.TryCreate(g.ThumbnailUrl, UriKind.Absolute,out var uri) ? Path.GetExtension(uri.AbsolutePath).ToLowerInvariant() : "").GroupBy(ext => ext).ToDictionary(g => g.Key,g => g.Count());
                        checks.Add(new { mode = query.Mode, requestedLimit = query.Limit, success = true, httpStatus = 200, itemCount = page.Items.Count, categoryCount = page.Categories?.Count ?? 0, gifThumbnailCount = page.Items.Count(g => GifSources.IsDirectGif(g.ThumbnailUrl)), thumbnailExtensions = extensions, hosts });
                    }
                    catch (ServiceException error) { checks.Add(new { mode = query.Mode, requestedLimit = query.Limit, success = false, httpStatus = error.Status, errorCode = transport.Code, invalidFields = transport.InvalidFields }); }
                    catch (Exception error) when (error is HttpRequestException or IOException or JsonException or ArgumentException or OperationCanceledException)
                    {
                        string localReason = error is ArgumentException && new[] { "Los guardados superan el límite permitido.", "Guardados no válidos.", "Demasiados campos de guardados.", "Campo de guardados no válido.", "Formato de guardados no compatible.", "Guardados truncados.", "Número de guardados no válido." }.Contains(error.Message) ? error.Message : "";
                        checks.Add(new { mode = query.Mode, success = false, category = error is OperationCanceledException ? "timeout" : "transport-or-parse", localParserReason = localReason });
                    }
                }
            }
        }
        catch (Exception) { checks.Add(new { success = false, category = "local-check-unavailable" }); }
        string clientAssemblySha256 = Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(File.ReadAllBytes(typeof(DiscordGifCatalog).Assembly.Location)));
        string output = JsonSerializer.Serialize(new { checkedAtUtc = DateTimeOffset.UtcNow, clientAssemblySha256, credentialAvailable, checks, accountContentRecorded = false, credentialRecorded = false, gatewayStarted = false, externalWrites = false }, new JsonSerializerOptions { WriteIndented = true });
        File.WriteAllText(args[1], output); Console.WriteLine(output); return 0;
    }
    private sealed class ErrorFieldsHandler : DelegatingHandler
    {
        public int? Code; public Dictionary<string, string[]> InvalidFields = new();
        public ErrorFieldsHandler() : base(new HttpClientHandler { AutomaticDecompression = System.Net.DecompressionMethods.All, AllowAutoRedirect = false }) { }
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellation)
        {
            Code = null; InvalidFields.Clear(); var response = await base.SendAsync(request, cancellation);
            if (!response.IsSuccessStatusCode && response.Content.Headers.ContentLength is not > 65536)
            {
                try
                {
                    using var body = JsonDocument.Parse(await response.Content.ReadAsByteArrayAsync(cancellation));
                    if (body.RootElement.TryGetProperty("code", out var code) && code.TryGetInt32(out int number)) Code = number;
                    if (body.RootElement.TryGetProperty("errors", out var errors) && errors.ValueKind == JsonValueKind.Object) foreach (var field in errors.EnumerateObject().Take(20))
                        if (field.Name.Length <= 80 && field.Name.All(c => char.IsAsciiLetterOrDigit(c) || c == '_'))
                            InvalidFields[field.Name] = field.Value.ValueKind == JsonValueKind.Object && field.Value.TryGetProperty("_errors", out var details) && details.ValueKind == JsonValueKind.Array
                                ? details.EnumerateArray().Take(6).Select(error => error.TryGetProperty("code", out var errorCode) ? errorCode.GetString() ?? "" : "").Where(value => value.Length <= 80 && value.All(c => char.IsAsciiLetterOrDigit(c) || c == '_')).ToArray() : Array.Empty<string>();
                }
                catch (Exception error) when (error is JsonException or InvalidOperationException) { }
            }
            return response;
        }
    }
}
