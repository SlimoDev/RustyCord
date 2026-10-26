using NotDiscord.Terminal;
using System.Text.Json;

int checks = 0;
void Check(bool condition, string name) { if (!condition) throw new Exception(name); checks++; }
var normalized = new MediaPreferences { InputId = "fixture\ninput", OutputId = new string('x', 513), CameraId = "fixture-camera", InputVolume = -1, OutputVolume = 999, PttVirtualKey = 0x20 }.Normalize();
Check(normalized.InputId == "default" && normalized.OutputId == "default", "untrusted device ids rejected");
Check(normalized.CameraId == "fixture-camera", "camera selection preserved without opening");
Check(normalized.InputVolume == 0 && normalized.OutputVolume == 400, "volume range clamped");
Check(normalized.PttVirtualKey == 0xA3, "space and arbitrary keys never become a PTT default");
string fixture = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../media-settings-fixture.json"));
var selected = new MediaPreferences { InputId = "fixture-input", OutputId = "fixture-output", CameraId = "fixture-camera", InputVolume = 85, OutputVolume = 150, NoiseSuppression = false, EchoCancellation = true, PushToTalk = true, PttVirtualKey = 0x78 };
Check(selected.Save(fixture), "preference save");
Check(MediaPreferences.Load(fixture) == selected, "round trip includes PTT, devices, processing and volume");
var payload = JsonSerializer.SerializeToElement(selected.AudioParameters());
Check(payload.GetProperty("inputVolume").GetInt32() == 85 && payload.GetProperty("outputVolume").GetInt32() == 150, "helper receives percentages");
Check(payload.GetProperty("pushToTalk").GetBoolean(), "PTT helper contract");
Check(!payload.TryGetProperty("CameraId", out _) && !payload.TryGetProperty("cameraId", out _), "settings never activate or forward camera");
File.WriteAllText(fixture, "{broken");
Check(MediaPreferences.Load(fixture) == new MediaPreferences(), "malformed settings recover safe defaults");
File.WriteAllText(fixture, new string('x', 9000));
Check(MediaPreferences.Load(fixture) == new MediaPreferences(), "oversized settings not parsed");
File.Delete(fixture);
Console.WriteLine(JsonSerializer.Serialize(new { success = true, checks, microphoneOpened = false, cameraOpened = false, networkUsed = false }));
