using System.Diagnostics;
using System.Text.Json;
using NotDiscord.Terminal;

string root = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../"));
int childId;
await using (var bridge = new MediaBridge(root))
{
    childId = bridge.ProcessId;
    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(15));
    var capabilities = await bridge.CommandAsync("capabilities", ct: timeout.Token);
    if (!capabilities.GetProperty("voice").GetBoolean()) throw new Exception("Missing voice code path");
    var stats = await bridge.CommandAsync("stats", ct: timeout.Token);
    if (!stats.GetProperty("sanitized").GetBoolean() || stats.GetProperty("voice").ValueKind != JsonValueKind.Null || stats.GetProperty("share").ValueKind != JsonValueKind.Null)
        throw new Exception("Unexpected media initialization");
}
bool childExited;
try { using var child = Process.GetProcessById(childId); childExited = child.HasExited; }
catch (ArgumentException) { childExited = true; }
if (!childExited) throw new Exception("Owned media process did not exit");
Console.WriteLine(JsonSerializer.Serialize(new { success = true, pipePeerVerified = true, capabilitiesReceived = true, noMediaSessionsCreated = true, childExited }));

namespace NotDiscord.Terminal
{
    public sealed class ServiceException(string message) : Exception(message);
    public static class Controller
    {
        public static string Str(JsonElement value, string name) => value.TryGetProperty(name, out var property) && property.ValueKind == JsonValueKind.String ? property.GetString() ?? "" : "";
    }
}
