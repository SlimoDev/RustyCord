using NotDiscord.Terminal;

int checks = 0;
void Check(bool value, string name) { checks++; if (!value) throw new Exception(name); }
var context = new SearchContext("100", new[] { new Person("101", "Ana", Username: "ana_example"), new Person("102", "Bright Person", Username: "bright_example") },
    new[] { new NavItem("201", "general", "text"), new NavItem("202", "arte y fotos", "thread"), new NavItem("203", "Voz", "voice") });
var defaults = SearchCompletion.Suggest("hello", 5, context);
Check(defaults.Select(s => s.Key).SequenceEqual(new[] { "from", "has", "mentions", "in", "before", "after", "during", "pinned" }), "Discord-style default filter list");
Check(SearchCompletion.At("hello from:br", 13) is { Key: "from", Partial: "br" }, "Active partial token");
Check(SearchCompletion.Suggest("from:br", 7, context).Single().Value == "102", "Suggest known display name and username");
Check(SearchCompletion.Suggest("mentions:ana", 12, context).Single().Value == "101", "Mentions suggestions use known people");
Check(SearchCompletion.Suggest("has:im", 6, context).Single().Value == "image", "Specific has type suggestions");
Check(SearchCompletion.Suggest("in:", 3, context).Count == 2, "Channel suggestions exclude voice channels");
Check(SearchCompletion.Suggest("pinned:", 7, context).Select(s => s.Value).SequenceEqual(new[] { "true", "false" }), "Pinned true and false suggestions");
Check(SearchCompletion.Suggest("during:", 7, context, new(2026, 10, 19)).Select(s => s.Value).SequenceEqual(new[] { "2026-10-19", "2026-10", "2026" }), "Day/month/year suggestions");
Check(SearchCompletion.Suggest("from:unknown", 12, context).Count == 0, "No network-backed unknown-user guesses");
Check(SearchCompletion.At(":from br", 8) is { Key: "from", Partial: "br" }, "Leading-colon alias completion");
var empty = SearchCompletion.Complete("hello", 5, "from", "");
Check(empty.Text == "hello from:" && empty.Caret == empty.Text.Length, "Clicking a filter starts a token at the caret");
var next = SearchCompletion.Complete(empty.Text, empty.Caret, "from", "101");
Check(next.Text == "hello from:101 ", "Selecting a value completes one token");
string input = "hello from:br has:image";
var middle = SearchCompletion.Complete(input, input.IndexOf("br", StringComparison.Ordinal) + 2, "from", "102");
Check(middle.Text == "hello from:102 has:image", "Mid-query completion preserves following filters and text");
string quoted = "from:\"Bright Pe\" has:link";
var quoteResult = SearchCompletion.Complete(quoted, quoted.IndexOf("Pe", StringComparison.Ordinal) + 2, "from", "102");
Check(quoteResult.Text == "from:102 has:link", "Quoted partial replacement consumes only the active token");
var channel = SearchCompletion.Complete("topic in:", 9, "in", "arte y fotos");
Check(channel.Text == "topic in:\"arte y fotos\" ", "Names containing spaces are quoted");
var repeated = SearchCompletion.Complete("has:image has:", 14, "has", "link");
Check(repeated.Text == "has:image has:link ", "Repeated has filters are preserved");
var parsed = SearchQuery.Parse("text from:Ana has:image mentions:me in:general before:2026-10-20 after:2026-10-01 pinned:true", context);
Check(parsed.Content == "text" && parsed.Filters.Count == 7, "Combining all common filter families");
Check(parsed.Filters.Contains(new("author_id", "101")) && parsed.Filters.Contains(new("mentions", "100")) && parsed.Filters.Contains(new("channel_id", "201")), "Resolved user/mention/channel IDs");
var aliases = SearchQuery.Parse(":from Ana :has image topic", context);
Check(aliases.Content == "topic" && aliases.Filters.Contains(new("author_id", "101")) && aliases.Filters.Contains(new("has", "image")), "Leading-colon aliases normalize to Discord syntax");
Check(SearchQuery.Parse("https://example.invalid/a:b", context).Content == "https://example.invalid/a:b", "Ordinary URLs are not rewritten");
Console.WriteLine($"Search completion/parser: {checks} assertions passed; no account or network used.");
