using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using NotDiscord.Terminal;

internal static class ComposerMentionsUiTests
{
    [STAThread] static int Main()
    {
        int checks = 0; var errors = new List<string>(); void Check(bool value, string name) { checks++; if (!value) errors.Add(name); }
        var app = new Application { ShutdownMode = ShutdownMode.OnExplicitShutdown }; var window = new TerminalWindow();
        T Field<T>(string name) => (T)typeof(TerminalWindow).GetField(name, BindingFlags.NonPublic | BindingFlags.Instance)!.GetValue(window)!;
        object? Invoke(string name, params object[] values) => typeof(TerminalWindow).GetMethod(name, BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(window, values);
        bool Accepts(Key key, ModifierKeys modifiers) => (bool)typeof(TerminalWindow).GetMethod("IsComposerMentionSelectionKey", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, new object[] { key, modifiers })!;
        var me = new Person("100", "Yo"); var alice = new Person("101", "Alice", Username: "alice.user");
        var snapshot = new AppSnapshot { SignedIn = true, CurrentUser = me, View = "chat", SelectedChannelId = "200", SelectedChannelKind = "dm", ChannelAccess = ChannelAccess.Private, SearchPeople = new[] { me, alice }, DirectMessages = new[] { new NavItem("200", "A", "dm"), new NavItem("201", "B", "dm") } };
        var actions = new List<UiAction>(); window.ActionRequested += actions.Add; window.Apply(snapshot); var composer = Field<TextBox>("composer"); var store = Field<ComposerDraftStore>("composerDrafts");
        composer.Text = "Hola @ali"; composer.CaretIndex = composer.Text.Length; Invoke("RefreshComposerMentions");
        Check(Field<Person[]>("composerMentionCandidates").Single().Id == alice.Id, "Typing @ filters the participants available to the active editor");
        Check(!Field<Popup>("composerMentionPopup").IsOpen && !window.IsLoaded, "An unloaded fixture never opens a native popup or starts an account");
        Invoke("CommitComposerMention", alice);
        Check(composer.Text == "Hola @Alice " && !composer.Text.Contains("101"), "Selecting a person inserts the friendly name instead of exposing the transport identifier");
        Check(actions.Count == 0 && store.TransportText("200") == "Hola <@101> ", "Selecting the suggestion prepares a mention without sending any message");
        window.Apply(snapshot with { SelectedChannelId = "201" }); composer.Text = "Borrador B"; window.Apply(snapshot);
        Check(composer.Text == "Hola @Alice " && Field<IReadOnlyList<ComposerMention>>("composerMentions").Single().UserId == alice.Id, "Changing chats preserves the visible mention and its per-draft identity");
        window.Apply(snapshot with { View = "profile", ProfileUser = alice }); window.Apply(snapshot);
        Check(composer.Text == "Hola @Alice " && store.TransportText("200") == "Hola <@101> ", "Opening and leaving a profile preserves mention metadata");
        Invoke("SendPreparedMessage"); var sent = actions.Last(a => a.Kind == "send-prepared"); var prepared = JsonSerializer.Deserialize<PreparedSend>(sent.Extra)!;
        Check(sent.Text == "Hola <@101> " && composer.Text == "Hola @Alice ", "Explicit send serializes the selected mention while keeping the editor friendly until acknowledgement");
        window.Apply(snapshot with { SelectedChannelId = "201" }); window.AcknowledgePreparedSend("200", sent.Text, "", prepared.SubmissionId);
        Check(composer.Text == "Borrador B", "Acknowledgement for a mention in another chat never clears the current editor");
        window.Apply(snapshot); Check(composer.Text.Length == 0 && Field<IReadOnlyList<ComposerMention>>("composerMentions").Count == 0, "Acknowledged mention metadata is cleared only in the submitted draft");
        composer.Text = "@ali"; composer.CaretIndex = composer.Text.Length; Invoke("CommitComposerMention", alice); Invoke("SendPreparedMessage");
        sent = actions.Last(a => a.Kind == "send-prepared"); prepared = JsonSerializer.Deserialize<PreparedSend>(sent.Extra)!;
        composer.Text += "texto posterior"; window.AcknowledgePreparedSend("200", sent.Text, "", prepared.SubmissionId);
        Check(composer.Text == "@Alice texto posterior" && store.TransportText("200") == "<@101> texto posterior", "Typing during submission survives ACK with the selected mention intact");
        composer.Text = composer.Text.Replace("@Alice", "@Alicia");
        Check(store.TransportText("200") == "@Alicia texto posterior" && Field<IReadOnlyList<ComposerMention>>("composerMentions").Count == 0, "Editing the name invalidates the ping instead of silently mentioning the old identity");
        Check(Accepts(Key.Enter, ModifierKeys.None) && Accepts(Key.Tab, ModifierKeys.None) && !Accepts(Key.Enter, ModifierKeys.Shift) && !Accepts(Key.Tab, ModifierKeys.Shift), "Only plain Enter and Tab commit suggestions; Shift combinations retain normal editor behavior");
        int beforeReadOnly = actions.Count; window.Apply(snapshot with { ChannelAccess = ChannelAccess.Private with { CanSend = false } }); composer.Text = "@ali"; composer.CaretIndex = composer.Text.Length;
        Invoke("RefreshComposerMentions"); Invoke("CommitComposerMention", alice);
        Check(Field<Person[]>("composerMentionCandidates").Length == 0 && composer.Text == "@ali" && actions.Count == beforeReadOnly, "Permission changes disable both suggestion and completion execution");
        window.Apply(snapshot with { SignedIn = false, CurrentUser = null, SelectedChannelId = "", ChannelAccess = ChannelAccess.Unknown, View = "welcome" });
        Check(Field<IReadOnlyList<ComposerMention>>("composerMentions").Count == 0 && store.Count == 0, "Logout clears mention spans alongside all session drafts");
        window.Close(); app.Shutdown();
        Console.WriteLine(JsonSerializer.Serialize(new { success = errors.Count == 0, checks, errors, syntheticDataOnly = true, realNetworkUsed = false, realSends = 0, nativePopupsShown = 0 })); return errors.Count == 0 ? 0 : 1;
    }
}
