using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;

namespace NotDiscord.Tools {
    [StructLayout(LayoutKind.Sequential)]
    internal struct PropertyKey {
        public Guid FormatId;
        public uint PropertyId;
        public PropertyKey(Guid id, uint pid) { FormatId = id; PropertyId = pid; }
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct PropVariant {
        public ushort Type, Reserved1, Reserved2, Reserved3;
        public IntPtr Value, Extra;
        public static PropVariant FromString(string value) {
            return new PropVariant { Type = 31, Value = Marshal.StringToCoTaskMemUni(value) };
        }
        public string AsString() {
            if (Type == 0) return null;
            if (Type != 31) throw new InvalidOperationException("Unexpected property type: " + Type);
            return Marshal.PtrToStringUni(Value);
        }
    }

    [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
    internal class ShellLinkClass {}

    [ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IShellLinkW {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder value, int count, IntPtr findData, uint flags);
        void GetIDList(out IntPtr value);
        void SetIDList(IntPtr value);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder value, int count);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string value);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder value, int count);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string value);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder value, int count);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string value);
        void GetHotkey(out short value);
        void SetHotkey(short value);
        void GetShowCmd(out int value);
        void SetShowCmd(int value);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder value, int count, out int index);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string value, int index);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string value, uint reserved);
        void Resolve(IntPtr window, uint flags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string value);
    }

    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IPropertyStore {
        [PreserveSig] int GetCount(out uint count);
        [PreserveSig] int GetAt(uint index, out PropertyKey key);
        [PreserveSig] int GetValue(ref PropertyKey key, out PropVariant value);
        [PreserveSig] int SetValue(ref PropertyKey key, ref PropVariant value);
        [PreserveSig] int Commit();
    }

    public sealed class ShortcutInfo {
        public string Path { get; set; }
        public string TargetPath { get; set; }
        public string Arguments { get; set; }
        public string WorkingDirectory { get; set; }
        public string IconPath { get; set; }
        public int IconIndex { get; set; }
        public string Description { get; set; }
        public string AppUserModelId { get; set; }
    }

    public static class Shortcut {
        private static readonly PropertyKey AppId = new PropertyKey(new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), 5);
        [DllImport("ole32.dll")] private static extern int PropVariantClear(ref PropVariant value);

        public static void Create(string file, string target, string directory, string icon, string appId) {
            object instance = new ShellLinkClass();
            try {
                var link = (IShellLinkW)instance;
                link.SetPath(target);
                link.SetArguments("");
                link.SetWorkingDirectory(directory);
                link.SetDescription("NotDiscord");
                link.SetIconLocation(icon, 0);
                link.SetShowCmd(1);
                var store = (IPropertyStore)instance;
                var key = AppId;
                var value = PropVariant.FromString(appId);
                try { Marshal.ThrowExceptionForHR(store.SetValue(ref key, ref value)); }
                finally { Marshal.ThrowExceptionForHR(PropVariantClear(ref value)); }
                Marshal.ThrowExceptionForHR(store.Commit());
                ((IPersistFile)instance).Save(file, true);
            } finally { Marshal.FinalReleaseComObject(instance); }
        }

        public static ShortcutInfo Read(string file) {
            object instance = new ShellLinkClass();
            try {
                ((IPersistFile)instance).Load(file, 0);
                var link = (IShellLinkW)instance;
                var target = new StringBuilder(32768);
                var args = new StringBuilder(32768);
                var directory = new StringBuilder(32768);
                var icon = new StringBuilder(32768);
                var description = new StringBuilder(32768);
                int iconIndex;
                link.GetPath(target, target.Capacity, IntPtr.Zero, 4);
                link.GetArguments(args, args.Capacity);
                link.GetWorkingDirectory(directory, directory.Capacity);
                link.GetIconLocation(icon, icon.Capacity, out iconIndex);
                link.GetDescription(description, description.Capacity);
                var key = AppId;
                PropVariant value;
                Marshal.ThrowExceptionForHR(((IPropertyStore)instance).GetValue(ref key, out value));
                string appId;
                try { appId = value.AsString(); }
                finally { Marshal.ThrowExceptionForHR(PropVariantClear(ref value)); }
                return new ShortcutInfo {
                    Path = file, TargetPath = target.ToString(), Arguments = args.ToString(),
                    WorkingDirectory = directory.ToString(), IconPath = icon.ToString(),
                    IconIndex = iconIndex, Description = description.ToString(), AppUserModelId = appId
                };
            } finally { Marshal.FinalReleaseComObject(instance); }
        }
    }
}
