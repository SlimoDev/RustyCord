using System.Runtime.InteropServices;
using System.IO;
using System.Runtime.InteropServices.ComTypes;
using System.Text;
using Microsoft.Win32;

namespace NotDiscord.Terminal;

/// <summary>Per-user Windows identity for this app's existing native notification backend.</summary>
public static class NotificationIdentity
{
    public const string AppId = "com.notdiscord.Terminal.Native";
    public const string DisplayName = "NotDiscord";
    public static void EnsureRegistered(string executablePath, string iconPath)
    {
        if (!OperatingSystem.IsWindows() || !Path.IsPathFullyQualified(executablePath) || !File.Exists(executablePath)) return;
        executablePath = Path.GetFullPath(executablePath);
        iconPath = File.Exists(iconPath) ? Path.GetFullPath(iconPath) : executablePath;
        string shortcut = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), DisplayName + ".lnk");
        using var identity = Registry.CurrentUser.CreateSubKey(@"Software\Classes\AppUserModelId\" + AppId);
        bool unchanged = File.Exists(shortcut) && identity.GetValue("DisplayName") as string == DisplayName && identity.GetValue("NotDiscordExecutable") as string == executablePath && identity.GetValue("IconUri") as string == iconPath;
        if (unchanged) return;
        Directory.CreateDirectory(Path.GetDirectoryName(shortcut)!);
        object instance = new ShellLink();
        try
        {
            var link = (IShellLink)instance;
            link.SetPath(executablePath); link.SetWorkingDirectory(Path.GetDirectoryName(executablePath)!);
            link.SetDescription(DisplayName + " — aplicación de escritorio"); link.SetIconLocation(iconPath, 0);
            var properties = (IPropertyStore)instance;
            var key = new PropertyKey(new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), 5);
            var value = PropVariant.FromString(AppId);
            try { Marshal.ThrowExceptionForHR(properties.SetValue(ref key, ref value)); Marshal.ThrowExceptionForHR(properties.Commit()); }
            finally { PropVariantClear(ref value); }
            ((IPersistFile)instance).Save(shortcut, true);
            identity.SetValue("DisplayName", DisplayName, RegistryValueKind.String);
            identity.SetValue("IconUri", iconPath, RegistryValueKind.String);
            identity.SetValue("NotDiscordExecutable", executablePath, RegistryValueKind.String);
            SHChangeNotify(0x08000000, 0, IntPtr.Zero, IntPtr.Zero);
        }
        finally { if (Marshal.IsComObject(instance)) Marshal.FinalReleaseComObject(instance); }
    }

    [StructLayout(LayoutKind.Sequential)] private struct PropertyKey(Guid format, uint property) { public Guid Format = format; public uint Property = property; }
    // The app targets x64: PROPVARIANT's counted-array union makes it24 bytes,
    // including when only its LPWSTR member is used by SetValue/Clear.
    [StructLayout(LayoutKind.Explicit, Size = 24)] private struct PropVariant
    {
        [FieldOffset(0)] public ushort Type;
        [FieldOffset(8)] public IntPtr Pointer;
        public static PropVariant FromString(string text) => new() { Type = 31, Pointer = Marshal.StringToCoTaskMemUni(text) };
    }
    [ComImport, Guid("00021401-0000-0000-C000-000000000046"), ClassInterface(ClassInterfaceType.None)] private class ShellLink { }
    [ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellLink
    {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int maximum, IntPtr data, uint flags);
        void GetIDList(out IntPtr list); void SetIDList(IntPtr list);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder description, int maximum);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string description);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder directory, int maximum);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string directory);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder arguments, int maximum);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string arguments);
        void GetHotkey(out short hotkey); void SetHotkey(short hotkey); void GetShowCmd(out int command); void SetShowCmd(int command);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int maximum, out int index);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string path, int index);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string path, uint reserved); void Resolve(IntPtr window, uint flags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string path);
    }
    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyStore
    {
        [PreserveSig] int GetCount(out uint count); [PreserveSig] int GetAt(uint index, out PropertyKey key);
        [PreserveSig] int GetValue(ref PropertyKey key, out PropVariant value); [PreserveSig] int SetValue(ref PropertyKey key, ref PropVariant value); [PreserveSig] int Commit();
    }
    [DllImport("ole32.dll")] private static extern int PropVariantClear(ref PropVariant variant);
    [DllImport("shell32.dll")] private static extern void SHChangeNotify(uint notification, uint flags, IntPtr first, IntPtr second);
}
