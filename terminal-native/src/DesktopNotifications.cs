using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Windows;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;

namespace NotDiscord.Terminal;

public sealed record NativeDesktopNotice(uint Id, string ChannelId, string Title, string Body, bool IsCall, string? AvatarUrl = null);
public enum DesktopNoticeEventKind { Shown, Hidden, TimedOut, Activated }
public readonly record struct DesktopNoticeEvent(uint Id, DesktopNoticeEventKind Kind);

/// <summary>Inject a fake in tests: construction never needs to contact the Windows shell.</summary>
public interface IDesktopNotificationBackend : IDisposable
{
    bool IsAvailable { get; }
    string Status { get; }
    event Action<DesktopNoticeEvent>? NoticeEvent;
    bool Show(NativeDesktopNotice notice);
    void Hide(uint id);
}

/// <summary>
/// Native Windows banners. The controller decides whether a message is eligible
/// (mute preferences, own messages, focused channel, mentions and incoming calls).
/// This class retains at most16 queued notices and64 recent channel destinations.
/// Notification text is never logged or saved to disk.
/// </summary>
public sealed class DesktopNotifications : IDisposable
{
    private const int MaximumQueued = 16, MaximumRecent = 64;
    private readonly IDesktopNotificationBackend backend;
    private readonly Action<string> onOpenChannel;
    private readonly TimeProvider clock;
    private readonly List<NativeDesktopNotice> queue = new();
    private readonly Dictionary<uint, RecentDestination> recent = new();
    private readonly Queue<uint> recentOrder = new();
    private readonly Window? owner;
    private readonly DispatcherTimer? expiryTimer;
    private NativeDesktopNotice? active;
    private DateTimeOffset activeDeadline;
    private uint nextId;
    private bool disposed;
    private string? localStatus;
    private sealed record RecentDestination(string ChannelId, bool Activated);

    public DesktopNotifications(Window owner, Action<string> onOpenChannel)
        : this(new ShellNotificationBackend(owner), onOpenChannel)
    {
        this.owner = owner;
        expiryTimer = new DispatcherTimer(DispatcherPriority.Background, owner.Dispatcher) { Interval = TimeSpan.FromSeconds(5) };
        expiryTimer.Tick += (_, _) => ExpireStalledNotice();
        owner.Closed += OwnerClosed;
    }

    public DesktopNotifications(IDesktopNotificationBackend backend, Action<string> onOpenChannel, TimeProvider? clock = null)
    {
        this.backend = backend ?? throw new ArgumentNullException(nameof(backend));
        this.onOpenChannel = onOpenChannel ?? throw new ArgumentNullException(nameof(onOpenChannel));
        this.clock = clock ?? TimeProvider.System;
        backend.NoticeEvent += OnNoticeEvent;
    }

    public bool IsAvailable => !disposed && backend.IsAvailable && nextId < ushort.MaxValue;
    /// <summary>Acceptance is not proof of visibility: Windows may suppress a banner.</summary>
    public string Status => disposed ? "Notificaciones cerradas" : nextId >= ushort.MaxValue ? "Se alcanzó el límite de identificadores de avisos. Vuelve a abrir NotDiscord." : localStatus ?? backend.Status;

    public bool Show(string channelId, string title, string body, bool isCall = false, string? avatarUrl = null)
    {
        if (owner != null && !owner.Dispatcher.CheckAccess())
            return !owner.Dispatcher.HasShutdownStarted && owner.Dispatcher.Invoke(() => Show(channelId, title, body, isCall, avatarUrl));
        if (!IsAvailable || !ValidChannel(channelId)) return false;
        string heading = Clean(title, 63); if (heading.Length == 0) heading = "NotDiscord";
        string text = Clean(body, 240);
        if (text.Length == 0) text = isCall ? "Llamada entrante" : "Nuevo mensaje";
        var notice = new NativeDesktopNotice(++nextId, channelId, heading, text, isCall, avatarUrl);
        int same = queue.FindIndex(n => n.ChannelId == channelId && n.IsCall == isCall);
        if (same >= 0) queue[same] = notice;
        else
        {
            if (queue.Count == MaximumQueued) queue.RemoveAt(0);
            // Calls are transient: do not leave an incoming call behind a message burst.
            if (isCall) queue.Insert(0, notice); else queue.Add(notice);
        }
        localStatus = null;
        if (isCall && active is { IsCall: false }) RetireActive(false);
        return active != null || DisplayNext();
    }

