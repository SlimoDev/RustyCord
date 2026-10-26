[CmdletBinding()]
param(
    [switch] $Worker,
    [string] $ChildPidFile
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$powerShellExe = (Get-Process -Id $PID).Path

if ($Worker) {
    $busyScript = '$timer = [Diagnostics.Stopwatch]::StartNew(); while ($timer.Elapsed.TotalSeconds -lt 14) { for ($i = 0; $i -lt 20000; $i++) { $v = [Math]::Sqrt($i + 1) } }'
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($busyScript))
    $child = Start-Process -FilePath $powerShellExe -ArgumentList @('-NoProfile', '-EncodedCommand', $encoded) -WindowStyle Hidden -PassThru
    [pscustomobject]@{ RootId = $PID; ChildId = $child.Id } | ConvertTo-Json | Set-Content -LiteralPath $ChildPidFile -Encoding UTF8
    $child.WaitForExit()
    $child.Dispose()
    exit 0
}

$testDir = Join-Path $PSScriptRoot ('test-output\' + [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'))
[IO.Directory]::CreateDirectory($testDir) | Out-Null
$pidPath = Join-Path $testDir 'controlled-processes.json'
$unrelatedScript = 'Start-Sleep -Seconds 14'
$unrelatedEncoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($unrelatedScript))
$unrelated = Start-Process -FilePath $powerShellExe -ArgumentList @('-NoProfile', '-EncodedCommand', $unrelatedEncoded) -WindowStyle Hidden -PassThru
$controlledRoot = Start-Process -FilePath $powerShellExe -ArgumentList @(
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"{0}"' -f $PSCommandPath),
    '-Worker', '-ChildPidFile', ('"{0}"' -f $pidPath)
) -WindowStyle Hidden -PassThru

try {
    $startupClock = [Diagnostics.Stopwatch]::StartNew()
    while (-not (Test-Path -LiteralPath $pidPath)) {
        if ($startupClock.Elapsed.TotalSeconds -gt 8) { throw 'El proceso de prueba no inicio a tiempo.' }
        Start-Sleep -Milliseconds 100
    }
    $fixture = Get-Content -LiteralPath $pidPath -Raw | ConvertFrom-Json
    $result = & (Join-Path $PSScriptRoot 'Measure-Client.ps1') -Client NotDiscord -RootProcessId $controlledRoot.Id -Scenario 'controlled-test' -DurationSeconds 5 -IntervalSeconds 0.5 -OutputDirectory $testDir -Notes 'Synthetic test only; this is not a NotDiscord performance result.'
    $report = Get-Content -LiteralPath $result.JsonPath -Raw | ConvertFrom-Json
    $observedIds = @($report.ProcessesSeen.ProcessId)
    if ($fixture.RootId -notin $observedIds) { throw 'No se incluyo la raiz controlada.' }
    if ($fixture.ChildId -notin $observedIds) { throw 'No se incluyo el proceso hijo controlado.' }
    if ($unrelated.Id -in $observedIds) { throw 'Se incluyo un proceso ajeno.' }
    if ($report.Summary.ProcessCountMax -lt 2) { throw "Conteo inesperado: $($report.Summary.ProcessCountMax)." }
    if ($report.Summary.CpuSystemPercentWeightedMean -le 0) { throw 'No se detecto CPU del hijo activo.' }
    if ($null -ne $report.Samples[0].CpuSystemPercent) { throw 'La primera muestra no debe atribuir CPU.' }
    if ($report.Summary.PrivateMiBMean -le 0 -or $report.Summary.WorkingSetMiBMean -le 0) { throw 'Memoria invalida.' }
    if ($report.ExitReason -ne 'duration_completed') { throw 'La medicion termino antes de tiempo.' }

    $maxFormulaDifference = 0.0
    foreach ($sample in $report.Samples) {
        if ($null -eq $sample.IntervalSeconds) { continue }
        $expected = 100 * $sample.CpuDeltaMilliseconds / ($sample.IntervalSeconds * 1000 * $report.LogicalProcessors)
        $difference = [Math]::Abs($expected - $sample.CpuSystemPercent)
        $maxFormulaDifference = [Math]::Max($maxFormulaDifference, $difference)
    }
    if ($maxFormulaDifference -gt 0.01) { throw 'La formula CPU no corresponde al delta y la capacidad total.' }

    # Unordered synthetic tree: two generations, one unrelated WebView2 process.
    $entries = @(
        @{ Id = 30; ParentId = 20; Name = 'msedgewebview2.exe' },
        @{ Id = 90; ParentId = 80; Name = 'msedgewebview2.exe' },
        @{ Id = 20; ParentId = 10; Name = 'msedgewebview2.exe' },
        @{ Id = 10; ParentId = 1; Name = 'NotDiscord.exe' }
    ) | ForEach-Object {
        $entry = New-Object NotDiscord.Diagnostics.ProcessEntry
        $entry.Id = $_.Id; $entry.ParentId = $_.ParentId; $entry.Name = $_.Name
        $entry
    }
    $descendants = [NotDiscord.Diagnostics.ProcessTree]::Descendants($entries, 10)
    if (($descendants -join ',') -ne '10,20,30') { throw 'Fallo de cierre transitivo o aislamiento del arbol.' }

    $validation = [ordered]@{
        Passed = $true
        TestKind = 'Controlled root + CPU-active child + unrelated process; synthetic unordered two-generation tree'
        TestedUtc = [DateTime]::UtcNow.ToString('o')
        RootId = $fixture.RootId
        ChildId = $fixture.ChildId
        ExcludedUnrelatedId = $unrelated.Id
        ObservedProcessIds = $observedIds
        MaxCpuFormulaDifferencePercentagePoints = $maxFormulaDifference
        BenchmarkJson = $result.JsonPath
        Checks = @('root included', 'child included', 'unrelated excluded', 'root and child counted, including optional console hosts', 'child CPU detected', 'baseline excludes first CPU sample', 'positive memory', 'complete interval', 'normalized CPU formula', 'transitive tree membership')
    }
    $validationPath = Join-Path $testDir 'validation.json'
    $validation | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $validationPath -Encoding UTF8
    [pscustomobject]@{ Passed = $true; ValidationPath = $validationPath; BenchmarkJson = $result.JsonPath }
} finally {
    # Test processes exit by themselves. No Stop-Process, taskkill or target mutation.
    $controlledRoot.WaitForExit(25000) | Out-Null
    $unrelated.WaitForExit(25000) | Out-Null
    $controlledRoot.Dispose()
    $unrelated.Dispose()
}


