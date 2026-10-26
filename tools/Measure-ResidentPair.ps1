[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][ValidateRange(1,2147483647)][int]$DiscordRootProcessId,
    [Parameter(Mandatory=$true)][ValidateRange(1,2147483647)][int]$NotDiscordRootProcessId,
    [ValidateRange(1,120)][int]$DurationSeconds=35,
    [ValidateRange(0.1,5)][double]$IntervalSeconds=1,
    [ValidatePattern('^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,80}$')][string]$Scenario='user-reported-same-channel-idle',
    [string]$Notes='User reports both clients on the same channel, no call or screen sharing. Content and activity were not independently inspected.',
    [string]$OutputDirectory=(Join-Path $PSScriptRoot '..\evidence\benchmarks')
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Windows is required.' }

# Read-only process metadata/counters; no command lines, titles, UI content,
# account/profile data or network. Window state is restricted to booleans.
if (-not ('NotDiscord.PairDiagnostics.Native' -as [type])) {
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace NotDiscord.PairDiagnostics {
 public sealed class Entry { public int Id; public int ParentId; public string Name; }
 public sealed class Counter {
  public int Id; public long StartTicks; public string StartUtc; public double CpuMilliseconds;
  public ulong WorkingSet; public ulong PrivateCommit; public ulong? PrivateWorkingSet;
  public int MemoryError; public string PrivateWorkingSetStatus;
 }
 public sealed class WindowState {
  public int TopLevelWindowCount; public bool AnyVisible; public bool AnyMinimized;
  public bool AnyMaximized; public bool IsForeground;
 }
 public static class Native {
  [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]
  private struct PROCESSENTRY32 {
   public uint dwSize,cntUsage,th32ProcessID; public UIntPtr th32DefaultHeapID;
   public uint th32ModuleID,cntThreads,th32ParentProcessID; public int pcPriClassBase;
   public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr,SizeConst=260)] public string szExeFile;
  }
  [StructLayout(LayoutKind.Sequential)] private struct FT { public uint Low,High; }
  [StructLayout(LayoutKind.Sequential)]
  private struct MEMORY_EX2 {
   public uint cb,PageFaultCount;
   public UIntPtr PeakWorkingSetSize,WorkingSetSize,QuotaPeakPagedPoolUsage,QuotaPagedPoolUsage,
    QuotaPeakNonPagedPoolUsage,QuotaNonPagedPoolUsage,PagefileUsage,PeakPagefileUsage,
    PrivateUsage,PrivateWorkingSetSize,SharedCommitUsage;
  }
  [DllImport("kernel32.dll",SetLastError=true)] private static extern IntPtr CreateToolhelp32Snapshot(uint flags,uint processId);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] private static extern bool Process32FirstW(IntPtr h,ref PROCESSENTRY32 p);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] private static extern bool Process32NextW(IntPtr h,ref PROCESSENTRY32 p);
  [DllImport("kernel32.dll")] private static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll",SetLastError=true)] private static extern IntPtr OpenProcess(uint access,bool inherit,int processId);
  [DllImport("kernel32.dll",SetLastError=true)] private static extern bool GetProcessTimes(IntPtr h,out FT creation,out FT exit,out FT kernel,out FT user);
  [DllImport("kernel32.dll")] private static extern uint GetActiveProcessorCount(ushort group);
  [DllImport("psapi.dll",SetLastError=true)] private static extern bool GetProcessMemoryInfo(IntPtr h,ref MEMORY_EX2 counters,uint cb);
  private delegate bool EnumWindowsProc(IntPtr h,IntPtr param);
  [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc callback,IntPtr param);
  [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr h,out uint processId);
  [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] private static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] private static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] private static extern IntPtr GetForegroundWindow();
  private static ulong FileTime(FT f) { return ((ulong)f.High<<32)|f.Low; }
  public static int LogicalProcessors() { uint n=GetActiveProcessorCount(0xffff); return n==0?Environment.ProcessorCount:(int)n; }
  public static Entry[] Snapshot() {
   IntPtr h=CreateToolhelp32Snapshot(2,0);
   if(h==new IntPtr(-1)) throw new Win32Exception(Marshal.GetLastWin32Error());
   var result=new List<Entry>();
   try {
    var p=new PROCESSENTRY32(); p.dwSize=(uint)Marshal.SizeOf(typeof(PROCESSENTRY32));
    if(!Process32FirstW(h,ref p)) throw new Win32Exception(Marshal.GetLastWin32Error());
    do { result.Add(new Entry { Id=(int)p.th32ProcessID, ParentId=(int)p.th32ParentProcessID, Name=p.szExeFile }); }
    while(Process32NextW(h,ref p));
    int error=Marshal.GetLastWin32Error(); if(error!=18) throw new Win32Exception(error);
   } finally { CloseHandle(h); }
   return result.ToArray();
  }
  public static int[] Descendants(Entry[] entries,int rootId) {
   var result=new HashSet<int>(); result.Add(rootId); bool changed;
   do { changed=false; foreach(var e in entries) if(result.Contains(e.ParentId)&&result.Add(e.Id)) changed=true; } while(changed);
   var a=new int[result.Count]; result.CopyTo(a); Array.Sort(a); return a;
  }
  public static Counter Read(int processId) {
   IntPtr h=OpenProcess(0x410,false,processId);
   if(h==IntPtr.Zero) h=OpenProcess(0x1000,false,processId);
   if(h==IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error());
   try {
    FT create,exit,kernel,user;
    if(!GetProcessTimes(h,out create,out exit,out kernel,out user)) throw new Win32Exception(Marshal.GetLastWin32Error());
    DateTime started=DateTime.FromFileTimeUtc((long)FileTime(create));
    var m=new MEMORY_EX2(); m.cb=(uint)Marshal.SizeOf(typeof(MEMORY_EX2));
    ulong sentinel=IntPtr.Size==8?ulong.MaxValue:uint.MaxValue;
    m.PrivateWorkingSetSize=new UIntPtr(sentinel);
    bool full=GetProcessMemoryInfo(h,ref m,m.cb);
    int error=full?0:Marshal.GetLastWin32Error();
    ulong? privateResident=null; string status="available";
    if(!full) {
     m=new MEMORY_EX2();
     m.cb=(uint)Marshal.OffsetOf(typeof(MEMORY_EX2),"PrivateWorkingSetSize").ToInt32();
     if(!GetProcessMemoryInfo(h,ref m,m.cb)) throw new Win32Exception(Marshal.GetLastWin32Error());
     status="ex2_unavailable";
    } else if(m.PrivateWorkingSetSize.ToUInt64()==sentinel) { status="ex2_field_not_populated"; }
    else if(m.PrivateWorkingSetSize.ToUInt64()>m.WorkingSetSize.ToUInt64()) { status="invalid_private_ws_exceeds_ws"; }
    else { privateResident=m.PrivateWorkingSetSize.ToUInt64(); }
    return new Counter {
     Id=processId, StartTicks=started.Ticks, StartUtc=started.ToString("o"),
     CpuMilliseconds=(FileTime(kernel)+FileTime(user))/10000.0,
     WorkingSet=m.WorkingSetSize.ToUInt64(), PrivateCommit=m.PrivateUsage.ToUInt64(),
     PrivateWorkingSet=privateResident, MemoryError=error, PrivateWorkingSetStatus=status
    };
   } finally { CloseHandle(h); }
  }
  public static WindowState Windows(int[] ids) {
   var selected=new HashSet<int>(ids); var state=new WindowState();
   IntPtr foreground=GetForegroundWindow();
   if(foreground!=IntPtr.Zero) {
    uint foregroundId; GetWindowThreadProcessId(foreground,out foregroundId);
    state.IsForeground=selected.Contains((int)foregroundId);
   }
   EnumWindows(delegate(IntPtr h,IntPtr p) {
    uint id; GetWindowThreadProcessId(h,out id);
    if(selected.Contains((int)id)) {
     state.TopLevelWindowCount++; if(IsWindowVisible(h)) state.AnyVisible=true;
     if(IsIconic(h)) state.AnyMinimized=true;
     if(IsZoomed(h)) state.AnyMaximized=true;
    }
    return true;
   },IntPtr.Zero);
   return state;
  }
 }
}
"@
}