    /// <summary>Withdraw a channel's notices when read or when its incoming call ends.</summary>
    public void DismissChannel(string channelId)
    {
        if (owner != null && !owner.Dispatcher.CheckAccess()) { if (!owner.Dispatcher.HasShutdownStarted) owner.Dispatcher.Invoke(() => DismissChannel(channelId)); return; }
        if (disposed) return;
        queue.RemoveAll(n => n.ChannelId == channelId);
        foreach (var id in recent.Where(p => p.Value.ChannelId == channelId).Select(p => p.Key).ToArray()) recent.Remove(id);
        if (active?.ChannelId == channelId) { RetireActive(activated: true); DisplayNext(); }
    }

    private bool DisplayNext()
    {
        if (disposed || active != null || queue.Count == 0) return active != null;
        var next = queue[0]; queue.RemoveAt(0);
        // Set before calling the backend so synchronous mock callbacks are safe.
        active = next; activeDeadline = clock.GetUtcNow().AddSeconds(45);
        if (backend.Show(next)) { expiryTimer?.Start(); return true; }
        active = null; localStatus = backend.Status; expiryTimer?.Stop();
        // Keep no stale message/call bodies when the Windows shell is unavailable.
        queue.Clear(); return false;
    }

    private void OnNoticeEvent(DesktopNoticeEvent value)
    {
        if (owner != null && !owner.Dispatcher.CheckAccess()) { if (!owner.Dispatcher.HasShutdownStarted) _ = owner.Dispatcher.BeginInvoke(() => OnNoticeEvent(value)); return; }
        if (disposed) return;
        if (active?.Id != value.Id)
        {
            if (value.Kind == DesktopNoticeEventKind.Activated && recent.TryGetValue(value.Id, out var destination) && !destination.Activated)
            {
                recent[value.Id] = destination with { Activated = true }; Activate(destination.ChannelId);
            }
            return;
        }
        if (value.Kind == DesktopNoticeEventKind.Shown)
        {
            // Let Windows' accessibility timeout control display; only bound a lost callback.
            activeDeadline = clock.GetUtcNow().AddMinutes(2); return;
        }
        string channel = active.ChannelId;
        bool clicked = value.Kind == DesktopNoticeEventKind.Activated;
        RetireActive(clicked);
        if (clicked) Activate(channel);
        DisplayNext();
    }

    private void Activate(string channel)
    {
        if (owner != null)
        {
            if (!owner.IsVisible) owner.Show();
            if (owner.WindowState == WindowState.Minimized) SystemCommands.RestoreWindow(owner);
            owner.Activate();
        }
        onOpenChannel(channel);
    }

    private void RetireActive(bool activated)
    {
        if (active == null) return;
        var old = active; active = null; backend.Hide(old.Id); expiryTimer?.Stop();
        recent[old.Id] = new(old.ChannelId, activated); recentOrder.Enqueue(old.Id);
        while (recentOrder.Count > MaximumRecent) recent.Remove(recentOrder.Dequeue());
    }

    private void ExpireStalledNotice()
    {
        if (disposed || active == null || clock.GetUtcNow() < activeDeadline) return;
        RetireActive(false); DisplayNext();
    }

    private static bool ValidChannel(string? id) => id is { Length: > 0 and <= 20 } && id.All(c => c is >= '0' and <= '9');
    private static string Clean(string? value, int limit)
    {
        if (string.IsNullOrEmpty(value)) return "";
        var text = new StringBuilder(Math.Min(limit, value.Length));
        foreach (char character in value)
        {
            if (text.Length >= limit) break;
            text.Append(char.IsControl(character) ? ' ' : character);
        }
        if (text.Length > 0 && char.IsHighSurrogate(text[^1])) text.Length--;
        return text.ToString().Trim();
    }

    private void OwnerClosed(object? sender, EventArgs e) => Dispose();
    public void Dispose()
    {
        if (owner != null && !owner.Dispatcher.CheckAccess()) { if (!owner.Dispatcher.HasShutdownStarted) owner.Dispatcher.Invoke(Dispose); return; }
        if (disposed) return; disposed = true;
        expiryTimer?.Stop(); queue.Clear(); recent.Clear(); recentOrder.Clear(); active = null;
        if (owner != null) owner.Closed -= OwnerClosed;
        backend.NoticeEvent -= OnNoticeEvent; backend.Dispose();
    }

