param(
    [Parameter(Mandatory=$true)][int[]]$ProcessIds,
    [Parameter(Mandatory=$true)][string]$OutputPath,
    [ValidateRange(1,3600)][int]$Seconds=60,
    [ValidateRange(250,60000)][int]$IntervalMilliseconds=1000,
    [ValidatePattern('^[a-z0-9-]{1,40}$')][string]$Phase='unspecified',
    [switch]$IncludeChildren
)
# Explicit, opt-in numeric counters only. Does not launch/attach a debugger,
# read process contents, inspect an account or collect paths/window names.
$ErrorActionPreference='Stop'
if($ProcessIds.Count -gt 32 -or @($ProcessIds|Where-Object {$_ -le 0}).Count){throw 'Selecciona entre 1 y 32 PID válidos.'}
$output=[IO.Path]::GetFullPath($OutputPath)
foreach($ancestor in @($output,(Split-Path $output -Parent))){
    $current=$ancestor
    while($current){
        if((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'La salida no permite enlaces.'}
        $parent=[IO.Path]::GetDirectoryName($current);if($parent -eq $current){break};$current=$parent
    }
}
if(Test-Path -LiteralPath $output){throw 'El archivo de salida ya existe; no se sobrescriben mediciones.'}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class NotDiscordCounterNative050 {
    [StructLayout(LayoutKind.Sequential)] public struct Memory {
        public uint cb, PageFaultCount;
        public UIntPtr PeakWorkingSetSize, WorkingSetSize, QuotaPeakPagedPoolUsage, QuotaPagedPoolUsage,
            QuotaPeakNonPagedPoolUsage, QuotaNonPagedPoolUsage, PagefileUsage, PeakPagefileUsage,
            PrivateUsage, PrivateWorkingSetSize, SharedCommitUsage;
    }
    [DllImport("psapi.dll", SetLastError=true)] public static extern bool GetProcessMemoryInfo(IntPtr process, ref Memory counters, uint bytes);
    [StructLayout(LayoutKind.Sequential)] public struct Io {
        public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount,
            ReadTransferCount, WriteTransferCount, OtherTransferCount;
    }
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool GetProcessIoCounters(IntPtr process, out Io counters);
}
'@
$stream=[IO.File]::Open($output,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
$writer=New-Object IO.StreamWriter($stream,[Text.UTF8Encoding]::new($false))
$clock=[Diagnostics.Stopwatch]::StartNew()
$previous=@{}
# Pin a kernel-reported creation time before following any process tree. A PID
# that disappears/reappears is not silently adopted as the original process.
$identities=@{}
foreach($processId in $ProcessIds){
    $process=$null
    try {$process=[Diagnostics.Process]::GetProcessById($processId);$null=$process.Handle;$identities[$processId]=$process.StartTime.ToUniversalTime()}catch{$identities[$processId]=$null}finally{if($process){$process.Dispose()}}
}
function Test-CreationMatches([object]$Relation,[DateTime]$Created) {
    return $Relation -and $Relation.CreationDate -and [Math]::Abs(($Relation.CreationDate.ToUniversalTime()-$Created).TotalMilliseconds) -le 1
}
try {
    do {
        $relations=@(Get-CimInstance -ClassName Win32_Process -Property ProcessId,ParentProcessId,CreationDate | Select-Object ProcessId,ParentProcessId,CreationDate)
        $byId=@{}
        foreach($relation in $relations){$byId[[int]$relation.ProcessId]=$relation}
        $selected=New-Object 'System.Collections.Generic.HashSet[int]'
        foreach($processId in $ProcessIds){$null=$selected.Add($processId)}
        if($IncludeChildren){
            do {
                $added=$false
                foreach($relation in $relations){
                    $parentId=[int]$relation.ParentProcessId;$childId=[int]$relation.ProcessId
                    if(!$selected.Contains($parentId) -or $selected.Contains($childId) -or !$identities[$parentId]){continue}
                    if(!(Test-CreationMatches $byId[$parentId] $identities[$parentId]) -or !$relation.CreationDate -or $relation.CreationDate.ToUniversalTime() -lt $identities[$parentId]){continue}
                    if(!$identities.ContainsKey($childId)){
                        if($identities.Count -ge 256){throw 'Se alcanzó el máximo de identidades de proceso de la medición.'}
                        $child=$null
                        try {
                            $child=[Diagnostics.Process]::GetProcessById($childId)
                            $null=$child.Handle
                            $created=$child.StartTime.ToUniversalTime()
                            if(Test-CreationMatches $relation $created){$identities[$childId]=$created}
                        }catch{}finally{if($child){$child.Dispose()}}
                    }
                    if($identities[$childId] -and (Test-CreationMatches $relation $identities[$childId]) -and $selected.Add($childId)){$added=$true}
                }
            }while($added -and $selected.Count -le 64)
        }
        if($selected.Count -gt 64){throw 'El árbol supera el máximo de 64 procesos de la medición.'}
        foreach($processId in $selected){
            $relation=$byId[$processId]
            $record=[ordered]@{schema=1;utc=[DateTime]::UtcNow.ToString('o');elapsed_ms=$clock.ElapsedMilliseconds;phase=$Phase;pid=$processId;parent_pid=$null;created_utc=$null;working_set_bytes=$null;private_working_set_bytes=$null;private_committed_bytes=$null;cpu_total_ms=$null;cpu_normalized_percent=$null;page_faults_total=$null;handles=$null;threads=$null;io_read_bytes=$null;io_write_bytes=$null;gpu_dedicated_bytes=$null;gpu_shared_bytes=$null;visual_cache_bytes=$null;available=$false}
            if($identities[$processId]){$record.created_utc=$identities[$processId].ToString('o')}
            $process=$null
            try {
                if(!$identities[$processId]){throw 'Identidad inicial no disponible.'}
                $process=[Diagnostics.Process]::GetProcessById($processId)
                # Retain one native handle before reading creation/counters.
                # ProcessInfo-backed properties may query by PID, so reject the
                # whole sample if that original process exits during sampling.
                $null=$process.Handle;$process.Refresh()
                if($process.StartTime.ToUniversalTime() -ne $identities[$processId]){throw 'El PID fue reutilizado.'}
                if(Test-CreationMatches $relation $identities[$processId]){$record.parent_pid=[int]$relation.ParentProcessId}
                $record.working_set_bytes=$process.WorkingSet64
                $record.private_committed_bytes=$process.PrivateMemorySize64
                $record.cpu_total_ms=$process.TotalProcessorTime.TotalMilliseconds
                $record.handles=$process.HandleCount;$record.threads=$process.Threads.Count
                $memory=New-Object NotDiscordCounterNative050+Memory
                $memory.cb=[Runtime.InteropServices.Marshal]::SizeOf($memory)
                if([NotDiscordCounterNative050]::GetProcessMemoryInfo($process.Handle,[ref]$memory,$memory.cb)){
                    $record.working_set_bytes=$memory.WorkingSetSize.ToUInt64();$record.private_committed_bytes=$memory.PrivateUsage.ToUInt64()
                    $record.private_working_set_bytes=$memory.PrivateWorkingSetSize.ToUInt64();$record.page_faults_total=$memory.PageFaultCount
                }else{
                    # EX fallback omits the two EX2 fields. Missing private WS
                    # remains null, never a manufactured zero.
                    $memory.cb=[Runtime.InteropServices.Marshal]::SizeOf($memory)-2*[IntPtr]::Size
                    if([NotDiscordCounterNative050]::GetProcessMemoryInfo($process.Handle,[ref]$memory,$memory.cb)){$record.page_faults_total=$memory.PageFaultCount}
                }
                $io=New-Object NotDiscordCounterNative050+Io
                if([NotDiscordCounterNative050]::GetProcessIoCounters($process.Handle,[ref]$io)){$record.io_read_bytes=$io.ReadTransferCount;$record.io_write_bytes=$io.WriteTransferCount}
                if($process.HasExited){throw 'El proceso terminó durante la muestra.'}
                $key="$processId/$($record.created_utc)"
                $before=$previous[$key]
                if($before -and $clock.ElapsedMilliseconds -gt $before.time){$record.cpu_normalized_percent=[Math]::Max(0,100*($record.cpu_total_ms-$before.cpu)/(($clock.ElapsedMilliseconds-$before.time)*[Environment]::ProcessorCount))}
                if($previous.Count -ge 256){$previous.Clear()}
                $previous[$key]=@{time=$clock.ElapsedMilliseconds;cpu=$record.cpu_total_ms};$record.available=$true
            }catch{
                foreach($field in @('working_set_bytes','private_working_set_bytes','private_committed_bytes','cpu_total_ms','cpu_normalized_percent','page_faults_total','handles','threads','io_read_bytes','io_write_bytes')){$record[$field]=$null}
            }finally{if($process){$process.Dispose()}}
            $writer.WriteLine(($record|ConvertTo-Json -Compress))
        }
        $writer.Flush()
        if($stream.Position -gt 64MB){throw 'La medición alcanzó el límite de salida de 64 MiB.'}
        if($clock.Elapsed.TotalSeconds -ge $Seconds){break}
        Start-Sleep -Milliseconds $IntervalMilliseconds
    }while($true)
}finally{$writer.Dispose()}
