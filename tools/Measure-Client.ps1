[CmdletBinding()]
param(
    [ValidateSet('NotDiscord', 'Discord')]
    [string] $Client = 'NotDiscord',
    [int] $RootProcessId = 0,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,63}$')]
    [string] $Scenario,
    [ValidateRange(1, 3600)]
    [int] $DurationSeconds = 45,
    [ValidateRange(0.1, 60)]
    [double] $IntervalSeconds = 1,
    [string] $OutputDirectory = (Join-Path $PSScriptRoot '..\evidence\benchmarks'),
    [string] $Notes = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# This script reads only process IDs, parent IDs, names, start times and counters.
# No process command lines, browser profiles, account data or network are read.
if ($env:OS -ne 'Windows_NT') {
    throw 'Este medidor requiere Windows.'
}

if (-not ('NotDiscord.Diagnostics.ProcessTree' -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;

namespace NotDiscord.Diagnostics {
    public sealed class ProcessEntry {
        public int Id;
        public int ParentId;
        public string Name;
    }

    public static class ProcessTree {
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct PROCESSENTRY32 {
            public uint dwSize;
            public uint cntUsage;
            public uint th32ProcessID;
            public UIntPtr th32DefaultHeapID;
            public uint th32ModuleID;
            public uint cntThreads;
            public uint th32ParentProcessID;
            public int pcPriClassBase;
            public uint dwFlags;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
            public string szExeFile;
        }

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint processId);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool Process32FirstW(IntPtr snapshot, ref PROCESSENTRY32 entry);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool Process32NextW(IntPtr snapshot, ref PROCESSENTRY32 entry);
        [DllImport("kernel32.dll")]
        private static extern bool CloseHandle(IntPtr handle);
        [DllImport("kernel32.dll")]
        private static extern uint GetActiveProcessorCount(ushort groupNumber);

        public static int LogicalProcessors() {
            uint count = GetActiveProcessorCount(0xffff);
            return count == 0 ? Environment.ProcessorCount : (int)count;
        }

        public static ProcessEntry[] Snapshot() {
            IntPtr handle = CreateToolhelp32Snapshot(2, 0);
            if (handle == new IntPtr(-1)) throw new Win32Exception(Marshal.GetLastWin32Error());
            var entries = new List<ProcessEntry>();
            try {
                var entry = new PROCESSENTRY32();
                entry.dwSize = (uint)Marshal.SizeOf(typeof(PROCESSENTRY32));
                if (!Process32FirstW(handle, ref entry))
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                do {
                    entries.Add(new ProcessEntry {
                        Id = (int)entry.th32ProcessID,
                        ParentId = (int)entry.th32ParentProcessID,
                        Name = entry.szExeFile
                    });
                } while (Process32NextW(handle, ref entry));
                int error = Marshal.GetLastWin32Error();
                if (error != 18) throw new Win32Exception(error);
            } finally {
                CloseHandle(handle);
            }
            return entries.ToArray();
        }

        public static int[] Descendants(ProcessEntry[] entries, int rootId) {
            var result = new HashSet<int>();
            result.Add(rootId);
            bool changed;
            do {
                changed = false;
                foreach (var entry in entries) {
                    if (result.Contains(entry.ParentId) && !result.Contains(entry.Id)) {
                        result.Add(entry.Id);
                        changed = true;
                    }
                }
            } while (changed);
            var array = new int[result.Count];
            result.CopyTo(array);
            Array.Sort(array);
            return array;
        }
    }
}
"@
}

$initialTree = [NotDiscord.Diagnostics.ProcessTree]::Snapshot()
if ($RootProcessId -eq 0) {
    $namedProcesses = @($initialTree | Where-Object { $_.Name -ieq "$Client.exe" })
    $namedIds = @($namedProcesses | ForEach-Object { $_.Id })
    $roots = @($namedProcesses | Where-Object { $_.ParentId -notin $namedIds })
    if ($roots.Count -ne 1) {
        $candidates = (@($roots | ForEach-Object { $_.Id }) -join ', ')
        throw "Se requiere una raiz unica de $Client.exe. Candidatas: [$candidates]. Usa -RootProcessId con el PID del launcher principal."
    }
    $RootProcessId = $roots[0].Id
}
if ($RootProcessId -lt 1) { throw 'RootProcessId debe ser un PID positivo.' }

$root = Get-Process -Id $RootProcessId -ErrorAction Stop
try {
    $rootStartTicks = $root.StartTime.ToUniversalTime().Ticks
    $rootStartUtc = $root.StartTime.ToUniversalTime().ToString('o')
    $rootName = $root.ProcessName
} finally {
    $root.Dispose()
}
$logicalProcessors = [NotDiscord.Diagnostics.ProcessTree]::LogicalProcessors()
$startedUtc = [DateTime]::UtcNow
$clock = [Diagnostics.Stopwatch]::StartNew()
$seenProcesses = @{}
$sampleList = [Collections.Generic.List[object]]::new()
$membershipList = [Collections.Generic.List[object]]::new()
$previousCounters = @{}
$previousElapsed = $null
$exitReason = 'duration_completed'
$unreadableCount = 0