    private sealed class ShellNotificationBackend : IDesktopNotificationBackend
    {
        private const uint CallbackMessage = 0x8001;
        private const uint NifMessage = 1, NifIcon = 2, NifTip = 4, NifInfo = 0x10, NifRealtime = 0x40, NifShowTip = 0x80;
        private readonly HwndSource source;
        private readonly Window owner;
        private readonly uint taskbarCreated;
        private IntPtr icon;
        private IntPtr avatarIcon;
        private CancellationTokenSource? avatarWork;
        private NativeDesktopNotice? pendingAvatar;
        private bool ownedIcon, disposed;
        private NativeDesktopNotice? current;
        public bool IsAvailable { get; private set; } = true;
        public string Status { get; private set; } = "Disponible. Windows controla los banners, sonidos y No molestar.";
        public event Action<DesktopNoticeEvent>? NoticeEvent;

        internal ShellNotificationBackend(Window owner)
        {
            this.owner = owner;
            owner.Dispatcher.VerifyAccess();
            source = new HwndSource(new HwndSourceParameters("NotDiscord.Notifications")
            {
                WindowStyle = unchecked((int)0x80000000), ExtendedWindowStyle = 0x08000080, Width = 0, Height = 0
            });
            source.AddHook(WindowMessage); taskbarCreated = RegisterWindowMessage("TaskbarCreated");
        }

        public bool Show(NativeDesktopNotice notice)
        {
            if (disposed) return false;
            if (current != null) Hide(current.Id);
            if (pendingAvatar != null) Hide(pendingAvatar.Id);
            if (!MediaImages.IsAllowed(notice.AvatarUrl)) return ShowPrepared(notice, null);
            var work = avatarWork = new CancellationTokenSource(TimeSpan.FromSeconds(3));
            pendingAvatar = notice;
            _ = ShowWithAvatarAsync(notice, work);
            return true;
        }
        private async Task ShowWithAvatarAsync(NativeDesktopNotice notice, CancellationTokenSource work)
        {
            BitmapSource? avatar = null;
            try { avatar = await MediaImages.GetAsync(notice.AvatarUrl, 48, work.Token); }
            catch (OperationCanceledException) { }
            if (disposed || !ReferenceEquals(avatarWork, work) || pendingAvatar?.Id != notice.Id) return;
            pendingAvatar = null; avatarWork = null; work.Dispose();
            if (!ShowPrepared(notice, avatar)) NoticeEvent?.Invoke(new(notice.Id, DesktopNoticeEventKind.Hidden));
        }
        private bool ShowPrepared(NativeDesktopNotice notice, BitmapSource? avatar)
        {
            if (disposed) return false;
            try
            {
                if (current != null) Hide(current.Id);
                if (icon == IntPtr.Zero) icon = LoadApplicationIcon();
                if (avatarIcon != IntPtr.Zero) { DestroyIcon(avatarIcon); avatarIcon = IntPtr.Zero; }
                if (avatar != null) avatarIcon = CreateAvatarIcon(avatar);
                var data = Data(notice.Id); data.Flags = NifMessage | NifIcon | NifTip | NifShowTip;
                if (!ShellNotifyIcon(0, ref data)) return Failed();
                current = notice;
                data.TimeoutOrVersion = 4;
                if (!ShellNotifyIcon(4, ref data)) { Hide(notice.Id); return Failed(); }
                data.Flags = NifInfo | (notice.IsCall ? NifRealtime : 0);
                data.Info = notice.Body; data.InfoTitle = notice.Title; data.TimeoutOrVersion = 10000;
                data.InfoFlags = 4 | 0x20 | 0x80; // local icon, large icon, respect Windows quiet time
                data.BalloonIcon = avatarIcon != IntPtr.Zero ? avatarIcon : icon;
                if (!ShellNotifyIcon(1, ref data)) { Hide(notice.Id); return Failed(); }
                IsAvailable = true; Status = "Aviso enviado a Windows. Su visualización depende de los ajustes del sistema."; return true;
            }
            catch (Exception error) when (error is ExternalException or InvalidOperationException or IOException or UnauthorizedAccessException)
            {
                if (current != null) Hide(current.Id); return Failed();
            }
        }

