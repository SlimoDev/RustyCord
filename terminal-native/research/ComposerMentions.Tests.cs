using System.Text.Json;
using NotDiscord.Terminal;

internal static class ComposerMentionsTests
{
    static int Main()
    {
        int checks = 0; var errors = new List<string>(); void Check(bool value, string name) { checks++; if (!value) errors.Add(name); }
        Check(ComposerMentionText.FindQuery("@", 1) is { Start: 0, Length: 1, Query: "" }, "An initial at sign opens the current conversation candidates");
        Check(ComposerMentionText.FindQuery("Hola @ali, después", 9) is { Start: 5, Length: 4, Query: "ali" }, "Query detection is limited to the token at the caret");
        Check(ComposerMentionText.FindQuery("email@example", 13) == null && ComposerMentionText.FindQuery("<@123>", 5) == null, "Email addresses and existing transport mentions do not trigger suggestions");
        Check(ComposerMentionText.FindQuery("@alice hola", 11) == null && ComposerMentionText.FindQuery("@alice", 6, 1) == null, "Whitespace and selections close the active query");
        var me = new Person("100", "Yo"); var alice = new Person("101", "Alice", Username: "alice.user"); var outside = new Person("999", "Fuera del chat");
        var snapshot = new AppSnapshot { SignedIn = true, CurrentUser = me, View = "chat", SelectedChannelId = "200", SelectedChannelKind = "dm", ChannelAccess = ChannelAccess.Private, SearchPeople = new[] { alice }, Relationships = new[] { new Relationship(outside, 1) }, GuildMembers = new[] { new ChannelMember(outside) } };
        var people = ComposerMentionText.Candidates(snapshot, "");
        Check(people.Any(p => p.Id == alice.Id) && people.Any(p => p.Id == me.Id) && people.All(p => p.Id != outside.Id), "Private suggestions include current participants and self, never global friends or a prior server");
        Check(ComposerMentionText.Candidates(snapshot, "alice.us").Single().Id == alice.Id, "Username matching selects the relevant participant");
        Check(ComposerMentionText.Candidates(snapshot with { SelectedChannelKind = "group", GroupMembers = new[] { new ChannelMember(new Person("102", "Grupo")) } }, "Grupo").Length == 1, "Group participants are eligible in their group context");
        var guild = snapshot with { SelectedGuildId = "500", SelectedChannelKind = "text", GuildMembers = new[] { new ChannelMember(alice, Nickname: "Apodo del servidor") } };
        Check(ComposerMentionText.Candidates(guild, "Apodo").Single().DisplayName == "Apodo del servidor", "Guild suggestions use the contextual member nickname");
        Check(ComposerMentionText.Label(alice with { Name = "Apodo contextual", DisplayName = "Nombre global" }) == "Apodo contextual", "A contextual display name takes priority over the global account display name");
        Check(ComposerMentionText.Candidates(snapshot with { ChannelAccess = ChannelAccess.Private with { CanSend = false } }, "").Length == 0 && ComposerMentionText.Candidates(snapshot with { ChannelAccess = ChannelAccess.Unknown }, "").Length == 0, "Unknown and read-only channels never enable mention preparation");
        Check(ComposerMentionText.Candidates(snapshot with { SearchPeople = Enumerable.Range(1000, 100).Select(i => new Person(i.ToString(), "Persona " + i)).ToArray() }, "").Length == 8, "Suggestions have a hard eight-person bound");
        const string text = "Hola @Alice y resto"; var span = new ComposerMention(5, 6, alice.Id, "@Alice");
        Check(ComposerMentionText.Serialize(text, new[] { span }) == "Hola <@101> y resto", "Only explicit intact editor selections become transport mentions");
        Check(ComposerMentionText.Serialize(text, null) == text, "A manually typed display name is ordinary text until explicitly selected");
        Check(ComposerMentionText.Serialize(text, new[] { span with { Label = "@Other" } }) == text, "Stale or forged labels cannot become another user's mention");
        var shifted = ComposerMentionText.Adjust(text, "Antes " + text, new[] { span });
        Check(ComposerMentionText.Serialize("Antes " + text, shifted) == "Antes Hola <@101> y resto", "Inserting before a mention shifts its range without losing identity");
        Check(ComposerMentionText.Adjust(text, "Hola @Alic y resto", new[] { span }).Length == 0, "Editing part of a selected name invalidates its ping");
        Check(ComposerMentionText.Adjust(text, "Hola  y resto", new[] { span }).Length == 0, "Deleting the selected mention removes its metadata");
        Check(ComposerMentionText.Adjust("@Alice", "@Alicex", new[] { span with { Start = 0 } }).Length == 0, "Appending letters to the selected name invalidates its ping");
        Check(ComposerMentionText.Serialize(text + " nuevo", ComposerMentionText.Adjust(text, text + " nuevo", new[] { span })) == "Hola <@101> y resto nuevo", "Typing after the mention preserves the selected identity");
        var store = new ComposerDraftStore(); store.Put("200", text, "", "", Array.Empty<PendingAttachment>(), new[] { span }); store.Put("201", "Otro chat", "", "", Array.Empty<PendingAttachment>());
        Check(store.Get("200").Text == text && store.Get("200").Mentions!.Single() == span && store.Get("201").Mentions!.Count == 0, "Friendly mention spans are stored independently in each draft");
        var submission = store.Submit("200"); store.Acknowledge("200", text, "", submission.SubmissionId);
        Check(!store.Get("200").IsEmpty, "An acknowledgement with display text cannot consume a transport submission");
        store.Acknowledge("200", "Hola <@101> y resto", "", submission.SubmissionId);
        Check(store.Get("200").IsEmpty && store.Get("201").Text == "Otro chat", "Wire-text acknowledgement clears only the acknowledged draft");
        store.Put("200", text, "", "", Array.Empty<PendingAttachment>(), new[] { span }); submission = store.Submit("200");
        store.Put("200", text + " nuevo", "", "", Array.Empty<PendingAttachment>(), ComposerMentionText.Adjust(text, text + " nuevo", new[] { span }));
        store.Acknowledge("200", "Hola <@101> y resto", "", submission.SubmissionId);
        Check(store.TransportText("200") == "Hola <@101> y resto nuevo", "Late acknowledgement preserves later input and its friendly mention spans");
        string maxText = new string('x', 3997) + " @A"; store.Put("long", maxText, "", "", Array.Empty<PendingAttachment>(), new[] { new ComposerMention(3998, 2, "123456789012345678", "@A") });
        bool rejected = false; try { store.Submit("long"); } catch (InvalidOperationException) { rejected = true; }
        Check(rejected && store.Get("long").Text == maxText, "Transport expansion beyond four thousand characters rejects sending and keeps the friendly draft");
        string repeated = string.Concat(Enumerable.Repeat("@A ", 101)); var many = Enumerable.Range(0, 101).Select(i => new ComposerMention(i * 3, 2, "101", "@A")).ToArray();
        Check(ComposerMentionText.Valid(repeated, many).Length == 100, "Mention metadata is bounded to one hundred ranges per draft");
        Console.WriteLine(JsonSerializer.Serialize(new { success = errors.Count == 0, checks, errors, syntheticDataOnly = true, realNetworkUsed = false, realSends = 0 }));
        return errors.Count == 0 ? 0 : 1;
    }
}