function Get-TreeCounters {
    param([int] $TreeRootId, [long] $ExpectedRootStartTicks)

    $rootCheck = $null
    try {
        $rootCheck = Get-Process -Id $TreeRootId -ErrorAction Stop
        if ($rootCheck.StartTime.ToUniversalTime().Ticks -ne $ExpectedRootStartTicks) {
            return $null
        }
    } catch {
        return $null
    } finally {
        if ($null -ne $rootCheck) { $rootCheck.Dispose() }
    }

    $snapshot = [NotDiscord.Diagnostics.ProcessTree]::Snapshot()
    $selected = [NotDiscord.Diagnostics.ProcessTree]::Descendants($snapshot, $TreeRootId)
    $entryById = @{}
    foreach ($entry in $snapshot) { $entryById[$entry.Id] = $entry }
    $counters = @{}
    $missed = [Collections.Generic.List[int]]::new()
    foreach ($selectedId in $selected) {
        $process = $null
        try {
            $process = Get-Process -Id $selectedId -ErrorAction Stop
            $start = $process.StartTime.ToUniversalTime()
            # Reject an impossible stale parent link to a process older than this root.
            if ($start.Ticks -lt $ExpectedRootStartTicks) { continue }
            $key = "$selectedId/$($start.Ticks)"
            $counters[$key] = [pscustomobject]@{
                ProcessId = $selectedId
                ParentProcessId = $entryById[$selectedId].ParentId
                Name = $process.ProcessName
                StartUtc = $start.ToString('o')
                StartTicks = $start.Ticks
                CpuMilliseconds = $process.TotalProcessorTime.TotalMilliseconds
                PrivateBytes = [long] $process.PrivateMemorySize64
                WorkingSetBytes = [long] $process.WorkingSet64
            }
        } catch {
            $missed.Add($selectedId)
        } finally {
            if ($null -ne $process) { $process.Dispose() }
        }
    }
    return [pscustomobject]@{ Counters = $counters; UnreadableIds = @($missed.ToArray()) }
}

while ($true) {
    $captured = Get-TreeCounters -TreeRootId $RootProcessId -ExpectedRootStartTicks $rootStartTicks
    if ($null -eq $captured) {
        $exitReason = 'root_exited_or_pid_reused'
        break
    }
    $sampleUtc = [DateTime]::UtcNow
    $elapsed = $clock.Elapsed.TotalSeconds
    $current = $captured.Counters
    $cpuDeltaMs = 0.0
    $newUnbaselined = 0
    [long] $privateBytes = 0
    [long] $workingSetBytes = 0

    foreach ($key in $current.Keys) {
        $counter = $current[$key]
        $privateBytes += $counter.PrivateBytes
        $workingSetBytes += $counter.WorkingSetBytes
        if (-not $seenProcesses.ContainsKey($key)) {
            $seenProcesses[$key] = [pscustomobject]@{
                ProcessId = $counter.ProcessId
                ParentProcessId = $counter.ParentProcessId
                Name = $counter.Name
                StartUtc = $counter.StartUtc
            }
        }
        if ($null -ne $previousElapsed) {
            if ($previousCounters.ContainsKey($key)) {
                $cpuDeltaMs += [Math]::Max(0, $counter.CpuMilliseconds - $previousCounters[$key].CpuMilliseconds)
            } else {
                # First CPU counter for a new member is intentionally not attributed.
                # This prevents CPU used before joining the measured tree being counted.
                $newUnbaselined++
            }
        }
    }

    $intervalActual = $null
    $cpuSystemPercent = $null
    $cpuCoreEquivalents = $null
    if ($null -ne $previousElapsed) {
        $intervalActual = $elapsed - $previousElapsed
        $cpuCoreEquivalents = $cpuDeltaMs / ($intervalActual * 1000)
        $cpuSystemPercent = 100 * $cpuCoreEquivalents / $logicalProcessors
    }

    $processIds = @($current.Values | ForEach-Object { $_.ProcessId } | Sort-Object)
    $sampleList.Add([pscustomobject]@{
        TimestampUtc = $sampleUtc.ToString('o')
        ElapsedSeconds = [Math]::Round($elapsed, 4)
        IntervalSeconds = if ($null -eq $intervalActual) { $null } else { [Math]::Round($intervalActual, 4) }
        CpuDeltaMilliseconds = if ($null -eq $intervalActual) { $null } else { [Math]::Round($cpuDeltaMs, 4) }
        CpuSystemPercent = if ($null -eq $cpuSystemPercent) { $null } else { [Math]::Round($cpuSystemPercent, 4) }
        CpuCoreEquivalents = if ($null -eq $cpuCoreEquivalents) { $null } else { [Math]::Round($cpuCoreEquivalents, 4) }
        PrivateBytes = $privateBytes
        PrivateMiB = [Math]::Round($privateBytes / 1MB, 3)
        WorkingSetBytes = $workingSetBytes
        WorkingSetMiB = [Math]::Round($workingSetBytes / 1MB, 3)
        ProcessCount = $current.Count
        ProcessIds = $processIds -join ';'
        UnreadableProcessCount = $captured.UnreadableIds.Count
        NewProcessesWithoutCpuBaseline = $newUnbaselined
    })
    $membershipList.Add([pscustomobject]@{
        ElapsedSeconds = [Math]::Round($elapsed, 4)
        ProcessIds = $processIds
        UnreadableProcessIds = $captured.UnreadableIds
    })
    $unreadableCount += $captured.UnreadableIds.Count
    $previousCounters = $current
    $previousElapsed = $elapsed
    if ($elapsed -ge $DurationSeconds) { break }
    $remaining = $DurationSeconds - $elapsed
    Start-Sleep -Milliseconds ([int] [Math]::Max(1, [Math]::Min($IntervalSeconds, $remaining) * 1000))
}
$clock.Stop()
if ($sampleList.Count -eq 0) {
    throw 'La raiz finalizo antes de obtener una muestra. No se genero un benchmark valido.'
}

