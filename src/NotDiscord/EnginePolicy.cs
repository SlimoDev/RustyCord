using System;
using System.Collections.Generic;

namespace NotDiscord;

/// <summary>
/// Builds the reversible experimental Chromium policies used by this host.
/// Flags are fixed literals: no profile values or web content become arguments.
/// </summary>
internal static class EnginePolicy
{
    internal const int SourceCacheLimitMiBPerRenderer = 32;
    private const string CompactArguments =
        "--js-flags=\"--optimize-for-size --flush-baseline-code --bytecode-old-time=60\"";
    private const string CompressionFeature =
        "CompressParkableStrings:max_disk_capacity_mb/32";

    internal static string BuildArguments(
        bool compact, bool releaseUnused, bool compressSources, bool baseline)
    {
        if (baseline) return string.Empty;

        var arguments = new List<string>(3);
        var disabled = new List<string>(2);

        if (compact) arguments.Add(CompactArguments);
        if (releaseUnused) disabled.Add("MemoryCacheStrongReference");
        if (compressSources)
        {
            arguments.Add("--enable-features=" + CompressionFeature);
            // Without this, parking is paused while the renderer is foreground.
            disabled.Add("LessAggressiveParkableString");
        }
        if (disabled.Count != 0)
            arguments.Add("--disable-features=" + string.Join(",", disabled));

        return string.Join(" ", arguments);
    }

    internal static IReadOnlyDictionary<string, bool> SelfTestResults()
    {
        var checks = new Dictionary<string, bool>
        {
            ["engineNoPoliciesReturnsEmpty"] =
                BuildArguments(false, false, false, false) == string.Empty,
            ["engineExistingCompactFlagsPreserved"] =
                BuildArguments(true, false, false, false) == CompactArguments,
            ["engineCachePolicyDoesNotChangeV8"] =
                BuildArguments(false, true, false, false) ==
                "--disable-features=MemoryCacheStrongReference",
            ["engineCompressionHasBoundedCacheAndVisibleParking"] =
                BuildArguments(false, false, true, false) ==
                "--enable-features=CompressParkableStrings:max_disk_capacity_mb/32 " +
                "--disable-features=LessAggressiveParkableString",
            ["engineCombinedPoliciesUseSingleFeatureLists"] =
                BuildArguments(true, true, true, false) ==
                CompactArguments + " " +
                "--enable-features=CompressParkableStrings:max_disk_capacity_mb/32 " +
                "--disable-features=MemoryCacheStrongReference,LessAggressiveParkableString"
        };

        // Exhaustively check all switches, including baseline overriding every
        // preference. This does not start a browser or touch a user profile.
        for (int mask = 0; mask < 16; mask++)
        {
            bool compact = (mask & 1) != 0;
            bool releaseUnused = (mask & 2) != 0;
            bool compressSources = (mask & 4) != 0;
            bool baseline = (mask & 8) != 0;
            string args = BuildArguments(compact, releaseUnused, compressSources, baseline);
            bool ok = baseline
                ? args.Length == 0
                : args.Contains("--js-flags=", StringComparison.Ordinal) == compact
                    && args.Contains("MemoryCacheStrongReference", StringComparison.Ordinal) == releaseUnused
                    && args.Contains(CompressionFeature, StringComparison.Ordinal) == compressSources
                    && args.Contains("LessAggressiveParkableString", StringComparison.Ordinal) == compressSources
                    && Count(args, "--enable-features=") == (compressSources ? 1 : 0)
                    && Count(args, "--disable-features=") == (releaseUnused || compressSources ? 1 : 0)
                    && args == args.Trim();
            checks["enginePreferenceCombination" + mask.ToString("D2")] = ok;
        }
        return checks;
    }

    private static int Count(string text, string token)
    {
        int count = 0, offset = 0;
        while ((offset = text.IndexOf(token, offset, StringComparison.Ordinal)) >= 0)
        {
            count++;
            offset += token.Length;
        }
        return count;
    }
}