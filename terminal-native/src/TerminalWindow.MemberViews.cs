using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private UiMemberModel[] BuildMemberRows(IReadOnlyList<ChannelMember> members)
    {
        if (state.SelectedGuildId.Length == 0) return members.Select(MemberModel).ToArray();
        string Group(ChannelMember member) => member.Presence.Length == 0 ? "unknown" : member.Presence is "offline" or "invisible" ? "offline" : member.HoistRoleId.Length > 0 ? member.HoistRoleId : "online";
        var rows = new List<UiMemberModel>();
        foreach (var group in members.GroupBy(Group))
        {
            var supplied = state.GuildMemberGroups.FirstOrDefault(g => g.Id == group.Key);
            var role = state.GuildRoles.FirstOrDefault(r => r.Id == group.Key);
            string name = supplied?.Name ?? role?.Name ?? (group.Key == "online" ? "En línea" : group.Key == "offline" ? "Desconectados" : "Estado no disponible");
            int count = Math.Max(group.Count(), supplied?.Count ?? 0);
            rows.Add(new(new ChannelMember(new Person("", "")), handleAction, name.ToUpperInvariant() + " — " + count, role?.Color ?? supplied?.Color ?? 0));
            rows.AddRange(group.Select(MemberModel));
        }
        return rows.ToArray();
    }
    private UiMemberModel MemberModel(ChannelMember member) => new(member, handleAction, RelationshipType: relationshipTypes.GetValueOrDefault(member.User.Id), IsSelf: member.User.Id == state.CurrentUser?.Id, CanAct: state.SignedIn && !state.Demo && !state.Busy);
}

internal static class UiRoleViews
{
    public static Brush Color(int color, bool readable = false)
    {
        if (color <= 0 || color > 0xFFFFFF) return TerminalWindow.Ink;
        var result = System.Windows.Media.Color.FromRgb((byte)(color >> 16), (byte)(color >> 8), (byte)color);
        if (readable && UiTheme.Contrast(result, UiTheme.Panel.Color) < 3) return TerminalWindow.Ink;
        return new SolidColorBrush(result);
    }
    public static Border Badge(GuildRole role)
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal };
        row.Children.Add(new Border { Width = 10, Height = 10, CornerRadius = new CornerRadius(5), Background = Color(role.Color), Margin = new Thickness(0, 0, 6, 0) });
        row.Children.Add(TerminalWindow.Text(role.Name, 12));
        return new Border { Child = row, Background = UiTheme.Surface, BorderBrush = Color(role.Color), BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(5), Padding = new Thickness(8, 5, 8, 5), Margin = new Thickness(0, 4, 6, 0), ToolTip = role.Name + " · #" + role.Color.ToString("X6") };
    }
}

internal static class UiReactionViews
{
    internal static UIElement Build(ChatMessage message, UiMessageModel model)
    {
        var row = new WrapPanel { Margin = new Thickness(0, 6, 0, 0) };
        var access = model.Access ?? ChannelAccess.Private;
        foreach (var reaction in (message.Reactions ?? Array.Empty<MessageReaction>()).Where(r => r.Count > 0).Take(50))
        {
            string value = reaction.Emoji.Id.Length > 0 ? reaction.Emoji.Name + ":" + reaction.Emoji.Id : reaction.Emoji.Name;
            var content = new StackPanel { Orientation = Orientation.Horizontal };
            FrameworkElement emoji = reaction.Emoji.Id.Length > 0
                ? new UiMediaImage($"https://cdn.discordapp.com/emojis/{reaction.Emoji.Id}.png?size=32", 32) { Width = 20, Height = 20, Stretch = Stretch.Uniform }
                : new SystemEmojiImage(reaction.Emoji.Name, 20);
            content.Children.Add(emoji); var count = TerminalWindow.Text(reaction.Count.ToString(), 13); count.Margin = new Thickness(6, 0, 0, 0); content.Children.Add(count);
            var button = TerminalWindow.Button("", () => model.Action(new("react-toggle", message.Id, value, ChannelId: message.ChannelId)));
            button.Content = content; button.Padding = new Thickness(7, 4, 7, 4); button.Margin = new Thickness(0, 0, 5, 4);
            button.Background = reaction.Me ? UiTheme.Selected : UiTheme.Surface; button.BorderBrush = reaction.Me ? TerminalWindow.Accent : UiTheme.Line;
            button.IsEnabled = model.SignedIn && !model.Demo && !message.Deleted && access.CanView && access.CanReadHistory;
            button.ToolTip = (reaction.Me ? "Quitar mi reacción · " : "Añadir mi reacción · ") + reaction.Emoji.Name + " · " + reaction.Count;
            ToolTipService.SetShowOnDisabled(button, true); row.Children.Add(button);
        }
        return row;
    }
}
