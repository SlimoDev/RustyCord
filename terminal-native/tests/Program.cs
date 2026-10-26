using System.Text.Json;

var errors = await FeatureSelfTest.RunAsync(args.Length > 0 ? args[0] : Path.Combine(AppContext.BaseDirectory, "synthetic-data"));
Console.WriteLine(JsonSerializer.Serialize(new { success = errors.Count == 0, errors, syntheticDataOnly = true }, new JsonSerializerOptions { WriteIndented = true }));
return errors.Count == 0 ? 0 : 1;
