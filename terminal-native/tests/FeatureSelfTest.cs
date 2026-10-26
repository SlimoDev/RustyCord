using System.Text;
using NotDiscord.Terminal;

public static class FeatureSelfTest
{
    public static IReadOnlyList<string> Run(string dataDirectory) => RunAsync(dataDirectory).GetAwaiter().GetResult();
    public static async Task<IReadOnlyList<string>> RunAsync(string dataDirectory)
    {
        var errors = new List<string>();
        void Check(bool condition, string description) { if (!condition) errors.Add(description); }
        string folder = Path.Combine(Path.GetFullPath(dataDirectory), "synthetic-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(folder);
        await using (var features = new NativeFeatures(folder))
        {
            Check(features.PrepareOutgoing("<:ok:123456> <a:wave:234567>") ==
                "https://cdn.discordapp.com/emojis/123456.png?size=48&name=ok https://cdn.discordapp.com/emojis/234567.gif?size=48&name=wave", "FakeNitro static/animated fallback");
            Check(features.PrepareOutgoing("<:ok:123456> <a:wave:234567>", id => id == "123456").StartsWith("<:ok:123456> https://"), "Usable native emoji preserved");
            Check(features.PrepareOutgoing("<:ñ:123456>").Contains("name=%C3%B1"), "Emoji name escaped");
            string code = "`<:ok:123456> https://test.invalid/?utm_source=x`\n```js\n<a:wave:234567>\n```";
            Check(features.PrepareOutgoing(code) == code, "Markdown code preserved");
            Check(features.PrepareOutgoing("`unfinished <:ok:123456>") == "`unfinished <:ok:123456>", "Unfinished code preserved");
            Check(features.PrepareOutgoing(@"\<:ok:123456>") == @"\<:ok:123456>", "Escaped emoji preserved");
            Check(features.PrepareOutgoing("<notemoji> @everyone :smile:") == "<notemoji> @everyone :smile:", "Ordinary text preserved");
            string rawUrl = "https://test.invalid/path?token=a%2Bb%3D&ex=abcd&is=efgh&hm=0123&utm_source=test&x=a+b#anchor";
            string cleaned = "https://test.invalid/path?token=a%2Bb%3D&ex=abcd&is=efgh&hm=0123&x=a+b#anchor";
            Check(features.PrepareOutgoing(rawUrl) == cleaned, "Auth-like query, escaping, ordering and fragment preserved");
            string signed = "https://test.invalid/?signature=xyz&utm_source=required&x=1";
            Check(features.PrepareOutgoing(signed) == signed, "Signed URL not modified");
            Check(features.PrepareOutgoing("https://test.invalid/?utm_source=x&gclid=y#keep") == "https://test.invalid/#keep", "Tracking-only query removed");
            string hashQuery = "https://test.invalid/#section?utm_source=keep";
            Check(features.PrepareOutgoing(hashQuery) == hashQuery, "Fragment query not stripped");
            Check(features.PrepareOutgoing("[link](https://test.invalid/?utm_source=x).") == "[link](https://test.invalid/).", "Markdown link boundaries and punctuation preserved");
            Check(features.PrepareOutgoing("https://test.invalid/path_(ok)?utm_source=x") == "https://test.invalid/path_(ok)", "Balanced parentheses in URL preserved");

            features.TogglePin("synthetic-channel");
            Check(features.IsPinned("synthetic-channel") && features.PinnedIds.Count == 1, "Pin toggled");
            await using (var reopened = new NativeFeatures(folder)) Check(reopened.IsPinned("synthetic-channel"), "Pin durable across instances");
            features.TogglePin("synthetic-channel");
            Check(!features.IsPinned("synthetic-channel"), "Pin removed");

            ChatMessage original = Message("1", "history", "synthetic-secret-old");
            ChatMessage edit = original with { Content = "synthetic-secret-new", Edited = true };
            await features.RecordChangeAsync(original, edit);
            await features.RecordChangeAsync(edit, null);
            var revisions = await features.ReadHistoryAsync("history");
            Check(revisions.Count == 2 && revisions[0].Content == original.Content && revisions[0].Edited
                && revisions[1].Content == edit.Content && revisions[1].Deleted, "Edit/deletion snapshots retained in order");
            Check((await features.ReadHistoryAsync("history", 1)).Single().Deleted, "History tail limit");
            await features.RecordChangeAsync(edit, edit);
            Check((await features.ReadHistoryAsync("history")).Count == 2, "No-op edit not journaled");
            Check((await features.ReadHistoryAsync("another-channel")).Count == 0, "Channel isolation");
            foreach (string file in Directory.EnumerateFiles(Path.Combine(folder, "message-history"), "*.ndj"))
                Check(!Encoding.UTF8.GetString(File.ReadAllBytes(file)).Contains("synthetic-secret"), "History content encrypted on disk");
            try { await features.RecordChangeAsync(original, edit with { Id = "different" }); errors.Add("Mismatched revision accepted"); }
            catch (ArgumentException) { }
            using var cancellation = new CancellationTokenSource(); cancellation.Cancel();
            try { await features.RecordChangeAsync(original, null, cancellation.Token); errors.Add("Cancelled write accepted"); }
            catch (OperationCanceledException) { }
            try { await features.RecordChangeAsync(Message("oversize", "history", new string('x', 1024 * 1024)), null); errors.Add("Oversized history entry accepted"); }
            catch (HistoryCapacityException) { }
            Check((await features.ReadHistoryAsync("history")).Count == 2, "Capacity error preserves existing history");
            var beforeRepairFiles = Directory.GetFiles(Path.Combine(folder, "message-history"), "*.ndj");
            await features.RecordChangeAsync(Message("1", "tail-repair", "synthetic-first"), null);
            string repairPath = Directory.GetFiles(Path.Combine(folder, "message-history"), "*.ndj").Except(beforeRepairFiles).Single();
            using (var tail = new FileStream(repairPath, FileMode.Append, FileAccess.Write)) tail.Write([1, 2]);
            await features.RecordChangeAsync(Message("2", "tail-repair", "synthetic-second"), null);
            Check((await features.ReadHistoryAsync("tail-repair")).Count == 2, "Interrupted trailing frame repaired without losing complete records");
            await Task.WhenAll(Enumerable.Range(0, 40).Select(i => features.RecordChangeAsync(Message(i.ToString(), "parallel", "synthetic-change"), null)));
            Check((await features.ReadHistoryAsync("parallel")).Count == 40, "Concurrent queued journal operations complete");
        }
        await using (var reopened = new NativeFeatures(folder))
            Check((await reopened.ReadHistoryAsync("history")).Count == 2, "History survives reopen");

        var cache = new MessageWindowCache(maxChannels: 3, maxMessages: 200, maxTextBytes: 4 * 1024 * 1024);
        cache.Replace("first", Enumerable.Range(0, 5000).Select(i => Message(i.ToString(), "first", "bounded")));
        Check(cache.Count == 200 && cache.Read("first").First().Id == "4800", "Long input stays bounded and retains newest window");
        cache.Upsert(Message("4999", "first", "edited"));
        Check(cache.Count == 200 && cache.Get("first", "4999")?.Content == "edited", "Upsert replaces without duplicate");
        Check(cache.Remove("first", "4999") && cache.Get("first", "4999") is null, "Remove updates lookup");
        cache.Clear();
        foreach (string channel in new[] { "a", "b", "c" }) cache.Upsert(Message("1", channel, "small"));
        cache.Read("a"); cache.Upsert(Message("1", "d", "small"));
        Check(cache.ChannelCount == 3 && cache.Read("b").Count == 0 && cache.Read("a").Count == 1, "LRU channel eviction");
        var small = new MessageWindowCache(maxChannels: 3, maxMessages: 200, maxTextBytes: 2048);
        foreach (int i in Enumerable.Range(0, 100)) small.Upsert(Message(i.ToString(), "text", new string('x', 300)));
        Check(small.EstimatedTextBytes <= 2048 && small.Count < 100, "Byte budget bounded");
        small.Upsert(Message("huge", "text", new string('z', 10000)));
        Check(small.Get("text", "huge") is null && small.EstimatedTextBytes <= 2048, "Oversized message not truncated or cached");
        var attachments = new List<ChatAttachment> { new("a", "synthetic", "https://example.invalid/test.png") };
        cache.Upsert(Message("copy", "copy", "immutable") with { Attachments = attachments }); attachments.Clear();
        Check(cache.Get("copy", "copy")?.Attachments.Count == 1, "Caller cannot mutate cached attachment list");
        await Task.WhenAll(Enumerable.Range(0, 6).Select(worker => Task.Run(() =>
        {
            for (int i = 0; i < 500; i++) { cache.Upsert(Message(i.ToString(), "worker-" + worker, "concurrent")); cache.Read("worker-" + worker); }
        })));
        Check(cache.Count <= 200 && cache.ChannelCount <= 3 && cache.EstimatedTextBytes <= 4 * 1024 * 1024, "Concurrent cache invariants");
        return errors;
    }

    private static ChatMessage Message(string id, string channel, string text) => new(id, channel,
        new Person("synthetic-user", "Synthetic user"), text,
        DateTimeOffset.UnixEpoch.AddSeconds(int.TryParse(id, out int value) ? value : 0), Array.Empty<ChatAttachment>());
}
