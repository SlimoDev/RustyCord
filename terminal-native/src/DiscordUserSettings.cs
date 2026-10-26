using System.Text;

namespace NotDiscord.Terminal;

// Focused protobuf reader for observed PreloadedUserSettings.status (11), with no dependency
// on the rest of the account settings. Unknown fields are skipped, never rewritten.
public static class DiscordUserSettings
{
    public sealed record StatusValues(string? Status, bool? ShowCurrentGame);
    public static StatusValues Read(string encoded, bool fullSettings)
    {
        if (encoded.Length > 700_000) throw new ArgumentException("Los ajustes superan el límite permitido.");
        byte[] bytes;
        try { bytes = Convert.FromBase64String(encoded); } catch (FormatException) { throw new ArgumentException("Ajustes no válidos."); }
        if (bytes.Length > 512 * 1024) throw new ArgumentException("Los ajustes superan el límite permitido.");
        string? status = null; bool? show = fullSettings ? true : null;
        foreach (var field in Fields(bytes))
        {
            if (field.Number != 11 || field.Wire != 2) continue;
            foreach (var setting in Fields(field.Data))
            {
                if (setting.Number == 1 && setting.Wire == 2)
                    foreach (var value in Fields(setting.Data))
                        if (value.Number == 1 && value.Wire == 2)
                        {
                            string selected = Encoding.UTF8.GetString(value.Data);
                            if (selected is "online" or "idle" or "dnd" or "invisible") status = selected;
                        }
                if (setting.Number == 3 && setting.Wire == 2)
                {
                    show = false; // An explicitly present, empty BoolValue means false.
                    foreach (var value in Fields(setting.Data))
                        if (value.Number == 1 && value.Wire == 0) show = value.Scalar != 0;
                }
            }
        }
        return new(status, show);
    }
    public static string VisibilityDelta(bool enabled) => Convert.ToBase64String(enabled
        ? new byte[] { 0x5a, 4, 0x1a, 2, 8, 1 }
        : new byte[] { 0x5a, 2, 0x1a, 0 });

    private readonly record struct Field(int Number, int Wire, byte[] Data, ulong Scalar);
    private static IEnumerable<Field> Fields(byte[] data)
    {
        int cursor = 0, count = 0;
        while (cursor < data.Length)
        {
            if (++count > 10000) throw new ArgumentException("Ajustes con demasiados campos.");
            ulong tag = Varint(data, ref cursor);
            int number = (int)(tag >> 3), wire = (int)(tag & 7);
            if (number <= 0) throw new ArgumentException("Campo de ajustes no válido.");
            if (wire == 0) { yield return new(number, wire, Array.Empty<byte>(), Varint(data, ref cursor)); continue; }
            int length = wire switch { 1 => 8, 5 => 4, 2 => CheckedLength(Varint(data, ref cursor)), _ => throw new ArgumentException("Formato de ajustes no compatible.") };
            if (length > data.Length - cursor) throw new ArgumentException("Ajustes truncados.");
            byte[] value = wire == 2 ? data.AsSpan(cursor, length).ToArray() : Array.Empty<byte>();
            cursor += length;
            yield return new(number, wire, value, 0);
        }
    }
    private static int CheckedLength(ulong size) => size <= 512 * 1024 ? (int)size : throw new ArgumentException("Campo de ajustes demasiado grande.");
    private static ulong Varint(byte[] bytes, ref int cursor)
    {
        ulong value = 0;
        for (int shift = 0; shift < 70; shift += 7)
        {
            if (cursor >= bytes.Length) throw new ArgumentException("Ajustes truncados.");
            byte part = bytes[cursor++];
            if (shift == 63 && part > 1) throw new ArgumentException("Número de ajustes no válido.");
            value |= (ulong)(part & 127) << shift;
            if ((part & 128) == 0) return value;
        }
        throw new ArgumentException("Número de ajustes no válido.");
    }
}
