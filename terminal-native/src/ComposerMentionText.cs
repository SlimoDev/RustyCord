using System.Text;

namespace NotDiscord.Terminal;

public sealed record ComposerMentionQuery(int Start, int Length, string Query);

/// <summary>Editor text remains friendly; only explicit, intact selections become transport mentions.</summary>
public static class ComposerMentionText
{
    public static string Label(Person person) => person.Name.Length > 0 ? person.Name : person.DisplayName.Length > 0 ? person.DisplayName : person.Username;
    public static ComposerMention[] Valid(string text, IReadOnlyList<ComposerMention>? spans)
    {
        var output = new List<ComposerMention>(); int previousEnd = 0;
        foreach (var span in (spans ?? Array.Empty<ComposerMention>()).OrderBy(s => s.Start).Take(100))
        {
            if (span.Start < previousEnd || span.Start < 0 || span.Length < 2 || span.Length > text.Length - span.Start || span.Label.Length != span.Length || !span.Label.StartsWith('@') ||
                span.UserId.Length is < 1 or > 20 || span.UserId.Any(c => !char.IsAsciiDigit(c)) || !text.AsSpan(span.Start, span.Length).SequenceEqual(span.Label.AsSpan())) continue;
            output.Add(span); previousEnd = span.Start + span.Length;
        }
        return output.ToArray();
    }
    public static string Serialize(string text, IReadOnlyList<ComposerMention>? spans)
    {
        var safe = Valid(text, spans); if (safe.Length == 0) return text;
        var output = new StringBuilder(text.Length); int cursor = 0;
        foreach (var span in safe) { output.Append(text, cursor, span.Start - cursor); output.Append("<@").Append(span.UserId).Append('>'); cursor = span.Start + span.Length; }
        output.Append(text, cursor, text.Length - cursor); return output.ToString();
    }
    public static ComposerMention[] Adjust(string previous, string current, IReadOnlyList<ComposerMention> spans)
    {
        int start = 0; while (start < previous.Length && start < current.Length && previous[start] == current[start]) start++;
        int suffix = 0; while (suffix < previous.Length - start && suffix < current.Length - start && previous[previous.Length - suffix - 1] == current[current.Length - suffix - 1]) suffix++;
        int oldEnd = previous.Length - suffix, inserted = current.Length - start - suffix, delta = current.Length - previous.Length;
        var output = new List<ComposerMention>();
        foreach (var span in Valid(previous, spans))
        {
            int end = span.Start + span.Length;
            if (oldEnd <= span.Start) output.Add(span with { Start = span.Start + delta });
            else if (start >= end)
            {
                // Extending the selected name turns it back into ordinary text.
                if (start == end && inserted > 0 && !char.IsWhiteSpace(current[start]) && !char.IsPunctuation(current[start])) continue;
                output.Add(span);
            }
        }
        return Valid(current, output);
    }
    public static ComposerMentionQuery? FindQuery(string text, int caret, int selectionLength = 0)
    {
        if (selectionLength != 0 || caret < 1 || caret > text.Length) return null;
        int start = text.LastIndexOf('@', caret - 1, Math.Min(caret, 65));
        if (start < 0 || start > 0 && !char.IsWhiteSpace(text[start - 1]) && text[start - 1] is not ('(' or '[' or '{' or ',' or ';')) return null;
        string query = text[(start + 1)..caret];
        if (query.Length > 64 || query.Any(c => char.IsWhiteSpace(c) || c is '<' or '>' or '@')) return null;
        return new(start, caret - start, query);
    }
    public static Person[] Candidates(AppSnapshot snapshot, string query)
    {
        if (!snapshot.SignedIn || snapshot.Demo || snapshot.Busy || snapshot.View != "chat" || snapshot.SelectedChannelId.Length == 0 || !snapshot.ChannelAccess.CanView || !snapshot.ChannelAccess.CanSend) return Array.Empty<Person>();
        var people = new Dictionary<string, Person>(StringComparer.Ordinal);
        void Add(Person person) { if (person.Id.Length is > 0 and <= 20 && person.Id.All(char.IsAsciiDigit)) people[person.Id] = person; }
        foreach (var person in snapshot.SearchPeople.Take(2000)) Add(person);
        var members = snapshot.SelectedGuildId.Length > 0 ? snapshot.ChannelAccess.CanViewMembers ? snapshot.GuildMembers : Array.Empty<ChannelMember>() : snapshot.SelectedChannelKind == "group" ? snapshot.GroupMembers : Array.Empty<ChannelMember>();
        foreach (var member in members.Take(2000)) Add(member.Nickname.Length > 0 ? member.User with { Name = member.Nickname, DisplayName = member.Nickname } : member.User);
        if (snapshot.CurrentUser != null) Add(snapshot.CurrentUser);
        return people.Values.Where(p => query.Length == 0 || Label(p).Contains(query, StringComparison.OrdinalIgnoreCase) || p.Username.Contains(query, StringComparison.OrdinalIgnoreCase))
            .OrderBy(p => Label(p).StartsWith(query, StringComparison.OrdinalIgnoreCase) || p.Username.StartsWith(query, StringComparison.OrdinalIgnoreCase) ? 0 : 1)
            .ThenBy(Label, StringComparer.CurrentCultureIgnoreCase).Take(8).ToArray();
    }
}
