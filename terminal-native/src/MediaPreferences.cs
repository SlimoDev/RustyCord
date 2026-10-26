using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed record MediaDevice(string Id, string Name);
public sealed record MediaDeviceInventory(IReadOnlyList<MediaDevice> Input, IReadOnlyList<MediaDevice> Output, IReadOnlyList<MediaDevice> Cameras);
public sealed record MediaPreferences
{
    public string InputId { get; init; } = "default";
    public string OutputId { get; init; } = "default";
    public string CameraId { get; init; } = "";
    public int InputVolume { get; init; } = 100;
    public int OutputVolume { get; init; } = 100;
    public bool NoiseSuppression { get; init; } = true;
    public bool EchoCancellation { get; init; } = true;
    public bool PushToTalk { get; init; }
    public int PttVirtualKey { get; init; } = 0xA3;
    public static readonly (int Key, string Label)[] PttKeys = [(0xA3, "Ctrl derecho"), (0xA5, "Alt derecho"), (0x77, "F8"), (0x78, "F9")];
    public MediaPreferences Normalize() => this with
    {
        InputId = DeviceId(InputId, "default", 512), OutputId = DeviceId(OutputId, "default", 512), CameraId = DeviceId(CameraId, "", 1024),
        InputVolume = Math.Clamp(InputVolume, 0, 200), OutputVolume = Math.Clamp(OutputVolume, 0, 400),
        PttVirtualKey = PttKeys.Any(k => k.Key == PttVirtualKey) ? PttVirtualKey : 0xA3
    };
    private static string DeviceId(string? value, string fallback, int limit) => string.IsNullOrWhiteSpace(value) || value.Length > limit || value.Any(char.IsControl) ? fallback : value;
    public object AudioParameters() => new { inputId = InputId, outputId = OutputId, inputVolume = InputVolume, outputVolume = OutputVolume, noiseSuppression = NoiseSuppression, echoCancellation = EchoCancellation, pushToTalk = PushToTalk };
    public static MediaPreferences Load(string path)
    {
        try { if (File.Exists(path) && new FileInfo(path).Length <= 8192) return (JsonSerializer.Deserialize<MediaPreferences>(File.ReadAllText(path)) ?? new()).Normalize(); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException) { }
        return new();
    }
    public bool Save(string path)
    {
        try { File.WriteAllText(path + ".tmp", JsonSerializer.Serialize(Normalize())); File.Move(path + ".tmp", path, true); return true; }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return false; }
    }
}