        public void Hide(uint id)
        {
            if (pendingAvatar?.Id == id) { pendingAvatar = null; avatarWork?.Cancel(); avatarWork?.Dispose(); avatarWork = null; }
            if (disposed || current?.Id != id) return;
            var data = Data(id); data.Flags = NifInfo; data.Info = "";
            ShellNotifyIcon(1, ref data); ShellNotifyIcon(2, ref data); current = null;
            if (avatarIcon != IntPtr.Zero) { DestroyIcon(avatarIcon); avatarIcon = IntPtr.Zero; }
        }

        private bool Failed() { if (avatarIcon != IntPtr.Zero) { DestroyIcon(avatarIcon); avatarIcon = IntPtr.Zero; } IsAvailable = false; Status = "Windows no aceptó el aviso. Revisa sus notificaciones o vuelve a abrir NotDiscord."; return false; }
        private NativeNotice Data(uint id) => new()
        {
            Size = (uint)Marshal.SizeOf<NativeNotice>(), Window = source.Handle, Id = id, Callback = CallbackMessage,
            Icon = icon, Tip = "NotDiscord", Info = "", InfoTitle = ""
        };

        private IntPtr WindowMessage(IntPtr hwnd, int message, IntPtr wParam, IntPtr lParam, ref bool handled)
        {
            if (message == taskbarCreated && taskbarCreated != 0 && current != null)
            {
                // Restore only the transient tray icon; never replay private banners.
                var data = Data(current.Id); data.Flags = NifMessage | NifIcon | NifTip | NifShowTip;
                if (ShellNotifyIcon(0, ref data)) { data.TimeoutOrVersion = 4; ShellNotifyIcon(4, ref data); }
            }
            if (message != CallbackMessage) return IntPtr.Zero;
            handled = true;
            uint value = unchecked((uint)lParam.ToInt64()), id = value >> 16;
            var kind = (value & 0xFFFF) switch
            {
                0x402 => DesktopNoticeEventKind.Shown,
                0x403 => DesktopNoticeEventKind.Hidden,
                0x404 => DesktopNoticeEventKind.TimedOut,
                0x405 or 0x400 or 0x401 => DesktopNoticeEventKind.Activated,
                _ => (DesktopNoticeEventKind?)null
            };
            if (kind != null) NoticeEvent?.Invoke(new(id, kind.Value));
            return IntPtr.Zero;
        }

        private IntPtr LoadApplicationIcon()
        {
            string candidate = Path.Combine(AppContext.BaseDirectory, "assets", "discord.ico");
            if (!File.Exists(candidate))
            {
                for (var directory = new DirectoryInfo(AppContext.BaseDirectory); directory != null; directory = directory.Parent)
                    if (directory.Name.Equals("terminal-native", StringComparison.OrdinalIgnoreCase)) { candidate = Path.Combine(directory.FullName, "assets", "discord.ico"); break; }
            }
            if (File.Exists(candidate))
            {
                IntPtr result = LoadImage(IntPtr.Zero, candidate, 1, 32, 32, 0x10);
                if (result != IntPtr.Zero) { ownedIcon = true; return result; }
            }
            if (owner.Icon != null)
            {
                var visual = new DrawingVisual(); using (var drawing = visual.RenderOpen()) drawing.DrawImage(owner.Icon, new Rect(0, 0, 32, 32));
                var bitmap = new RenderTargetBitmap(32, 32, 96, 96, PixelFormats.Pbgra32); bitmap.Render(visual);
                var png = new PngBitmapEncoder(); png.Frames.Add(BitmapFrame.Create(bitmap)); using var stream = new MemoryStream(); png.Save(stream);
                byte[] bytes = stream.ToArray(); IntPtr result = CreateIconFromResourceEx(bytes, (uint)bytes.Length, true, 0x00030000, 32, 32, 0);
                if (result != IntPtr.Zero) { ownedIcon = true; return result; }
            }
            return LoadIcon(IntPtr.Zero, new IntPtr(32512));
        }

