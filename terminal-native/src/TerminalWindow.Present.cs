namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private void GoToPresent()
    {
        if (state.View != "chat" || !state.SignedIn || state.Demo || state.Busy || !state.ChannelAccess.CanView || !state.ChannelAccess.CanReadHistory) return;
        string channel = state.SelectedChannelId;
        if (channel.Length == 0) return;
        chatViewports.Remove(channel); ++chatNavigationEpoch; ++chatInputEpoch;
        restorePositionChannel = channel; pendingChatViewport = null; pendingFocusId = ""; userScrollPending = false;
        ClearMessageHighlight();
        Handle(new("latest-messages", channel, ChannelId: channel));
    }
}
