using System.Text.RegularExpressions;
using System.Text;
using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;

namespace NotDiscord.Terminal;

internal static class UiPresenceViews
{
    private static readonly Regex CompactEmojiToken = new(@"\G<a?:[^:\s<>]{1,64}:\d{1,20}>", RegexOptions.Compiled | RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(100));
    private static readonly Regex Tokens = new(@"<@!?(?<user>\d{1,20})>|<(?<animated>a?):(?<name>[^:\s<>]{1,64}):(?<id>\d{1,20})>|https?://[^\s<>]+", RegexOptions.Compiled | RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(100));
    public static string PresenceLabel(string status) => status switch { "online" => "En línea", "idle" => "Ausente", "dnd" => "No molestar", "offline" or "invisible" => "Desconectado", _ => "Estado no disponible" };
    public static FrameworkElement AvatarWithPresence(Person person, string status, int size)
    {
        var holder = new Grid { Width = size, Height = size }; holder.Children.Add(TerminalWindow.RoundAvatar(person, size));
        if (status.Length > 0)
        {
            double dot = Math.Max(12, size * .22); string color = status switch { "online" => "#23A55A", "idle" => "#F0B232", "dnd" => "#F23F43", _ => "#80848E" };
            holder.Children.Add(new Border { Width = dot, Height = dot, Background = TerminalWindow.Brush(color), CornerRadius = new CornerRadius(dot / 2), BorderBrush = UiTheme.Panel, BorderThickness = new Thickness(2), HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Bottom, ToolTip = PresenceLabel(status) });
        }
        return holder;
    }
    public static string ActivityLabel(PresenceActivity activity)
    {
        string content = activity.Type switch
        {
            2 => "♫ " + (activity.Details.Length > 0 ? activity.Details : activity.Name) + (activity.State.Length > 0 ? " — " + activity.State : ""),
            0 => "🎮 " + activity.Name + (activity.Details.Length > 0 ? " · " + activity.Details : ""),
            1 => "En directo · " + activity.Name,
            3 => "Viendo " + activity.Name,
            5 => "Compitiendo · " + activity.Name,
            _ => activity.State.Length > 0 ? activity.State : activity.Name
        };
        return content.Length > 300 ? content[..300] : content;
    }
    public static string CompactActivityText(string text, int maxElements = 20)
    {
        maxElements = Math.Clamp(maxElements, 2, 120);
        var normalized = new StringBuilder(text.Length); bool pendingSpace = false;
        foreach (char value in text)
        {
            if (char.IsWhiteSpace(value)) { pendingSpace = normalized.Length > 0; continue; }
            if (pendingSpace) { normalized.Append(' '); pendingSpace = false; }
            normalized.Append(value);
        }
        string valueText = normalized.ToString(); var elements = new List<string>(maxElements + 1);
        for (int index = 0; index < valueText.Length && elements.Count <= maxElements;)
        {
            // A server emoji is one visible element even though its transport
            // representation contains a name and numeric identifier.
            var token = valueText[index] == '<' ? CompactEmojiToken.Match(valueText, index) : Match.Empty;
            string element = token.Success ? token.Value : StringInfo.GetNextTextElement(valueText, index);
            elements.Add(element); index += element.Length;
        }
        return elements.Count <= maxElements ? valueText : string.Concat(elements.Take(maxElements - 1)) + "…";
    }
    public static TextBlock CompactActivity(string text, double size = 13, Brush? color = null)
    {
        var block = RichText(CompactActivityText(text), size, color ?? TerminalWindow.Muted);
        block.TextWrapping = TextWrapping.NoWrap; block.TextTrimming = TextTrimming.CharacterEllipsis;
        block.LineStackingStrategy = LineStackingStrategy.BlockLineHeight; block.LineHeight = Math.Ceiling(size * 1.5);
        block.MaxHeight = Math.Ceiling(size * 1.7); block.ToolTip = text;
        return block;
    }
    public static TextBlock RichText(string text, double size = 14, Brush? color = null, Action<string>? link = null, IReadOnlyList<Person>? mentions = null, Action<Person>? mention = null)
    {
        var block = new TextBlock { FontSize = size, Foreground = color ?? TerminalWindow.Ink, TextWrapping = TextWrapping.Wrap, FontFamily = new FontFamily("Segoe UI") };
        int cursor = 0, images = 0;
        foreach (Match match in Tokens.Matches(text))
        {
            if (match.Index > cursor) AddSystemText(block, text[cursor..match.Index], size);
            if (match.Groups["user"].Success)
            {
                var person = mentions?.FirstOrDefault(p => p.Id == match.Groups["user"].Value);
                string label = "@" + (person == null ? "usuario desconocido" : person.Name.Length > 0 ? person.Name : person.DisplayName);
                if (person != null && mention != null)
                {
                    var tag = new Hyperlink(new Run(label)) { Foreground = TerminalWindow.Accent, Background = UiTheme.AccentSurface, TextDecorations = null, ToolTip = person.Username.Length > 0 ? "@" + person.Username : "Ver perfil" };
                    tag.Click += (_, _) => mention(person); block.Inlines.Add(tag);
                }
                else block.Inlines.Add(new Run(label) { Foreground = TerminalWindow.Accent, Background = UiTheme.AccentSurface });
            }
            else if (match.Groups["id"].Success && images++ < 24)
            {
                string url = $"https://cdn.discordapp.com/emojis/{match.Groups["id"].Value}.png?size=32";
                var image = new UiMediaImage(url, 48) { Width = size * 1.3, Height = size * 1.3, ToolTip = ":" + match.Groups["name"].Value + ":", Stretch = Stretch.Uniform };
                block.Inlines.Add(new InlineUIContainer(image) { BaselineAlignment = BaselineAlignment.Center });
            }
            else if (!match.Groups["id"].Success && link != null)
            {
                string address = match.Value; var hyperlink = new Hyperlink(new Run(address)) { Foreground = TerminalWindow.Accent, ToolTip = address };
                hyperlink.Click += (_, _) => link(address); block.Inlines.Add(hyperlink);
            }
            else AddSystemText(block, match.Value, size);
            cursor = match.Index + match.Length;
        }
        if (cursor < text.Length) AddSystemText(block, text[cursor..], size);
        return block;
    }
    public static string DisplayMentionText(string text, IReadOnlyList<Person>? mentions)
        => Tokens.Replace(text, match => !match.Groups["user"].Success ? match.Value : "@" + (mentions?.FirstOrDefault(p => p.Id == match.Groups["user"].Value) is Person person ? person.Name.Length > 0 ? person.Name : person.DisplayName : "usuario desconocido"));
    public static UIElement StatusLine(string text, StatusEmoji? emoji, double size = 13)
    {
        var line = new DockPanel { LastChildFill = true };
        if (emoji != null)
        {
            FrameworkElement image = MediaImages.IsAllowed(emoji.ImageUrl)
                ? new UiMediaImage(emoji.ImageUrl, 32) { Width = size + 3, Height = size + 3, Stretch = Stretch.Uniform }
                : new SystemEmojiImage(emoji.Name, size + 4);
            image.Margin = new Thickness(0, 0, 4, 0); DockPanel.SetDock(image, Dock.Left); line.Children.Add(image);
        }
        var content = RichText(text, size, TerminalWindow.Muted); content.TextTrimming = TextTrimming.CharacterEllipsis; content.TextWrapping = TextWrapping.NoWrap; line.Children.Add(content);
        line.Visibility = text.Length > 0 || emoji != null ? Visibility.Visible : Visibility.Collapsed;
        return line;
    }
    private static void AddSystemText(TextBlock block, string text, double size)
    {
        // Normal prose takes the inexpensive text path; only emoji graphemes
        // become tiny cached color images from the installed Windows font.
        if (!text.Any(c => char.IsSurrogate(c) || c == '\uFE0F')) { block.Inlines.Add(new Run(text)); return; }
        var elements = StringInfo.GetTextElementEnumerator(text); var plain = new StringBuilder(); int images = 0;
        while (elements.MoveNext())
        {
            string element = elements.GetTextElement();
            bool emoji = element.Contains('\uFE0F') || element.EnumerateRunes().Any(r => r.Value is >= 0x1F000 and <= 0x1FAFF);
            if (emoji && images++ < 64)
            {
                if (plain.Length > 0) { block.Inlines.Add(new Run(plain.ToString())); plain.Clear(); }
                block.Inlines.Add(new InlineUIContainer(new SystemEmojiImage(element, size * 1.3)) { BaselineAlignment = BaselineAlignment.Center });
            }
            else plain.Append(element);
        }
        if (plain.Length > 0) block.Inlines.Add(new Run(plain.ToString()));
    }
}
