using System.Diagnostics;
using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed record NativeGameActivityConfiguration(string ProcessName = "", string DisplayName = "", bool Enabled = false)
{
    public bool IsConfigured => ProcessName.Length > 0 && DisplayName.Length > 0;
}

/// <summary>Opt-in detection of one explicitly configured executable name; never launches programs.</summary>
public sealed class NativeGameActivity : IAsyncDisposable
{
    private readonly string configurationPath;
    private readonly Func<string, bool> isRunning;
    private readonly TimeSpan interval;
    private readonly object stateLock = new();
    private readonly SemaphoreSlim lifecycle = new(1, 1);
    private NativeGameActivityConfiguration configuration = new();
    private CancellationTokenSource? run;
    private Task? worker;
    private bool disposed;

    public NativeGameActivity(string dataDirectory, Func<string, bool>? isRunning = null, TimeSpan? interval = null)
    {
        configurationPath = Path.Combine(Path.GetFullPath(dataDirectory), "game-activity.json");
        this.isRunning = isRunning ?? IsConfiguredProcessRunning;
        this.interval = interval ?? TimeSpan.FromSeconds(10);
        if (this.interval <= TimeSpan.Zero || this.interval > TimeSpan.FromMinutes(5)) throw new ArgumentOutOfRangeException(nameof(interval));
        Directory.CreateDirectory(Path.GetDirectoryName(configurationPath)!);
        configuration = ReadConfiguration();
    }

    public NativeGameActivityConfiguration Configuration { get { lock (stateLock) return configuration; } }

    public void Configure(string executablePath, string displayName)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(executablePath);
        ArgumentException.ThrowIfNullOrWhiteSpace(displayName);
        if (!Path.IsPathFullyQualified(executablePath) || !Path.GetExtension(executablePath).Equals(".exe", StringComparison.OrdinalIgnoreCase) || !File.Exists(executablePath))
            throw new ArgumentException("Selecciona un archivo .exe existente; no se ejecutará.", nameof(executablePath));
        string processName = Path.GetFileNameWithoutExtension(executablePath);
        var next = new NativeGameActivityConfiguration(processName, displayName.Trim(), false);
        if (!Valid(next)) throw new ArgumentException("El ejecutable o el nombre de actividad no es válido.");
        lock (stateLock)
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            if (worker is { IsCompleted: false }) throw new InvalidOperationException("Detén la detección antes de cambiar el juego.");
            SaveConfiguration(next); configuration = next;
        }
    }

    public void SetEnabled(bool enabled)
    {
        lock (stateLock)
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            if (enabled && !configuration.IsConfigured) throw new InvalidOperationException("Configura un juego antes de activar su detección.");
            if (worker is { IsCompleted: false }) throw new InvalidOperationException("Detén la detección antes de cambiar su configuración.");
            var next = configuration with { Enabled = enabled };
            SaveConfiguration(next); configuration = next;
        }
    }

    /// <summary>Starts a background monitor and returns immediately. The publisher must guard account/session identity.</summary>
    public async Task StartAsync(Func<string?, CancellationToken, Task> publish, CancellationToken sessionToken = default)
    {
        ArgumentNullException.ThrowIfNull(publish);
        await lifecycle.WaitAsync(sessionToken);
        try
        {
            await StopCoreAsync();
            lock (stateLock)
            {
                ObjectDisposedException.ThrowIf(disposed, this);
                if (!configuration.Enabled || !configuration.IsConfigured || sessionToken.IsCancellationRequested) return;
                var selected = configuration;
                run = CancellationTokenSource.CreateLinkedTokenSource(sessionToken);
                var cancellation = run.Token;
                worker = Task.Run(() => MonitorAsync(selected, publish, cancellation), CancellationToken.None);
            }
        }
        finally { lifecycle.Release(); }
    }

    private async Task MonitorAsync(NativeGameActivityConfiguration selected, Func<string?, CancellationToken, Task> publish, CancellationToken cancellation)
    {
        string? lastPublished = null;
        bool hasPublished = false;
        try
        {
            while (true)
            {
                cancellation.ThrowIfCancellationRequested();
                try
                {
                    string? activity = isRunning(selected.ProcessName) ? selected.DisplayName : null;
                    cancellation.ThrowIfCancellationRequested();
                    if (!hasPublished || !string.Equals(activity, lastPublished, StringComparison.Ordinal))
                    {
                        await publish(activity, cancellation);
                        cancellation.ThrowIfCancellationRequested();
                        lastPublished = activity;
                        hasPublished = true;
                    }
                }
                catch (OperationCanceledException) when (cancellation.IsCancellationRequested) { throw; }
                catch (Exception error) when (error is not OutOfMemoryException)
                {
                    // Unknown process state or a failed Gateway write is not a state change.
                    // Retry at the next interval; never log process names or raw exceptions.
                }
                await Task.Delay(interval, cancellation);
            }
        }
        catch (OperationCanceledException) when (cancellation.IsCancellationRequested) { }
    }

    /// <summary>Cancels detection without publishing anything, including during logout.</summary>
    public async Task StopAsync()
    {
        await lifecycle.WaitAsync();
        try { await StopCoreAsync(); }
        finally { lifecycle.Release(); }
    }

    private async Task StopCoreAsync()
    {
        CancellationTokenSource? previous; Task? pending;
        lock (stateLock) { previous = run; pending = worker; run = null; worker = null; }
        if (previous == null) return;
        previous.Cancel();
        try { if (pending != null) await pending; }
        finally { previous.Dispose(); }
    }

    public async ValueTask DisposeAsync()
    {
        await lifecycle.WaitAsync();
        try
        {
            lock (stateLock) { if (disposed) return; disposed = true; }
            await StopCoreAsync();
        }
        finally { lifecycle.Release(); }
    }

    private static bool IsConfiguredProcessRunning(string processName)
    {
        // Only the selected name is queried; no application inventory or window titles.
        Process[] matches = Process.GetProcessesByName(processName);
        try { return matches.Length > 0; }
        finally { foreach (var process in matches) process.Dispose(); }
    }

    private static bool Valid(NativeGameActivityConfiguration value) =>
        value.ProcessName is { Length: > 0 and <= 128 } && value.ProcessName is not "." and not ".." &&
        value.ProcessName.IndexOfAny(Path.GetInvalidFileNameChars()) < 0 && !value.ProcessName.Any(char.IsControl) &&
        value.DisplayName is { Length: > 0 and <= 128 } && !string.IsNullOrWhiteSpace(value.DisplayName) && !value.DisplayName.Any(char.IsControl);

    private NativeGameActivityConfiguration ReadConfiguration()
    {
        try
        {
            if (!File.Exists(configurationPath) || new FileInfo(configurationPath).Length > 4096) return new();
            var value = JsonSerializer.Deserialize<NativeGameActivityConfiguration>(File.ReadAllText(configurationPath));
            return value != null && Valid(value) ? value : new();
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or JsonException) { return new(); }
    }

    private void SaveConfiguration(NativeGameActivityConfiguration next)
    {
        string temporary = configurationPath + "." + Guid.NewGuid().ToString("N") + ".tmp";
        try
        {
            File.WriteAllText(temporary, JsonSerializer.Serialize(new { next.ProcessName, next.DisplayName, next.Enabled }));
            File.Move(temporary, configurationPath, overwrite: true);
        }
        finally { if (File.Exists(temporary)) File.Delete(temporary); }
    }
}
