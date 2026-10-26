using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;

namespace NotDiscord.Terminal;

internal static class UiReplyViews
{
    public static UIElement Build(ChatMessage message, UiMessageModel model)
    {
        var preview = message.ReplyPreview;
        string channel = preview?.ChannelId ?? message.ChannelId, target = preview?.MessageId ?? message.ReplyTo ?? "";
        string summary = preview?.Deleted == true ? "Mensaje eliminado" : preview?.Unavailable != false ? "Mensaje no disponible" : preview.Content.Length > 0 ? preview.Content : "Imagen o archivo adjunto";
        var text = UiPresenceViews.RichText(summary, 13, TerminalWindow.Muted, mentions: preview?.Mentions);
        text.TextWrapping = TextWrapping.NoWrap; text.TextTrimming = TextTrimming.CharacterEllipsis; text.MaxHeight = 24;
        if (preview?.Author is Person author) text.Inlines.InsertBefore(text.Inlines.FirstInline, new Run(author.Name + ": ") { FontWeight = FontWeights.SemiBold });
        var row = new DockPanel(); var arrow = TerminalWindow.Text("↳", 15, TerminalWindow.Muted); arrow.Margin = new Thickness(0, 0, 7, 0); DockPanel.SetDock(arrow, Dock.Left); row.Children.Add(arrow); row.Children.Add(text);
        var button = TerminalWindow.Button("", () => model.Action(new("jump-message", target, ChannelId: channel)));
        button.Content = row; button.HorizontalContentAlignment = HorizontalAlignment.Stretch; button.Padding = new Thickness(3, 2, 3, 2); button.Margin = new Thickness(0, 2, 0, 3);
        button.Background = System.Windows.Media.Brushes.Transparent; button.IsEnabled = target.Length > 0 && channel == message.ChannelId && preview?.Deleted != true && (model.Access == null || model.Access.CanView && model.Access.CanReadHistory);
        button.ToolTip = preview?.Deleted == true ? "El mensaje original fue eliminado" : "Ver mensaje original"; ToolTipService.SetShowOnDisabled(button, true);
        return button;
    }
}
