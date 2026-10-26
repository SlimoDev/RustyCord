namespace NotDiscord.Terminal;

public sealed record PreparedSend(string SubmissionId, string ReplyId, string[] Paths);
public sealed record PendingAttachment(string Id, string Path, string Name, long Length, bool Temporary = false);
public sealed record ComposerMention(int Start, int Length, string UserId, string Label);
public sealed record ComposerDraft(string Text, string ReplyId, string ReplyLabel, IReadOnlyList<PendingAttachment> Attachments, long Revision = 0, IReadOnlyList<ComposerMention>? Mentions = null)
{
    public static ComposerDraft Empty { get; } = new("", "", "", Array.Empty<PendingAttachment>());
    public bool IsEmpty => Text.Length == 0 && ReplyId.Length == 0 && Attachments.Count == 0;
}

/// <summary>Session-only drafts. An acknowledgement never erases text typed after submission.</summary>
public sealed class ComposerDraftStore
{
    private readonly Dictionary<string, ComposerDraft> drafts = new(StringComparer.Ordinal);
    private readonly Dictionary<string, (string Channel, ComposerDraft Sent)> submitted = new(StringComparer.Ordinal);
    private long revision;
    public int Count => drafts.Count;
    public ComposerDraft Get(string channel) => drafts.GetValueOrDefault(channel) ?? ComposerDraft.Empty;
    public ComposerDraft Put(string channel, string text, string replyId, string replyLabel, IReadOnlyList<PendingAttachment> files, IReadOnlyList<ComposerMention>? mentions = null)
    {
        if (channel.Length == 0) return ComposerDraft.Empty;
        var safeMentions = ComposerMentionText.Valid(text, mentions);
        var old = Get(channel);
        if (old.Text == text && old.ReplyId == replyId && old.ReplyLabel == replyLabel && old.Attachments.SequenceEqual(files) && (old.Mentions ?? Array.Empty<ComposerMention>()).SequenceEqual(safeMentions)) return old;
        if (!drafts.ContainsKey(channel) && drafts.Count >= 1024) throw new InvalidOperationException("Hay 1024 borradores abiertos. Borra uno antes de preparar otro.");
        var next = new ComposerDraft(text, replyId, replyLabel, files.ToArray(), ++revision, safeMentions);
        if (next.IsEmpty) drafts.Remove(channel); else drafts[channel] = next;
        return next;
    }
    public PreparedSend Submit(string channel)
    {
        var value = Get(channel);
        if (value.IsEmpty || string.IsNullOrWhiteSpace(value.Text) && value.Attachments.Count == 0) throw new InvalidOperationException("Escribe un mensaje o añade un archivo.");
        _ = TransportText(channel);
        var existing = submitted.FirstOrDefault(p => p.Value.Channel == channel && p.Value.Sent.Revision == value.Revision);
        if (existing.Key != null) return new(existing.Key, value.ReplyId, value.Attachments.Select(a => a.Path).ToArray());
        string id = Guid.NewGuid().ToString("N");
        if (submitted.Count >= 64) submitted.Remove(submitted.Keys.First());
        submitted[id] = (channel, value);
        return new(id, value.ReplyId, value.Attachments.Select(a => a.Path).ToArray());
    }
    public IReadOnlyList<PendingAttachment> Acknowledge(string channel, string text, string replyId, string id)
    {
        if (!submitted.TryGetValue(id, out var pending) || pending.Channel != channel || ComposerMentionText.Serialize(pending.Sent.Text, pending.Sent.Mentions) != text || pending.Sent.ReplyId != replyId) return Array.Empty<PendingAttachment>();
        submitted.Remove(id);
        var current = Get(channel);
        var sentIds = pending.Sent.Attachments.Select(a => a.Id).ToHashSet(StringComparer.Ordinal);
        var released = current.Attachments.Where(a => sentIds.Contains(a.Id)).ToArray();
        if (current.Revision == pending.Sent.Revision) drafts.Remove(channel);
        else Put(channel, current.Text, current.ReplyId, current.ReplyLabel, current.Attachments.Where(a => !sentIds.Contains(a.Id)).ToArray(), current.Mentions);
        return released;
    }
    public string TransportText(string channel)
    {
        var draft = Get(channel); string content = ComposerMentionText.Serialize(draft.Text, draft.Mentions);
        if (content.Length > 4000) throw new InvalidOperationException("El mensaje con sus menciones supera el límite de 4000 caracteres. El borrador se conserva.");
        return content;
    }
    public ComposerDraft Remove(string channel) { var old = Get(channel); drafts.Remove(channel); return old; }
    public IReadOnlyList<PendingAttachment> Clear()
    { var files = drafts.Values.SelectMany(d => d.Attachments).ToArray(); drafts.Clear(); submitted.Clear(); return files; }
}
