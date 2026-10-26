using System.Globalization;
using System.Reflection;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;
using NotDiscord.Terminal;

internal static class CompactActivityTests
{
    [STAThread] static int Main()
    {
        var errors = new List<string>(); int checks = 0;
        void Check(bool success, string name) { checks++; if (!success) errors.Add(name); }
        var views = typeof(TerminalWindow).Assembly.GetType("NotDiscord.Terminal.UiPresenceViews")!;
        object Invoke(string method, params object?[] supplied)
        {
            var info = views.GetMethod(method, BindingFlags.Static | BindingFlags.Public)!;
            var values = info.GetParameters().Select((parameter, index) => index < supplied.Length ? supplied[index] : parameter.DefaultValue).ToArray();
            return info.Invoke(null, values)!;
        }
        string Clip(string text) => (string)Invoke("CompactActivityText", text);
        const string full = "Flower In A Hurricane (feat. Tame Impala, SZA, King Gizzard & The Lizard Wizard) [from GTAVI] — Justice; Tame Impala; SZA; King Gizzard & The Lizard Wizard";
        var clipped = Clip(full);
        Check(new StringInfo(clipped).LengthInTextElements == 20 && clipped.EndsWith('…'), "Long Spotify activity is twenty visible text elements including ellipsis");
        Check(Clip("Portal 2") == "Portal 2", "Short game names remain unchanged");
        Check(Clip("  Portal\r\n  2\t ") == "Portal 2", "Activity line breaks and repeated whitespace cannot expand a row");
        const string family = "👨‍👩‍👧‍👦";
        Check(Clip(string.Concat(Enumerable.Repeat(family, 21))) == string.Concat(Enumerable.Repeat(family, 19)) + "…", "Family ZWJ emoji is never cut in the middle");
        const string accent = "e\u0301";
        Check(Clip(string.Concat(Enumerable.Repeat(accent, 21))) == string.Concat(Enumerable.Repeat(accent, 19)) + "…", "Combining characters stay attached when clipped");
        const string custom = "<a:dance:123456789012345678>";
        Check(Clip(custom + " " + new string('x', 40)).StartsWith(custom + " "), "Server emoji markup remains intact and counts as one visible element");
        Check(Clip(new string('x', 20)) == new string('x', 20), "Exactly twenty characters do not gain an unnecessary ellipsis");
        var compact = (TextBlock)Invoke("CompactActivity", full, 12d, Brushes.Gray);
        Check(compact.TextWrapping == TextWrapping.NoWrap && compact.TextTrimming == TextTrimming.CharacterEllipsis && compact.MaxHeight <= 21, "Compact activity is constrained to one row even in an unconstrained list measure");
        Check((string)compact.ToolTip == full, "Full activity remains available through the tooltip");
        var expanded = (TextBlock)Invoke("RichText", full, 16d, Brushes.Gray, null);
        Check(expanded.TextWrapping == TextWrapping.Wrap && new TextRange(expanded.ContentStart, expanded.ContentEnd).Text.Contains(full), "Expanded profile rich text retains the full activity details and wrapping");
        var activity = new PresenceActivity(2, "Spotify", full);
        var member = new UiMemberCell { DataContext = new UiMemberModel(new ChannelMember(new Person("101", "Persona ficticia"), "online", Activities: new[] { activity }), _ => {}) };
        member.Measure(new Size(230, double.PositiveInfinity));
        Check(member.DesiredSize.Height <= 78, "A long Spotify activity cannot turn a compact member row into a tall paragraph");
        Check(Descendants(member).OfType<TextBlock>().Any(b => b.ToolTip is string tooltip && tooltip.Contains(full)), "Member activity uses the compact control while retaining details");
        var nav = new UiChannelCell { DataContext = new UiNavModel(new NavItem("201", "Chat ficticio", "dm", ActivityText: full), _ => {}, true) };
        nav.Measure(new Size(250, double.PositiveInfinity));
        Check(nav.DesiredSize.Height < 100 && Descendants(nav).OfType<TextBlock>().Any(b => b.ToolTip as string == full && b.TextWrapping == TextWrapping.NoWrap), "Conversation sidebar uses a bounded compact activity with full details in tooltip");
        var friend = new UiPersonCell { DataContext = new UiPersonModel(new Relationship(new Person("102", "Amigo ficticio"), 1), _ => {}, false, new UserPresence("online", "", null, new[] { activity })) };
        friend.Measure(new Size(430, double.PositiveInfinity));
        Check(friend.DesiredSize.Height < 110 && Descendants(friend).OfType<TextBlock>().Any(b => b.ToolTip is string tooltip && tooltip.Contains(full) && b.TextWrapping == TextWrapping.NoWrap), "Friends list uses the compact activity without altering the profile rendering");
        Console.WriteLine(JsonSerializer.Serialize(new { success = errors.Count == 0, checks, errors, syntheticDataOnly = true, realNetworkUsed = false }));
        return errors.Count == 0 ? 0 : 1;
    }
    private static IEnumerable<DependencyObject> Descendants(DependencyObject root)
    {
        var seen = new HashSet<DependencyObject>(); var queue = new Queue<DependencyObject>(); seen.Add(root); queue.Enqueue(root);
        while (queue.Count > 0)
        {
            var current = queue.Dequeue(); var children = LogicalTreeHelper.GetChildren(current).OfType<DependencyObject>().ToList();
            if (current is Visual) for (int i = 0; i < VisualTreeHelper.GetChildrenCount(current); i++) children.Add(VisualTreeHelper.GetChild(current, i));
            foreach (var child in children) if (seen.Add(child)) { yield return child; queue.Enqueue(child); }
        }
    }
}
