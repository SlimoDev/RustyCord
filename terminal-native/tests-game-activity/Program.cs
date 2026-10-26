using System.Collections.Concurrent;
using System.Text.Json;
using NotDiscord.Terminal;

string root = Path.GetFullPath(args.Length > 0 ? args[0] : throw new ArgumentException("Synthetic data directory required"));
Directory.CreateDirectory(root);
string data = Path.Combine(root, Guid.NewGuid().ToString("N")); Directory.CreateDirectory(data);
string executable = Path.Combine(data, "SyntheticGame.exe"); File.WriteAllText(executable, "Test fixture; never executed.");
var failures = new List<string>(); int assertions = 0;
void Check(bool condition, string name) { assertions++; if (!condition) failures.Add(name); }
async Task Until(Func<bool> condition) { for (int i = 0; i < 200 && !condition(); i++) await Task.Delay(5); Check(condition(), "Expected synthetic state arrived"); }
int queries = 0, running = 0, failLookup = 0, failPublish = 0, attempts = 0;
var observedNames = new ConcurrentQueue<string>(); var published = new ConcurrentQueue<string?>();
await using (var monitor = new NativeGameActivity(data, name => { Interlocked.Increment(ref queries); observedNames.Enqueue(name); if (Volatile.Read(ref failLookup) != 0) throw new InvalidOperationException("Synthetic failure"); return Volatile.Read(ref running) != 0; }, TimeSpan.FromMilliseconds(10)))
{
    Check(!monitor.Configuration.Enabled && !monitor.Configuration.IsConfigured, "Default disabled and unconfigured");
    Task Publish(string? name, CancellationToken ct) { ct.ThrowIfCancellationRequested(); Interlocked.Increment(ref attempts); if (Interlocked.Exchange(ref failPublish, 0) != 0) throw new IOException("Synthetic send failure"); published.Enqueue(name); return Task.CompletedTask; }
    await monitor.StartAsync(Publish); await Task.Delay(30);
    Check(queries == 0 && published.IsEmpty, "Disabled detector does not inspect processes or publish");
    monitor.Configure(executable, "Synthetic Game");
    Check(monitor.Configuration.IsConfigured && !monitor.Configuration.Enabled, "Selecting executable does not enable monitoring");
    monitor.SetEnabled(true); await monitor.StartAsync(Publish);
    await Until(() => published.Count == 1);
    Check(published.ToArray()[0] == null, "Enabled detector reconciles initially absent game with null presence");
    Interlocked.Exchange(ref failPublish, 1); Volatile.Write(ref running, 1);
    await Until(() => published.Count == 2);
    Check(attempts >= 3 && published.ToArray()[1] == "Synthetic Game", "Failed send retries and successful state is cached only afterward");
    int before = Volatile.Read(ref queries); await Until(() => Volatile.Read(ref queries) >= before + 3);
    Check(published.Count == 2, "Unchanged running state is not republished");
    Volatile.Write(ref failLookup, 1); Volatile.Write(ref running, 0); before = Volatile.Read(ref queries); await Until(() => Volatile.Read(ref queries) >= before + 2);
    Check(published.Count == 2, "Unknown process state does not fabricate game exit");
    Volatile.Write(ref failLookup, 0); await Until(() => published.Count == 3);
    Check(published.ToArray()[2] == null, "Exiting configured game clears presence once");
    await monitor.StopAsync(); before = Volatile.Read(ref queries); await Task.Delay(30);
    Check(queries == before && published.Count == 3, "Stop cancels polling without publishing at logout");
    Volatile.Write(ref running, 1); await monitor.StartAsync(Publish); await Until(() => published.Count == 4); await monitor.StopAsync();
    Volatile.Write(ref running, 0); await monitor.StartAsync(Publish); await Until(() => published.Count == 5);
    before = Volatile.Read(ref queries); await Until(() => Volatile.Read(ref queries) >= before + 3);
    Check(published.Count == 5 && published.ToArray()[4] == null, "Reconnect after game closed publishes null exactly once");
    await monitor.StopAsync();
    Check(observedNames.All(x => x == "SyntheticGame"), "Only the configured executable name is queried");
}
await using (var restored = new NativeGameActivity(data, _ => false))
{
    Check(restored.Configuration.Enabled && restored.Configuration.ProcessName == "SyntheticGame", "Explicit opt-in persists");
    restored.Configure(executable, "New Name"); Check(!restored.Configuration.Enabled, "Reconfiguration requires opt-in again");
}
var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
await using (var cancel = new NativeGameActivity(Path.Combine(data, "cancellation"), _ => true, TimeSpan.FromMilliseconds(10)))
{
    cancel.Configure(executable, "Synthetic Game"); cancel.SetEnabled(true);
    using var session = new CancellationTokenSource();
    await cancel.StartAsync(async (_, ct) => { entered.TrySetResult(); await Task.Delay(Timeout.InfiniteTimeSpan, ct); }, session.Token);
    await entered.Task.WaitAsync(TimeSpan.FromSeconds(2)); session.Cancel();
    await cancel.StopAsync().WaitAsync(TimeSpan.FromSeconds(2)); Check(true, "Session cancellation interrupts pending publication and stop completes");
}
File.WriteAllText(Path.Combine(data, "game-activity.json"), "{\"ProcessName\":\"../Unapproved\",\"DisplayName\":\"Invalid\",\"Enabled\":true}");
await using (var invalid = new NativeGameActivity(data, _ => throw new Exception("Must not inspect"))) Check(!invalid.Configuration.Enabled && !invalid.Configuration.IsConfigured, "Invalid persisted process selection fails closed");
Console.WriteLine(JsonSerializer.Serialize(new { success = failures.Count == 0, assertions, failures, syntheticProcessesOnly = true, machineProcessEnumeration = false, executablesLaunched = 0 }, new JsonSerializerOptions { WriteIndented = true }));
return failures.Count == 0 ? 0 : 1;