$samples = @($sampleList.ToArray())
$cpuSamples = @($samples | Where-Object { $null -ne $_.IntervalSeconds })
$intervalSum = ($cpuSamples | Measure-Object -Property IntervalSeconds -Sum).Sum
$cpuDeltaSum = ($cpuSamples | Measure-Object -Property CpuDeltaMilliseconds -Sum).Sum
$weightedCpuPercent = if ($intervalSum -gt 0) {
    100 * $cpuDeltaSum / (1000 * $intervalSum * $logicalProcessors)
} else { $null }

$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
[IO.Directory]::CreateDirectory($outputRoot) | Out-Null
$stamp = $startedUtc.ToString('yyyyMMddTHHmmssfffZ')
$fileBase = "$Client-$Scenario-$stamp-pid$RootProcessId"
$csvPath = Join-Path $outputRoot "$fileBase.csv"
$jsonPath = Join-Path $outputRoot "$fileBase.json"
$samples | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding UTF8

$report = [ordered]@{
    SchemaVersion = 1
    ClientLabel = $Client
    Scenario = $Scenario
    Notes = $Notes
    StartedUtc = $startedUtc.ToString('o')
    EndedUtc = [DateTime]::UtcNow.ToString('o')
    RequestedDurationSeconds = $DurationSeconds
    ActualDurationSeconds = [Math]::Round($clock.Elapsed.TotalSeconds, 4)
    RequestedIntervalSeconds = $IntervalSeconds
    ExitReason = $exitReason
    RootProcessId = $RootProcessId
    RootProcessName = $rootName
    RootStartedUtc = $rootStartUtc
    LogicalProcessors = $logicalProcessors
    ProcessEnumeration = 'Windows Toolhelp32 process snapshot; descendants of selected root only'
    CpuDenominator = 'Actual elapsed seconds multiplied by GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)'
    Summary = [ordered]@{
        SampleCount = $samples.Count
        CpuSystemPercentWeightedMean = if ($null -eq $weightedCpuPercent) { $null } else { [Math]::Round($weightedCpuPercent, 4) }
        CpuSystemPercentPeak = ($cpuSamples | Measure-Object -Property CpuSystemPercent -Maximum).Maximum
        PrivateMiBMean = [Math]::Round(($samples | Measure-Object -Property PrivateMiB -Average).Average, 3)
        PrivateMiBPeak = ($samples | Measure-Object -Property PrivateMiB -Maximum).Maximum
        WorkingSetMiBMean = [Math]::Round(($samples | Measure-Object -Property WorkingSetMiB -Average).Average, 3)
        WorkingSetMiBPeak = ($samples | Measure-Object -Property WorkingSetMiB -Maximum).Maximum
        ProcessCountMin = ($samples | Measure-Object -Property ProcessCount -Minimum).Minimum
        ProcessCountMax = ($samples | Measure-Object -Property ProcessCount -Maximum).Maximum
        UnreadableProcessObservations = $unreadableCount
        NewProcessesWithoutCpuBaseline = ($samples | Measure-Object -Property NewProcessesWithoutCpuBaseline -Sum).Sum
    }
    Limitations = @(
        'Snapshots are sequential, not an atomic OS-wide measurement.'
        'A first sample establishes CPU baselines; new members establish their own baseline and their first CPU interval is excluded.'
        'Processes that start and exit between snapshots, and the final CPU interval of processes that exit, can be missed.'
        'Working-set sums can count shared pages more than once; private bytes measure committed private allocation, not exclusively resident RAM.'
        'Children with a parent outside the selected tree, including deliberately detached processes, are excluded.'
        'If the root exits or its PID is reused, measurement stops and ExitReason records an incomplete scenario.'
        'CPU is process CPU across all threads; GPU activity is not measured.'
    )
    ProcessesSeen = @($seenProcesses.Values | Sort-Object ProcessId, StartUtc)
    Membership = @($membershipList.ToArray())
    Samples = $samples
    CsvPath = $csvPath
}
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $jsonPath -Encoding UTF8
[pscustomobject]@{
    JsonPath = $jsonPath
    CsvPath = $csvPath
    ExitReason = $exitReason
    RootProcessId = $RootProcessId
    Summary = $report.Summary
}