$initial=[NotDiscord.PairDiagnostics.Native]::Snapshot()
$clients=@(
 [pscustomobject]@{Label='Discord';RootId=$DiscordRootProcessId;ExpectedName='Discord.exe';StartTicks=0L;StartUtc='';Previous=@{};PreviousElapsed=$null},
 [pscustomobject]@{Label='NotDiscord';RootId=$NotDiscordRootProcessId;ExpectedName='NotDiscord.exe';StartTicks=0L;StartUtc='';Previous=@{};PreviousElapsed=$null}
)
foreach($client in $clients) {
 $entry=@($initial | Where-Object { $_.Id -eq $client.RootId })
 if($entry.Count -ne 1 -or $entry[0].Name -ine $client.ExpectedName) { throw "Unexpected root identity for $($client.Label), PID $($client.RootId)." }
 $first=[NotDiscord.PairDiagnostics.Native]::Read($client.RootId)
 $client.StartTicks=$first.StartTicks
 $client.StartUtc=$first.StartUtc
}
$logical=[NotDiscord.PairDiagnostics.Native]::LogicalProcessors()
$started=[DateTime]::UtcNow
$timer=[Diagnostics.Stopwatch]::StartNew()
$rows=[Collections.Generic.List[object]]::new()
$members=[Collections.Generic.List[object]]::new()
$seen=@{}
$reason='duration_completed'
$sampleIndex=0
while($true) {
 $snapshot=[NotDiscord.PairDiagnostics.Native]::Snapshot()
 $entries=@{}; foreach($entry in $snapshot) { $entries[$entry.Id]=$entry }
 $pairStart=$timer.Elapsed.TotalSeconds
 foreach($client in $clients) {
  try { $rootCheck=[NotDiscord.PairDiagnostics.Native]::Read($client.RootId) }
  catch { $reason="$($client.Label)_root_unreadable_or_exited"; break }
  if($rootCheck.StartTicks -ne $client.StartTicks) { $reason="$($client.Label)_root_pid_reused"; break }
  $selected=[NotDiscord.PairDiagnostics.Native]::Descendants($snapshot,$client.RootId)
  $current=@{}; $missed=[Collections.Generic.List[int]]::new(); $privateMissing=0; $invalidPrivate=0
  [double]$commit=0; [double]$ws=0; [double]$pws=0; [double]$cpuDelta=0; $newCount=0
  foreach($id in $selected) {
   try {
    $counter=[NotDiscord.PairDiagnostics.Native]::Read($id)
    if($counter.StartTicks -lt $client.StartTicks) { continue }
    $key="$id/$($counter.StartTicks)"; $current[$key]=$counter
    $commit+=$counter.PrivateCommit; $ws+=$counter.WorkingSet
    if($null -eq $counter.PrivateWorkingSet) { $privateMissing++ } else { $pws+=$counter.PrivateWorkingSet }
    if($counter.PrivateWorkingSetStatus -eq 'invalid_private_ws_exceeds_ws') { $invalidPrivate++ }
    if($null -ne $client.PreviousElapsed) {
     if($client.Previous.ContainsKey($key)) { $cpuDelta+=[Math]::Max(0,$counter.CpuMilliseconds-$client.Previous[$key].CpuMilliseconds) }
     else { $newCount++ }
    }
    $seenKey="$($client.Label)/$key"
    if(-not $seen.ContainsKey($seenKey)) {
     $seen[$seenKey]=[pscustomobject]@{Client=$client.Label;ProcessId=$id;ParentProcessId=$entries[$id].ParentId;Name=$entries[$id].Name;StartUtc=$counter.StartUtc;PrivateWorkingSetStatus=$counter.PrivateWorkingSetStatus;MemoryApiError=$counter.MemoryError}
    }
   } catch { $missed.Add($id) }
  }
  $elapsed=$timer.Elapsed.TotalSeconds
  $interval=$null; $cpuPercent=$null
  if($null -ne $client.PreviousElapsed) {
   $interval=$elapsed-$client.PreviousElapsed
   $cpuPercent=100*$cpuDelta/(1000*$interval*$logical)
  }
  $window=[NotDiscord.PairDiagnostics.Native]::Windows([int[]]$selected)
  $rootWindow=[NotDiscord.PairDiagnostics.Native]::Windows([int[]]@($client.RootId))
  $row=[pscustomobject]@{
   Sample=$sampleIndex;Client=$client.Label;TimestampUtc=[DateTime]::UtcNow.ToString('o')
   ElapsedSeconds=[Math]::Round($elapsed,6)
   PairReadOffsetSeconds=[Math]::Round($elapsed-$pairStart,6)
   IntervalSeconds=$interval
   CpuDeltaMilliseconds=if($null -eq $interval){$null}else{$cpuDelta}
   CpuSystemPercent=$cpuPercent
   PrivateCommitBytes=$commit;PrivateCommitMiB=$commit/1MB
   WorkingSetBytes=$ws;WorkingSetMiB=$ws/1MB
   PrivateWorkingSetBytes=if($privateMissing -eq 0 -and $missed.Count -eq 0){$pws}else{$null}
   PrivateWorkingSetMiB=if($privateMissing -eq 0 -and $missed.Count -eq 0){$pws/1MB}else{$null}
   ProcessCount=$current.Count;UnreadableProcessCount=$missed.Count;UnavailablePrivateWorkingSetCount=$privateMissing
   InvalidPrivateWorkingSetCount=$invalidPrivate;NewProcessesWithoutCpuBaseline=$newCount
   RootHasTopLevelWindow=($rootWindow.TopLevelWindowCount -gt 0)
   RootAnyVisible=if($rootWindow.TopLevelWindowCount -gt 0){$rootWindow.AnyVisible}else{$null}
   RootAnyMinimized=if($rootWindow.TopLevelWindowCount -gt 0){$rootWindow.AnyMinimized}else{$null}
   RootAnyMaximized=if($rootWindow.TopLevelWindowCount -gt 0){$rootWindow.AnyMaximized}else{$null}
   RootIsForeground=$rootWindow.IsForeground
   TreeHasTopLevelWindow=($window.TopLevelWindowCount -gt 0)
   TreeAnyVisible=if($window.TopLevelWindowCount -gt 0){$window.AnyVisible}else{$null}
   TreeAnyMinimized=if($window.TopLevelWindowCount -gt 0){$window.AnyMinimized}else{$null}
   TreeAnyMaximized=if($window.TopLevelWindowCount -gt 0){$window.AnyMaximized}else{$null}
   TreeIsForeground=$window.IsForeground
  }
  $rows.Add($row)
  $members.Add([pscustomobject]@{Sample=$sampleIndex;Client=$client.Label;Identities=@($current.Keys|Sort-Object);UnreadableProcessIds=@($missed.ToArray())})
  $client.Previous=$current; $client.PreviousElapsed=$elapsed
 }
 if($reason -ne 'duration_completed' -or $timer.Elapsed.TotalSeconds -ge $DurationSeconds){break}
 $sampleIndex++
 $next=[Math]::Min($DurationSeconds,$sampleIndex*$IntervalSeconds)
 $sleepMs=[Math]::Max(1,($next-$timer.Elapsed.TotalSeconds)*1000)
 Start-Sleep -Milliseconds ([int]$sleepMs)
}
$timer.Stop()
if($rows.Count -eq 0){throw 'No samples captured.'}
function Get-Stats {
 param($Values)
 $valid=@($Values | Where-Object { $null -ne $_ } | Sort-Object)
 if($valid.Count -eq 0){return [pscustomobject]@{Count=0;Mean=$null;Median=$null;Min=$null;Max=$null}}
 $mid=[int][Math]::Floor($valid.Count/2)
 $median=if($valid.Count%2 -eq 0){($valid[$mid-1]+$valid[$mid])/2}else{$valid[$mid]}
 $aggregate=$valid|Measure-Object -Average -Minimum -Maximum
 [pscustomobject]@{Count=$valid.Count;Mean=$aggregate.Average;Median=$median;Min=$aggregate.Minimum;Max=$aggregate.Maximum}
}
$summary=[ordered]@{}
foreach($client in $clients){
 $samples=@($rows.ToArray()|Where-Object{$_.Client -eq $client.Label})
 $cpuRows=@($samples|Where-Object{$null -ne $_.IntervalSeconds})
 $intervalSum=($cpuRows|Measure-Object IntervalSeconds -Sum).Sum
 $deltaSum=($cpuRows|Measure-Object CpuDeltaMilliseconds -Sum).Sum
 $summary[$client.Label]=[ordered]@{
  SampleCount=$samples.Count
  PrivateCommitMiB=(Get-Stats @($samples|ForEach-Object{$_.PrivateCommitMiB}))
  WorkingSetMiB=(Get-Stats @($samples|ForEach-Object{$_.WorkingSetMiB}))
  PrivateWorkingSetMiB=(Get-Stats @($samples|ForEach-Object{$_.PrivateWorkingSetMiB}))
  CpuSystemPercent=(Get-Stats @($cpuRows|ForEach-Object{$_.CpuSystemPercent}))
  CpuSystemPercentWeightedMean=if($intervalSum -gt 0){100*$deltaSum/(1000*$intervalSum*$logical)}else{$null}
  ProcessCount=(Get-Stats @($samples|ForEach-Object{$_.ProcessCount}))
  UnreadableProcessObservations=($samples|Measure-Object UnreadableProcessCount -Sum).Sum
  UnavailablePrivateWorkingSetObservations=($samples|Measure-Object UnavailablePrivateWorkingSetCount -Sum).Sum
  InvalidPrivateWorkingSetObservations=($samples|Measure-Object InvalidPrivateWorkingSetCount -Sum).Sum
  NewProcessesWithoutCpuBaseline=($samples|Measure-Object NewProcessesWithoutCpuBaseline -Sum).Sum
  RootWindowStates=@($samples|Select-Object RootHasTopLevelWindow,RootAnyVisible,RootAnyMinimized,RootAnyMaximized,RootIsForeground -Unique)
  TreeWindowStates=@($samples|Select-Object TreeHasTopLevelWindow,TreeAnyVisible,TreeAnyMinimized,TreeAnyMaximized,TreeIsForeground -Unique)
 }
}
$out=[IO.Path]::GetFullPath($OutputDirectory)
[IO.Directory]::CreateDirectory($out)|Out-Null
$name="Pair-$Scenario-$($started.ToString('yyyyMMddTHHmmssfffZ'))"
$csv=Join-Path $out "$name.csv"
$json=Join-Path $out "$name.json"
$rows.ToArray()|Export-Csv -LiteralPath $csv -NoTypeInformation -Encoding UTF8
$report=[ordered]@{
 SchemaVersion=1;Scenario=$Scenario;Notes=$Notes
 StartedUtc=$started.ToString('o');EndedUtc=[DateTime]::UtcNow.ToString('o')
 RequestedDurationSeconds=$DurationSeconds;ActualDurationSeconds=$timer.Elapsed.TotalSeconds
 RequestedIntervalSeconds=$IntervalSeconds;LogicalProcessors=$logical;ExitReason=$reason
 Roots=@($clients|Select-Object Label,RootId,StartUtc,StartTicks)
 MemoryApi='GetProcessMemoryInfo PROCESS_MEMORY_COUNTERS_EX2 with EX fallback; unavailable private resident values are null'
 ProcessEnumeration='One Toolhelp32 snapshot per paired sample; full descendants of explicit roots'
 CpuDenominator='Actual interval per client multiplied by all active logical processors; deltas matched by PID and creation time'
 Summary=$summary
 Limitations=@(
  'Channel, authentication and call/screen-sharing activity are not inspected; Scenario and Notes specify any user-reported conditions.'
  'Read order is Discord then NotDiscord, sequential within one snapshot. Samples are not atomic.'
  'Window fields report IsWindowVisible/IsIconic/IsZoomed over process-owned top-level windows and whether the foreground window PID belongs to the root or selected tree; no title, URL or content is read.'
  'Window and foreground queries are sequential; visibility/maximization does not establish lack of occlusion or matching foreground status between clients.'
  'This is an observational comparison, not a controlled causal benchmark; plugin configuration, caches, uptime and background activity may differ.'
  'Working-set sums can count shared physical pages repeatedly; private commit measures allocation rather than resident memory.'
  'Private working set is summed only if available and valid for every readable member; any unavailable/inaccessible member makes the tree total null.'
  'Other memory totals include readable members only; inspect inaccessible counters before interpreting.'
  'CPU of newly seen processes is excluded until a second counter; processes that exit between samples and their final CPU interval can be missed.'
  'Detached processes outside either ancestry tree are excluded. PID creation times validate identities; root exit/reuse stops collection.'
  'GPU usage and voice/screen-sharing correctness are not tested.'
 )
 ProcessesSeen=@($seen.Values|Sort-Object Client,ProcessId,StartUtc)
 Membership=@($members.ToArray());Samples=@($rows.ToArray());CsvPath=$csv
}
$report|ConvertTo-Json -Depth 10|Set-Content -LiteralPath $json -Encoding UTF8
[pscustomobject]@{JsonPath=$json;CsvPath=$csv;ExitReason=$reason;Summary=$summary}|ConvertTo-Json -Depth 8

