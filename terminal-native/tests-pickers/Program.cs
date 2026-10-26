using System.Text;
using System.Text.Json;
using NotDiscord.Terminal;

int passed = 0;
void Check(bool value, string name) { if (!value) throw new Exception(name); passed++; }
void Reject(Action action, string name) { try { action(); } catch (ArgumentException) { passed++; return; } throw new Exception(name); }
JsonElement Json(string text) { using var json = JsonDocument.Parse(text); return json.RootElement.Clone(); }
byte[] StringField(int field, string text) => Blob(field, Encoding.UTF8.GetBytes(text));
byte[] Blob(int field, byte[] bytes) => Varint((ulong)((field << 3) | 2)).Concat(Varint((ulong)bytes.Length)).Concat(bytes).ToArray();
byte[] Varint(ulong n) { var result = new List<byte>(); do { byte b = (byte)(n & 127); n >>= 7; result.Add((byte)(b | (n == 0 ? 0 : 128))); } while (n > 0); return result.ToArray(); }

Check(EmojiCatalog.All.Count == 3781, "Full qualified Unicode catalog");
Check(EmojiCatalog.All.Select(e => e.Value).Distinct().Count() == 3781, "No duplicate emoji sequences");
Check(EmojiCatalog.Search("", "corazon").Any(e => e.Value == "❤️"), "Spanish accent-independent keywords");
Check(EmojiCatalog.Search("", "heart").Any(e => e.Value == "❤️"), "English search");
Check(EmojiCatalog.Search("Flags", "").All(e => e.Category == "Flags"), "Category filter");
Check(EmojiCatalog.All.Any(e => e.Value == "👨‍👩‍👧‍👦"), "ZWJ sequence retained");
Check(EmojiCatalog.All.Any(e => e.Value == "👍🏽"), "Skin tone retained");
Check(DiscordGifCatalog.Endpoint(new("search", "cat & dog?")).StartsWith("gifs/search?q=cat%20%26%20dog%3F&"), "Query encoding");
Check(DiscordGifCatalog.Endpoint(new("trending", Limit: 100000)).EndsWith("limit=100"), "Server uses verified catalogue size while native results remain capped");
Check(new[] { -1, 1, 12, 20, 24, 44, 48, 50, 68, 72, 92, 96 }.Select(DiscordGifCatalog.RemoteLimit).SequenceEqual(new[] { 20, 20, 20, 20, 50, 50, 50, 50, 100, 100, 100, 100 }), "Arbitrary UI limits never send unsupported server sizes");
Check(new[] { DiscordGifCatalog.NextLimit(20), DiscordGifCatalog.NextLimit(50), DiscordGifCatalog.NextLimit(96) }.SequenceEqual(new[] { 50, 96, 96 }), "Load more follows verified sizes and stops at bounded display cap");
var cachedGifPage = new GifPage(Enumerable.Range(0, 50).Select(i => new GifItem(i.ToString(), "https://klipy.com/gifs/example-" + i, "")).ToArray(), HasMore: true);
Check(DiscordGifCatalog.ForDisplay(cachedGifPage, new(Limit:24)).Items.Count == 24 && DiscordGifCatalog.ForDisplay(cachedGifPage, new(Limit:48)).Items.Count == 48, "Normalized-size cache retains all results for later local growth");
Check(DiscordGifCatalog.ForDisplay(cachedGifPage, new(Limit:48)).HasMore && !DiscordGifCatalog.ForDisplay(new GifPage(Enumerable.Range(0,96).Select(i => new GifItem(i.ToString(), "https://klipy.com/gifs/x", "")).ToArray()), new(Limit:96)).HasMore, "Has more respects cached remainder and native maximum");
Reject(() => DiscordGifCatalog.Endpoint(new("invalid")), "Unknown API route rejected");
Reject(() => DiscordGifCatalog.Endpoint(new("search", new string('x', 151))), "Long search rejected");
foreach (string bad in new[] { "http://tenor.com/view/x", "https://evil.test/x.gif", "https://tenor.com:123/x", "https://x@tenor.com/view/x", "https://tenor.com.evil.test/x" }) Check(!DiscordGifCatalog.SafeLink(bad), "Safe source: " + bad);
var page = DiscordGifCatalog.Parse(Json("""[{"id":"1","url":"https://tenor.com/view/x-1","src":"https://media.tenor.com/x/g.gif","preview":"https://media.tenor.com/x/p.png","width":120,"height":90},{"url":"https://evil.test/x.gif","src":"https://evil.test/x.gif"}]"""), new());
Check(page.Items.Count == 1 && page.Items[0].Width == 120, "API parses safe result only");
var categories = DiscordGifCatalog.Parse(Json("""{"categories":[{"name":"Cats","src":"https://media.tenor.com/p.png"}],"gifs":[]}"""), new("categories"));
Check(categories.Categories?.Single().Name == "Cats", "Trending categories");
var movie = DiscordGifCatalog.Parse(Json("""[{"url":"https://tenor.com/view/x-1","src":"https://media.tenor.com/x/p.mp4","width":"not-a-number"}]"""), new());
Check(movie.Items.Count == 1 && movie.Items[0].ThumbnailUrl == movie.Items[0].Url, "Movie source uses safe share-page poster resolution instead of attempting video thumbnail decoding");
var klipy = DiscordGifCatalog.Parse(Json("""[{"id":"k1","url":"https://klipy.com/gifs/cat-example","gif_src":"https://static.klipy.com/ii/test/cat.gif","src":"https://static.klipy.com/ii/test/cat.webm"},{"url":"https://klipy.com/gifs/another","src":"//static1.klipy.com/ii/test/poster.gif"}]"""), new("search", "cat"));
Check(klipy.Items.Count == 2 && klipy.Items[0].ThumbnailUrl.EndsWith("cat.gif") && klipy.Items[1].ThumbnailUrl.StartsWith("https://static1.klipy.com/"), "current Discord Klipy provider and protocol-relative source remain visible");
Check(!DiscordGifCatalog.SafeLink("https://static.klipy.com.evil.test/a.gif") && !DiscordGifCatalog.SafeLink("https://api.klipy.com/v2/search"), "Klipy media support cannot broaden downloads to arbitrary hosts or provider API");
string url = "https://tenor.com/view/cat-123", src = "https://media.tenor.com/cat/g.gif";
byte[] favorite = StringField(2, src).Concat(new byte[] { 40, 9 }).ToArray();
byte[] entry = StringField(1, url).Concat(Blob(2, favorite)).ToArray();
byte[] settings = Blob(2, Blob(1, entry)).Concat(new byte[] { 120, 1 }).ToArray();
var saved = DiscordGifCatalog.ReadFavorites(Convert.ToBase64String(settings));
Check(saved.Count == 1 && saved[0].Url == url && saved[0].ThumbnailUrl == src, "Real observed protobuf shape and unknown field skip");
string largeSettings = Convert.ToBase64String(settings.Concat(Blob(10, new byte[5000])).ToArray());
Check(DiscordGifCatalog.Parse(JsonSerializer.SerializeToElement(new { settings = largeSettings }), new("favorites")).Items.Count == 1 && largeSettings.Length > 4096, "Full base64 account settings must not be truncated to the URL field limit");
Reject(() => DiscordGifCatalog.ReadFavorites("not base64"), "Malformed base64 rejected");
Reject(() => DiscordGifCatalog.ReadFavorites(Convert.ToBase64String(new byte[] { 18, 99, 1 })), "Truncated protobuf rejected");
Reject(() => DiscordGifCatalog.ReadFavorites(new string('a', 1_400_001)), "Oversized protobuf rejected");
string directory = Path.Combine(Path.GetTempPath(), "NotDiscord.PickerTests." + Guid.NewGuid().ToString("N"));
try
{
    var history = new PickerHistory(directory, "account-a"); var value = new PickerValue(url, "cat", src);
    history.FavoriteGif(value); history.Recent(value, true);
    var reloaded = new PickerHistory(directory, "account-a"); Check(reloaded.Data.GifFavorites.Single().Value == url && reloaded.Data.GifRecent.Count == 1, "Per-account persistence");
    Check(new PickerHistory(directory, "account-b").Data.GifFavorites.Count == 0, "Account isolation");
    Check(!reloaded.FavoriteGif(value) && reloaded.Data.GifFavorites.Count == 0, "GIF favorite toggle retained");
    for (int i = 0; i < 140; i++) reloaded.Recent(new("😀" + i, "face"), false);
    Check(reloaded.Data.EmojiRecent.Count == 10 && reloaded.Data.EmojiRecent[0].Value == "😀139", "Emoji recents cap10 in MRU order");
    reloaded.Recent(new("😀135", "face"), false);
    Check(reloaded.Data.EmojiRecent.Count == 10 && reloaded.Data.EmojiRecent[0].Value == "😀135" && reloaded.Data.EmojiRecent.Select(v => v.Value).Distinct().Count() == 10, "Emoji recent reuse promotes and deduplicates");
    var custom = new PickerValue("<a:wave:123>", "wave", "https://cdn.discordapp.com/emojis/123.png");
    reloaded.Recent(custom, false); reloaded.Recent(new("😀135", "face"), false);
    var mixed = new PickerHistory(directory, "account-a");
    Check(mixed.Data.EmojiRecent.Count == 10 && mixed.Data.EmojiRecent[0].Value == "😀135" && mixed.Data.EmojiRecent[1] == custom, "Unicode and server recents share one persisted MRU list");
    string accountFile = Directory.GetFiles(Path.Combine(directory, "pickers"), "*.json").Single();
    File.WriteAllText(accountFile, JsonSerializer.Serialize(new { EmojiFavorites = new[] { new PickerValue("❤️", "heart") }, EmojiRecent = mixed.Data.EmojiRecent.Concat(mixed.Data.EmojiRecent), GifFavorites = new[] { value }, GifRecent = new[] { value } }));
    var migrated = new PickerHistory(directory, "account-a");
    Check(migrated.Data.EmojiRecent.Count == 10 && typeof(PickerHistory.Values).GetProperty("EmojiFavorites") == null, "Legacy emoji favorites ignored and duplicate recents trimmed");
    migrated.Recent(new("👍", "thumb"), false);
    using (var stored = JsonDocument.Parse(File.ReadAllText(accountFile))) Check(!stored.RootElement.TryGetProperty("EmojiFavorites", out _) && stored.RootElement.GetProperty("GifFavorites").GetArrayLength() == 1, "Next save removes legacy emoji favorites without losing GIF favorites");
    Check(!PickerHistory.Valid(new("https://evil.test/x.gif", "invalid"), true), "Local history URL validation");
    var imports = new PickerHistory(directory, "import-account");
    var imported = imports.ImportGifLinks("https://klipy.com/gifs/cat-example\nhttps://static2.klipy.com/ii/cat.gif\nhttps://klipy.com/gifs/cat-example\nhttps://evil.test/cat.gif\nhttps://cdn.discordapp.com/avatars/1/photo.png");
    Check(imported == new GifImportResult(2, 1, 2) && imports.Data.GifFavorites.Count == 2, "GIF link import is deduplicated, provider-scoped and rejects non-GIF images");
    Check(new PickerHistory(directory, "import-account").Data.GifFavorites.Count == 2, "Imported GIF links persist only in local account history");
    Reject(() => imports.ImportGifLinks(new string('a', 256 * 1024 + 1)), "Oversized import list rejected before processing");
}
finally { if (Directory.Exists(directory)) Directory.Delete(directory, true); }
Console.WriteLine($"Picker tests: {passed} passed. No account, network or media devices used.");
