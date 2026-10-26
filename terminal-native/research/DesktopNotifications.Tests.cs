using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text.Json;
using NotDiscord.Terminal;

internal static class DesktopNotificationTests
{
    private static int checks;
    private static void Check(bool ok, string name) { if (!ok) throw new Exception(name); checks++; }
    [STAThread]
    public static int Main()
    {
        var mock = new MockBackend(); var opened = new List<string>(); var time = new MockTime();
        using (var notices = new DesktopNotifications(mock, opened.Add, time))
        {
            Check(mock.Shown.Count == 0, "construction never shows any notification");
            foreach (string bad in new[] { "", "x", "123 456", "https://example.invalid", new string('1', 21) }) Check(!notices.Show(bad, "Synthetic", "Synthetic"), "invalid channel rejected");
            Check(notices.Show("111", "Synthetic\nuser", new string('x', 239) + "😀"), "first eligible notice accepted");
            var first = mock.Shown[^1];
            Check(first.Title == "Synthetic user" && !first.Title.Any(char.IsControl), "sender title is sanitized without duplicating the registered application source name");
            Check(first.Body.Length <= 240 && !char.IsHighSurrogate(first.Body[^1]), "UTF16 and native body bound");
            notices.Show("222", "Second synthetic", "Body"); Check(mock.Shown.Count == 1, "one native icon at a time");
            mock.Emit(first.Id, DesktopNoticeEventKind.Activated);
            Check(opened.SequenceEqual(["111"]) && mock.Shown[^1].ChannelId == "222", "click opens its own channel then advances queue");
            mock.Emit(first.Id, DesktopNoticeEventKind.Activated); Check(opened.Count == 1, "duplicate callbacks do not open twice");
            var second = mock.Shown[^1]; mock.Emit(second.Id, DesktopNoticeEventKind.TimedOut);
            notices.Show("333", "Third synthetic", "Body");
            mock.Emit(second.Id, DesktopNoticeEventKind.Activated);
            Check(opened[^1] == "222" && mock.Shown[^1].ChannelId == "333", "late callback opens original destination, never latest");
            notices.DismissChannel("333");
            mock.Emit(mock.Shown[^1].Id, DesktopNoticeEventKind.Activated);
            Check(opened.Count == 2, "dismissed/read channel does not reopen from stale callback");
            notices.Show("444", "Message", "Body"); notices.Show("555", "Caller", "Incoming", true);
            Check(mock.Shown[^1].ChannelId == "555" && mock.Shown[^1].IsCall, "incoming call preempts an ordinary message");
            notices.DismissChannel("555");
            notices.Show("666", "Stalled", "Body"); notices.Show("777", "Next", "Body");
            time.Now = time.Now.AddSeconds(46);
            typeof(DesktopNotifications).GetMethod("ExpireStalledNotice", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(notices, null);
            Check(mock.Shown[^1].ChannelId == "777", "lost Windows callback cannot block the queue forever");
        }
        Check(mock.Disposed, "Dispose releases backend");
        var burst = new MockBackend(); using (var notices = new DesktopNotifications(burst, _ => { }))
        {
            for (int i = 1; i <= 200; i++) notices.Show(i.ToString(), "Burst", "Synthetic");
            while (burst.Current != null) burst.Emit(burst.Current.Id, DesktopNoticeEventKind.TimedOut);
            Check(burst.Shown.Count == 17, "queue retains only16 pending plus one active notice");
        }
        var coalesced = new MockBackend(); using (var notices = new DesktopNotifications(coalesced, _ => { }))
        {
            notices.Show("1", "Active", "Synthetic"); notices.Show("2", "Same channel", "old"); notices.Show("2", "Same channel", "new");
            coalesced.Emit(coalesced.Current!.Id, DesktopNoticeEventKind.TimedOut);
            Check(coalesced.Shown.Count == 2 && coalesced.Shown[^1].Body == "new", "pending channel burst coalesces");
        }
        var unavailable = new MockBackend { Accept = false }; using (var notices = new DesktopNotifications(unavailable, _ => { }))
            Check(!notices.Show("1", "Synthetic", "Synthetic") && notices.Status == unavailable.Status, "rejection reported without claiming display");
        var avatars = new MockBackend(); using (var notices = new DesktopNotifications(avatars, _ => { }))
        {
            notices.Show("11", "Author · #channel", "Preview", false, "https://cdn.discordapp.com/avatars/1/synthetic.png");
            Check(avatars.Shown.Single().AvatarUrl?.Contains("synthetic.png") == true && avatars.Shown.Single().Title == "Author · #channel", "avatar URL, sender and origin reach the native backend without a download in mocks");
        }
        Check(NotificationText.Preview("  Hola\n  mundo  ") == "Hola mundo", "notification preview normalizes whitespace");
        Check(NotificationText.Preview("<@123> <#456> <:wave:789>") == "@usuario #canal :wave:", "notification preview represents mentions and custom emoji without leaking raw markup");
        Check(new System.Globalization.StringInfo(NotificationText.Preview(string.Concat(Enumerable.Repeat("👨‍👩‍👧‍👦", 51)))).LengthInTextElements == 51, "preview truncates at fifty graphemes plus ellipsis without splitting emoji");
        NotificationIdentity.EnsureRegistered("", "");
        Check(NotificationIdentity.DisplayName == "NotDiscord" && NotificationIdentity.AppId == "com.notdiscord.Terminal.Native", "notification identity matches the existing app ID and intended display name without test registration");
        Check(Marshal.SizeOf(typeof(NotificationIdentity).GetNestedType("PropVariant", BindingFlags.NonPublic)!) == 24, "x64 PROPVARIANT includes the full native union for shell identity property storage");
        var native = typeof(DesktopNotifications).GetNestedType("ShellNotificationBackend", BindingFlags.NonPublic)!;
        var noticeData = native.GetNestedType("NativeNotice", BindingFlags.NonPublic)!;
        Check(Marshal.SizeOf(noticeData) == (IntPtr.Size == 8 ? 976 : 956), "NOTIFYICONDATAW native layout");
        byte[] pixels = Enumerable.Repeat((byte)255, 48 * 48 * 4).ToArray();
        var bitmap = System.Windows.Media.Imaging.BitmapSource.Create(48, 48, 96, 96, System.Windows.Media.PixelFormats.Pbgra32, null, pixels, 48 * 4);
        IntPtr avatarIcon = (IntPtr)native.GetMethod("CreateAvatarIcon", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, new object[] { bitmap })!;
        Check(avatarIcon != IntPtr.Zero, "a local synthetic avatar produces a native icon without shell registration or a banner");
        if (avatarIcon != IntPtr.Zero) native.GetMethod("DestroyIcon", BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, new object[] { avatarIcon });
        // Decode the documented Win32 v4 callback without constructing an HWND or touching the shell.
        object instance = RuntimeHelpers.GetUninitializedObject(native); DesktopNoticeEvent? callback = null;
        native.GetField("NoticeEvent", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(instance, new Action<DesktopNoticeEvent>(n => callback = n));
        object[] args = [IntPtr.Zero, 0x8001, IntPtr.Zero, new IntPtr((321L << 16) | 0x405), false];
        native.GetMethod("WindowMessage", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(instance, args);
        Check(callback == new DesktopNoticeEvent(321, DesktopNoticeEventKind.Activated) && (bool)args[4], "v4 callback uses HIWORD id and LOWORD event");
        Console.WriteLine(JsonSerializer.Serialize(new { success = true, checks, realWindowsNotifications = 0, userDataRead = false, networkUsed = false }));
        return 0;
    }
    private sealed class MockTime : TimeProvider { public DateTimeOffset Now = DateTimeOffset.UnixEpoch; public override DateTimeOffset GetUtcNow() => Now; }
    private sealed class MockBackend : IDesktopNotificationBackend
    {
        public bool Accept = true, Disposed;
        public List<NativeDesktopNotice> Shown = new(); public NativeDesktopNotice? Current;
        public bool IsAvailable => !Disposed;
        public string Status => "Synthetic backend state";
        public event Action<DesktopNoticeEvent>? NoticeEvent;
        public bool Show(NativeDesktopNotice value) { if (!Accept) return false; Current = value; Shown.Add(value); return true; }
        public void Hide(uint id) { if (Current?.Id == id) Current = null; }
        public void Emit(uint id, DesktopNoticeEventKind kind) => NoticeEvent?.Invoke(new(id, kind));
        public void Dispose() { Disposed = true; Current = null; }
    }
}