        public void Dispose()
        {
            if (disposed) return;
            if (current != null) Hide(current.Id);
            if (pendingAvatar != null) Hide(pendingAvatar.Id);
            disposed = true; source.RemoveHook(WindowMessage); source.Dispose();
            if (ownedIcon && icon != IntPtr.Zero) DestroyIcon(icon); icon = IntPtr.Zero;
        }
        private static IntPtr CreateAvatarIcon(BitmapSource image)
        {
            const int size = 48;
            var visual = new DrawingVisual();
            using (var drawing = visual.RenderOpen())
            { drawing.PushClip(new EllipseGeometry(new Point(size / 2d, size / 2d), size / 2d, size / 2d)); drawing.DrawImage(image, new Rect(0, 0, size, size)); drawing.Pop(); }
            var bitmap = new RenderTargetBitmap(size, size, 96, 96, PixelFormats.Pbgra32); bitmap.Render(visual);
            byte[] pixels = new byte[size * size * 4]; bitmap.CopyPixels(pixels, size * 4, 0);
            var information = new BitmapInfo { Size = 40, Width = size, Height = -size, Planes = 1, BitCount = 32 };
            IntPtr color = CreateDIBSection(IntPtr.Zero, ref information, 0, out var bits, IntPtr.Zero, 0), mask = IntPtr.Zero;
            if (color == IntPtr.Zero || bits == IntPtr.Zero) { if (color != IntPtr.Zero) DeleteObject(color); return IntPtr.Zero; }
            try
            {
                Marshal.Copy(pixels, 0, bits, pixels.Length); mask = CreateBitmap(size, size, 1, 1, IntPtr.Zero);
                if (mask == IntPtr.Zero) return IntPtr.Zero;
                var data = new IconInformation { IsIcon = true, Color = color, Mask = mask }; return CreateIconIndirect(ref data);
            }
            finally { DeleteObject(color); if (mask != IntPtr.Zero) DeleteObject(mask); }
        }
        [StructLayout(LayoutKind.Sequential)] private struct BitmapInfo { internal uint Size; internal int Width, Height; internal ushort Planes, BitCount; internal uint Compression, SizeImage; internal int XPels, YPels; internal uint ColorsUsed, ColorsImportant; }
        [StructLayout(LayoutKind.Sequential)] private struct IconInformation { [MarshalAs(UnmanagedType.Bool)] internal bool IsIcon; internal uint HotspotX, HotspotY; internal IntPtr Mask, Color; }
        [DllImport("gdi32.dll")] private static extern IntPtr CreateDIBSection(IntPtr dc, ref BitmapInfo information, uint usage, out IntPtr bits, IntPtr section, uint offset);
        [DllImport("gdi32.dll")] private static extern IntPtr CreateBitmap(int width, int height, uint planes, uint bits, IntPtr data);
        [DllImport("gdi32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool DeleteObject(IntPtr value);
        [DllImport("user32.dll")] private static extern IntPtr CreateIconIndirect(ref IconInformation information);

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct NativeNotice
        {
            internal uint Size; internal IntPtr Window; internal uint Id, Flags, Callback; internal IntPtr Icon;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] internal string Tip;
            internal uint State, StateMask;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] internal string Info;
            internal uint TimeoutOrVersion;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] internal string InfoTitle;
            internal uint InfoFlags; internal Guid ItemGuid; internal IntPtr BalloonIcon;
        }
        [DllImport("shell32.dll", EntryPoint = "Shell_NotifyIconW", CharSet = CharSet.Unicode)] [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool ShellNotifyIcon(uint operation, ref NativeNotice data);
        [DllImport("user32.dll", EntryPoint = "RegisterWindowMessageW", CharSet = CharSet.Unicode)] private static extern uint RegisterWindowMessage(string name);
        [DllImport("user32.dll", EntryPoint = "LoadImageW", CharSet = CharSet.Unicode)] private static extern IntPtr LoadImage(IntPtr instance, string name, uint type, int width, int height, uint flags);
        [DllImport("user32.dll", EntryPoint = "LoadIconW")] private static extern IntPtr LoadIcon(IntPtr instance, IntPtr resource);
        [DllImport("user32.dll")] private static extern IntPtr CreateIconFromResourceEx(byte[] bytes, uint size, [MarshalAs(UnmanagedType.Bool)] bool icon, uint version, int width, int height, uint flags);
        [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool DestroyIcon(IntPtr icon);
    }
}
