using System.Globalization;

namespace NotDiscord.Terminal;

public static class NavigationOrder
{
    public static ulong MessageOrder(string id) => ulong.TryParse(id, NumberStyles.None, CultureInfo.InvariantCulture, out var result) ? result : 0;
    public static IEnumerable<NavItem> DirectMessages(IEnumerable<NavItem> items) => items.OrderByDescending(n => n.HasActiveCall).ThenByDescending(n => n.IncomingCallRinging).ThenByDescending(n => n.IsPinned)
        .ThenByDescending(n => MessageOrder(n.LastMessageId)).ThenByDescending(n => MessageOrder(n.Id));
}
