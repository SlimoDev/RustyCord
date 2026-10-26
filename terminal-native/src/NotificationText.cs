using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace NotDiscord.Terminal;

public static class NotificationText
{
    public static string Preview(string? content, string fallback = "Nuevo mensaje", int maximum = 50)
    {
        maximum = Math.Clamp(maximum, 1, 200);
        string text = Regex.Replace(content ?? "", @"<@!?\d+>|<@&\d+>", "@usuario", RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(60));
        text = Regex.Replace(text, @"<#\d+>", "#canal", RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(60));
        text = Regex.Replace(text, @"<a?:([^:>]{1,80}):\d+>", ":$1:", RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(60));
        text = Regex.Replace(text, @"\s+", " ", RegexOptions.CultureInvariant, TimeSpan.FromMilliseconds(60)).Trim();
        if (text.Length == 0) return fallback;
        var result = new StringBuilder(); var elements = StringInfo.GetTextElementEnumerator(text); int count = 0;
        while (elements.MoveNext()) { if (count++ == maximum) { result.Append('…'); break; } result.Append(elements.GetTextElement()); }
        return result.ToString();
    }
}
