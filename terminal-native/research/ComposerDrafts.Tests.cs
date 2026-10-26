using System.Text.Json;
using NotDiscord.Terminal;

internal static class ComposerDraftTests
{
    static int Main()
    {
        int checks = 0; var failures = new List<string>();
        void Check(bool value, string title) { checks++; if (!value) failures.Add(title); }
        var store = new ComposerDraftStore();
        var firstFile = new PendingAttachment("file-a", "synthetic-a.png", "a.png", 10);
        var secondFile = new PendingAttachment("file-b", "synthetic-b.png", "b.png", 20);
        store.Put("200", "Primer borrador", "message-1", "Respuesta sintética", new[] { firstFile });
        store.Put("201", "Otro borrador", "", "", Array.Empty<PendingAttachment>());
        Check(store.Get("200").Text == "Primer borrador" && store.Get("201").Text == "Otro borrador", "Conversations keep independent text drafts");
        Check(store.Get("200").ReplyId == "message-1" && store.Get("200").Attachments.Single() == firstFile, "Reply and staged attachments belong to their conversation");
        var sent = store.Submit("200"); var retry = store.Submit("200");
        Check(sent.SubmissionId == retry.SubmissionId && sent.Paths.SequenceEqual(new[] { "synthetic-a.png" }), "Retrying an unchanged submission reuses its nonce and files");
        store.Acknowledge("wrong-channel", "Primer borrador", "message-1", sent.SubmissionId);
        Check(store.Get("200").Text == "Primer borrador" && store.Submit("200").SubmissionId == sent.SubmissionId, "An unrelated acknowledgement cannot consume or clear a draft submission");
        var released = store.Acknowledge("200", "Primer borrador", "message-1", sent.SubmissionId);
        Check(store.Get("200").IsEmpty && store.Get("201").Text == "Otro borrador" && released.Single() == firstFile, "ACK clears the sent channel even while a different channel is selected");
        store.Put("200", "Enviando", "", "", new[] { firstFile });
        sent = store.Submit("200");
        store.Put("200", "Texto nuevo mientras envía", "", "", new[] { firstFile, secondFile });
        released = store.Acknowledge("200", "Enviando", "", sent.SubmissionId);
        Check(store.Get("200").Text == "Texto nuevo mientras envía" && store.Get("200").Attachments.SequenceEqual(new[] { secondFile }), "ACK preserves later input and new files while releasing the files already sent");
        Check(released.SequenceEqual(new[] { firstFile }), "ACK identifies temporary files which can be released safely");
        var removed = store.Remove("200");
        Check(removed.Text == "Texto nuevo mientras envía" && store.Get("200").IsEmpty && store.Get("201").Text.Length > 0, "Manual discard affects only one conversation");
        Check(store.Clear().Count == 0 && store.Count == 0 && store.Get("201").IsEmpty, "Logout clears all session-only draft state");
        bool rejectedEmpty = false;
        try { store.Submit("empty"); } catch (InvalidOperationException) { rejectedEmpty = true; }
        Check(rejectedEmpty, "Empty drafts cannot trigger submission");
        var withoutText = store.Put("files", "", "", "", new[] { firstFile });
        Check(store.Submit("files").Paths.Length == 1 && !withoutText.IsEmpty, "An attachment-only message is an explicit valid submission");
        store.Clear(); for (int i = 0; i < 1024; i++) store.Put(i.ToString(), "kept", "", "", Array.Empty<PendingAttachment>());
        bool bounded = false; try { store.Put("overflow", "new", "", "", Array.Empty<PendingAttachment>()); } catch (InvalidOperationException) { bounded = true; }
        Check(bounded && store.Get("0").Text == "kept" && store.Count == 1024, "Memory cap never evicts an unsent draft silently");
        Console.WriteLine(JsonSerializer.Serialize(new { success = failures.Count == 0, checks, failures, syntheticDataOnly = true, realNetworkUsed = false, realSends = 0 }));
        return failures.Count == 0 ? 0 : 1;
    }
}
