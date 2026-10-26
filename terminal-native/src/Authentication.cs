using System.Runtime.InteropServices;
using System.Text;
using System.Windows;
using System.Windows.Controls;
using Microsoft.Web.WebView2.Core;
using Microsoft.Web.WebView2.Wpf;

namespace NotDiscord.Terminal;

// Uses only this application's fresh sign-in profile. Never scans Discord or
// Vencord storage. A credential is captured only from this window's own normal
// authenticated HTTPS requests, and is not written until /users/@me succeeds.
public sealed class LoginWindow : Window
{
    private readonly WebView2 web = new();
    private readonly string profile;
    private bool finished;
    public string? Credential { get; private set; }
    public LoginWindow(string dataDirectory)
    {
        profile = Path.Combine(dataDirectory, "login-webview", Guid.NewGuid().ToString("N"));
        Title = "NotDiscord · Inicio de sesión de Discord"; Width = 900; Height = 760;
        var layout = new DockPanel();
        var note = new TextBlock { Text = "Inicia sesión en Discord. Esta ventana se cerrará al autenticarte; el chat usa la interfaz nativa.", Margin = new Thickness(12), TextWrapping = TextWrapping.Wrap };
        DockPanel.SetDock(note, Dock.Top); layout.Children.Add(note); layout.Children.Add(web); Content = layout;
        Loaded += async (_, _) =>
        {
            try
            {
                var environment = await CoreWebView2Environment.CreateAsync(null, profile);
                await web.EnsureCoreWebView2Async(environment);
                web.CoreWebView2.Settings.AreDevToolsEnabled = false;
                web.CoreWebView2.Settings.IsStatusBarEnabled = false;
                web.CoreWebView2.AddWebResourceRequestedFilter("https://discord.com/api/*", CoreWebView2WebResourceContext.All);
                web.CoreWebView2.WebResourceRequested += CaptureAuthentication;
                web.CoreWebView2.NewWindowRequested += (_, e) => { e.Handled = true; };
                web.CoreWebView2.NavigationStarting += (_, e) =>
                {
                    if (Uri.TryCreate(e.Uri, UriKind.Absolute, out var uri) && uri.Scheme != "about" && (uri.Scheme != "https" || (uri.Host != "discord.com" && !uri.Host.EndsWith(".discord.com", StringComparison.OrdinalIgnoreCase)))) e.Cancel = true;
                };
                web.CoreWebView2.Navigate("https://discord.com/login");
            }
            catch (Exception) { note.Text = "No se pudo abrir el inicio de sesión. Comprueba que WebView2 esté instalado y que tengas conexión."; }
        };
        Closed += (_, _) => { finished = true; if (web.CoreWebView2 != null) web.CoreWebView2.WebResourceRequested -= CaptureAuthentication; web.Dispose(); _ = DeleteTransientProfileAsync(); };
    }

    private async Task DeleteTransientProfileAsync()
    {
        // Only the unique browser profile created for THIS dialog may be removed.
        string full = Path.GetFullPath(profile);
        string parent = Path.GetFullPath(Path.GetDirectoryName(profile)!) + Path.DirectorySeparatorChar;
        if (!full.StartsWith(parent, StringComparison.OrdinalIgnoreCase) || !System.Text.RegularExpressions.Regex.IsMatch(Path.GetFileName(full), "^[a-f0-9]{32}$")) return;
        for (int attempt = 0; attempt < 12; attempt++)
        {
            await Task.Delay(1000).ConfigureAwait(false);
            try { if (Directory.Exists(full)) Directory.Delete(full, true); return; }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
        }
    }

    private void CaptureAuthentication(object? sender, CoreWebView2WebResourceRequestedEventArgs e)
    {
        if (finished || !Uri.TryCreate(e.Request.Uri, UriKind.Absolute, out var uri) || uri.Host != "discord.com" || uri.Scheme != "https") return;
        if (!e.Request.Headers.Contains("Authorization")) return;
        string credential = e.Request.Headers.GetHeader("Authorization");
        if (credential.Length < 20 || credential.Length > 4096 || credential.Contains('\r') || credential.Contains('\n')) return;
        finished = true;
        Dispatcher.BeginInvoke(() => { Credential = credential; DialogResult = true; Close(); });
    }
}

public static class SecretVault
{
    [StructLayout(LayoutKind.Sequential)] private struct Blob { public int Length; public IntPtr Data; }
    [DllImport("crypt32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CryptProtectData(ref Blob input, string? description, IntPtr entropy, IntPtr reserved, IntPtr prompt, int flags, out Blob output);
    [DllImport("crypt32.dll", CharSet = CharSet.Unicode, SetLastError = true)] private static extern bool CryptUnprotectData(ref Blob input, IntPtr description, IntPtr entropy, IntPtr reserved, IntPtr prompt, int flags, out Blob output);
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr memory);
    public static void Save(string path, string token)
    {
        var plain = Encoding.UTF8.GetBytes(token);
        try { File.WriteAllBytes(path + ".tmp", Transform(plain, true)); File.Move(path + ".tmp", path, true); }
        finally { System.Security.Cryptography.CryptographicOperations.ZeroMemory(plain); }
    }
    public static string? Load(string path)
    {
        if (!File.Exists(path)) return null;
        byte[]? plain = null;
        try { plain = Transform(File.ReadAllBytes(path), false); return Encoding.UTF8.GetString(plain); }
        catch (Exception ex) when (ex is IOException or System.ComponentModel.Win32Exception) { return null; }
        finally { if (plain != null) System.Security.Cryptography.CryptographicOperations.ZeroMemory(plain); }
    }
    private static byte[] Transform(byte[] data, bool encrypt)
    {
        var input = new Blob { Length = data.Length, Data = Marshal.AllocHGlobal(data.Length) };
        try
        {
            Marshal.Copy(data, 0, input.Data, data.Length);
            Blob output;
            bool ok = encrypt ? CryptProtectData(ref input, "NotDiscord Terminal", IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, 1, out output) : CryptUnprotectData(ref input, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, 1, out output);
            if (!ok) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            try { var result = new byte[output.Length]; Marshal.Copy(output.Data, result, 0, result.Length); return result; }
            finally { LocalFree(output.Data); }
        }
        finally { for (int i = 0; i < input.Length; i++) Marshal.WriteByte(input.Data, i, 0); Marshal.FreeHGlobal(input.Data); }
    }
}
