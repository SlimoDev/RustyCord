param([string]$Destination = (Join-Path $PSScriptRoot 'dist\NotDiscord-Rust'))
$ErrorActionPreference = 'Stop'
$resolvedDestination = [IO.Path]::GetFullPath($Destination)
New-Item -ItemType Directory -Path $resolvedDestination -Force | Out-Null
$exe = Join-Path $PSScriptRoot 'target\release\NotDiscord.exe'
$addon = Join-Path $PSScriptRoot 'target\release\notdiscord_media_addon.dll'
if (!(Test-Path -LiteralPath $exe) -or !(Test-Path -LiteralPath $addon)) { throw 'Compila primero con build.ps1 -Test -Release.' }
Copy-Item -LiteralPath $exe -Destination (Join-Path $resolvedDestination 'NotDiscord.exe')
Copy-Item -LiteralPath $addon -Destination (Join-Path $resolvedDestination 'NotDiscord.Media.node')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets\discord.ico'),(Join-Path $PSScriptRoot 'assets\UNICODE-LICENSE.txt'),(Join-Path $PSScriptRoot 'README.md'),(Join-Path $PSScriptRoot 'DOCUMENTACION_PROYECTO.md') -Destination $resolvedDestination
$documentationDestination = Join-Path $resolvedDestination 'docs'
New-Item -ItemType Directory -Path $documentationDestination -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs\ARQUITECTURA_RUST.md'),(Join-Path $PSScriptRoot 'docs\CORRECCIONES_PARIDAD.md') -Destination $documentationDestination
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs\CORRECCIONES_UI_Y_LLAMADAS.md') -Destination $documentationDestination
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs\CORRECCIONES_UI3.md'),(Join-Path $PSScriptRoot 'docs\MEMORIA_Y_RECURSOS.md') -Destination $documentationDestination
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs\CORRECCIONES_UI4.md') -Destination $documentationDestination
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs\CORRECCIONES_UI5.md') -Destination $documentationDestination
foreach ($document in @('CORRECCIONES_UI6.md','UI6-MENSAJES.md','UI6-APARIENCIA.md','UI6-VOZ.md','UI6-SONIDOS.md','UI6-MULTIMEDIA.md')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('docs\'+$document)) -Destination $documentationDestination
}
foreach ($document in @('CORRECCIONES_UI7.md','UI7-CHAT.md','UI7-MENCIONES-Y-DIALOGOS.md','UI7-VOZ.md','UI7-TOAST-ACTIVACION.md','UI7-BOTS.md','UI7-PERFILES.md','UI7-NOTIFICACIONES.md','UI7-SONIDOS.md','UI7-RECURSOS-OFFLINE.md','TERCEROS_UI7.md')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('docs\'+$document)) -Destination $documentationDestination
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets\SYMPHONIA-LICENSE-MPL-2.0.txt') -Destination $documentationDestination
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs\PROTECCIONES_P0.md'),(Join-Path $PSScriptRoot 'docs\MATRIZ_DE_ACCESO_P0.md'),(Join-Path $PSScriptRoot 'docs\AUDITORIA_Y_PLAN_DE_RIESGOS_2026-10-21.md') -Destination $documentationDestination
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs\PROTECCIONES_P1.md') -Destination $documentationDestination
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs\OPTIMIZACIONES_P2.md') -Destination $documentationDestination
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'docs\CONTRATO_MEDICION_PROLONGADA_P2.md'),(Join-Path $PSScriptRoot 'docs\MODELO_DE_ACCESO_AUTORIZADO.md') -Destination $documentationDestination
$evidenceDestination = Join-Path $resolvedDestination 'evidence'
New-Item -ItemType Directory -Path $evidenceDestination -Force | Out-Null
foreach ($report in @('UI4-LLAMADAS.md','UI4-NOTIFICACIONES.md','LLAMADA-ENTRANTE-NATIVA.md','BLUR-NATIVO.md','PE-BLUR-CARGA-DIFERIDA.md','VIDEO-PIPELINE.md','blur-memory.json','memory-ui4-vs-ui3.json','owned-window-preview-probe.json','ui4-viewer-attachments-probe.jsonl','ui4-viewer-attachments-probe.meta.json','ui4-notification-image-probe.jsonl','ui4-notification-image-probe.meta.json','ui4-workspace-tests.txt','ui4-rust-host-idle-gate.txt')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\' + $report)) -Destination $evidenceDestination
}
foreach ($report in @('P0-VALIDACION.md','p0-core-tests.txt','p0-app-tests.txt','p0-workspace-tests.txt','p0-rust-host-idle-gate.txt')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\' + $report)) -Destination $evidenceDestination
}
foreach ($report in @('P1-VALIDACION.md','p1-core-tests.txt','p1-workspace-tests.txt','p1-webview-job-probe.txt','p1-rust-host-idle-gate.txt','p1-explicit-ignored-tests.txt','p1-preservation-checks.json','p1-release-build.txt','rust-source-manifest-p1.json')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\' + $report)) -Destination $evidenceDestination
}
foreach ($report in @('P2-VALIDACION.md','p2-core-tests.txt','p2-workspace-tests.txt','p2-explicit-ignored-tests.txt','p2-release-build.txt','p2-allocations-baseline.json','p2-allocations-final.json','p2-allocation-comparison.json','p2-memory-p1-vs-p2.json','rust-source-manifest-p2.json')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\' + $report)) -Destination $evidenceDestination
}
foreach ($report in @('p2-allocation-final-build.txt','p2-measure-script-validation.json','p2-memory-summary-repair.json','p2-memory-p1-vs-p2-before-summary-fix.json','p2-preservation-checks.json','p2-package-checks.json','p2-media-host-checks.json','p2-launchers.json','workspace-migration.json')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\' + $report)) -Destination $evidenceDestination
}
foreach ($report in @('P2-CIERRE-VALIDACION.md','p2-closure-checks.json','p2-closure-soaks.json','p2-emoji-cold-phases.json','p2-closure-monitor-validation.json','p2-closure-app-tests.txt','p2-closure-core-addon-tests.txt','p2-closure-app-tests-instrumentation-v2.txt','p2-closure-progress-sharing-tests.txt','p2-closure-script-parsing.json','p2-closure-preservation.json','p2-closure-appendix-preservation.json','rust-source-manifest-p2-closure.json')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\' + $report)) -Destination $evidenceDestination
}
$closureEvidence = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'evidence\p2-closure-soaks.json') -Raw | ConvertFrom-Json
foreach ($report in @('UI5-VALIDACION.md','ui5-workspace-tests.txt','ui5-emoji-comparison.json','ui5-native-viewer.json','ui5-preview-checks.json','ui5-release-build.txt','ui5-rust-host-idle-gate.txt','rust-source-manifest-ui5.json')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\' + $report)) -Destination $evidenceDestination
}
foreach ($report in @('ui5-unit-checks.json','ui5-preservation-checks.json','ui5-appendix-preservation.json','ui5-ram-deferral.json','ui5-emoji-comparison-before-renderer.json','ui5-package-checks.json','ui5-media-host-checks.json','ui5-launchers.json','ui5-validation-checks.json')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\' + $report)) -Destination $evidenceDestination
}
$ui5PreviewDestination = Join-Path $evidenceDestination 'ui5-preview'
foreach ($report in @('ui7-audio-component-provenance.json','ui7-build-identity.json','ui7-production-features.txt','ui7-production-addon-features.txt')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\'+$report)) -Destination $evidenceDestination
}
foreach ($report in @('UI6-VALIDACION.md','ui6-unit-checks.json','ui6-workspace-tests-final.txt','ui6-sounds-measurement.json','ui6-sounds-comparison.json','ui6-native-blur.json','ui6-preview-checks.json','ui6-release-build.txt','ui6-preservation-checks.json','ui6-media-host-checks.json','ui6-import-checks.json','ui6-appendix-preservation.json','ui6-rust-code-frozen.json','ui6-script-checks.json','rust-source-manifest-ui6.json')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\'+$report)) -Destination $evidenceDestination
}
$ui6PreviewDestination = Join-Path $evidenceDestination 'ui6-preview'
New-Item -ItemType Directory -Path $ui6PreviewDestination -Force | Out-Null
foreach ($preview in @('chat.png','voice.png')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\ui6-preview\'+$preview)) -Destination $ui6PreviewDestination
}
$soundEvidence = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'evidence\ui6-sounds-measurement.json') -Raw | ConvertFrom-Json
$ui7RamDeferralPath = Join-Path $PSScriptRoot 'evidence\ui7-ram-deferral.json'
$ui7RamDeferred = $false
if (Test-Path -LiteralPath $ui7RamDeferralPath) {
    if (!(Test-Path -LiteralPath $ui7RamDeferralPath -PathType Leaf) -or ((Get-Item -LiteralPath $ui7RamDeferralPath).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'El aplazamiento de RAM debe ser un archivo local propio.' }
    $ui7RamDeferral = Get-Content -LiteralPath $ui7RamDeferralPath -Raw | ConvertFrom-Json
    $ui7RamDeferred = $ui7RamDeferral.CancelledByUser -is [bool] -and $ui7RamDeferral.CancelledByUser
}
if ($ui7RamDeferred) {
    # User cancellation takes precedence over completing or validating RAM runs.
    # Its explicit record is mandatory in the resulting distribution.
    Copy-Item -LiteralPath $ui7RamDeferralPath -Destination $evidenceDestination
} else {
    # Read-only identity validation: never launches or copies diagnostic probes.
$ui7MeasurementIdentity = & (Join-Path $PSScriptRoot 'scripts\Summarize-UI7Measurements.ps1') -ValidateOnly
if ($ui7MeasurementIdentity.Success -ne $true) { throw 'Falta validar las identidades de las mediciones UI7.' }
$ui7Summary = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'evidence\ui7-measurement-summary.json') -Raw | ConvertFrom-Json
if ($ui7Summary.Success -ne $true -or @($ui7Summary.Sources).Count -ne 4) { throw 'El resumen UI7 no contiene sus cuatro informes fuente.' }
foreach ($source in $ui7MeasurementIdentity.Sources) {
    $recorded = @($ui7Summary.Sources | Where-Object Kind -eq $source.Kind)
    if ($recorded.Count -ne 1 -or $recorded[0].Sha256 -ne $source.Sha256 -or
        [IO.Path]::GetFullPath($recorded[0].Path) -ne $source.Path) { throw 'El resumen UI7 pertenece a otra revisión de sus informes fuente.' }
}
if ($ui7Summary.AudioComponentProvenance.Sha256 -ne $ui7MeasurementIdentity.AudioComponentProvenance.Sha256 -or
    $ui7Summary.AudioComponentProvenance.Path -ne $ui7MeasurementIdentity.AudioComponentProvenance.Path) {
    throw 'El resumen UI7 no conserva la procedencia verificada de audio/importación.'
}
}
foreach ($report in @('UI7-VALIDACION.md','ui7-workspace-tests-final.txt','ui7-toast-activation-final-tests.txt','ui7-unit-checks.json','ui7-release-build.txt','ui7-resource-probe-build.txt','ui7-addon-probe-build.txt','ui7-native-receive.json','ui7-preview-checks.json','ui7-native-comparison.json','ui7-resources-measurement.json','ui7-sounds-measurement.json','ui7-imports-measurement.json','ui7-measurement-summary.json','ui7-measurement-summary.md','ui7-preservation-checks.json','ui7-appendix-preservation.json','ui7-source-checks.json','ui7-script-checks.json','ui7-import-checks.json','ui7-production-addon.json','ui7-media-host-checks.json','ui7-rust-code-frozen.json','ui7-rust-code-frozen-before-toast-race.json','rust-source-manifest-ui7.json','ui7-check-final.txt','ui7-overwatch-process-check.json')) {
    if ($ui7RamDeferred -and $report -in @('ui7-resources-measurement.json','ui7-measurement-summary.json','ui7-measurement-summary.md')) { continue }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\'+$report)) -Destination $evidenceDestination
}
# Copy only reviewable diagnostic results and own framebuffers, never synthetic
# audio caches, session directories or executables used for measurement.
foreach ($measurement in @('native-comparison','resources-measurement','sounds-measurement','imports-measurement')) {
    if ($ui7RamDeferred -and $measurement -eq 'resources-measurement') { continue }
    $result = Get-Content -LiteralPath (Join-Path $PSScriptRoot ('evidence\ui7-'+$measurement+'.json')) -Raw | ConvertFrom-Json
    if (!$result.Success) { throw 'La medición UI7 requerida no terminó correctamente.' }
    foreach ($directory in @($result.Rows | ForEach-Object Directory | Select-Object -Unique)) {
        $source = Get-Item -LiteralPath $directory
        $prefix = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'evidence\ui7-'))
        if (!$source.PSIsContainer -or !$source.FullName.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase) -or ($source.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Directorio de diagnóstico UI7 no válido.' }
        $destination = Join-Path $evidenceDestination ('ui7-measurements\'+$measurement+'\'+$source.Name)
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        foreach ($name in @('result.json','own-framebuffer.png')) {
            $file = Join-Path $source.FullName $name
            if (Test-Path -LiteralPath $file) {
                if (!(Test-Path -LiteralPath $file -PathType Leaf) -or ((Get-Item -LiteralPath $file).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'La evidencia UI7 no admite directorios ni enlaces como archivos.' }
                Copy-Item -LiteralPath $file -Destination $destination
            }
        }
    }
}
$ui7Previews = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'evidence\ui7-preview-checks.json') -Raw | ConvertFrom-Json
if (!$ui7Previews.Success -or !$ui7Previews.Synthetic -or $ui7Previews.RealAccountUsed -or $ui7Previews.VisualReview -ne 'Passed') { throw 'Falta revisar las previews sintéticas UI7.' }
$ui7Native = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'evidence\ui7-native-comparison.json') -Raw | ConvertFrom-Json
$ui7ExecutableHash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
if ($ui7Previews.ExecutableSha256 -ne $ui7ExecutableHash -or $ui7Native.ExecutableHashes.UI7 -ne $ui7ExecutableHash) { throw 'El ejecutable no coincide con las previews y la medición UI7.' }
$ui7PreviewDestination = Join-Path $evidenceDestination 'ui7-preview'
New-Item -ItemType Directory -Path $ui7PreviewDestination -Force | Out-Null
foreach ($preview in $ui7Previews.Results) {
    if ($preview.Name -notin @('bots-embeds','bots-components','bot-profile','webhook-profile','search-header')) { throw 'Preview UI7 no permitida.' }
    $path = [IO.Path]::GetFullPath($preview.Image)
    if (!$path.StartsWith([IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'evidence\ui7-preview-')),[StringComparison]::OrdinalIgnoreCase)) { throw 'La preview no pertenece a este proyecto.' }
    if (((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $preview.ImageSha256) { throw 'La preview UI7 cambió después de su revisión.' }
    $fixture = Join-Path $PSScriptRoot ('evidence\ui7-preview-fixtures\'+$preview.Name+'.json')
    if (((Get-Item -LiteralPath $fixture).Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        (Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash -ne $preview.FixtureSha256) { throw 'La fixture UI7 cambió después de renderizar su preview.' }
    Copy-Item -LiteralPath $path -Destination (Join-Path $ui7PreviewDestination ($preview.Name+'.png'))
    Copy-Item -LiteralPath $fixture -Destination $ui7PreviewDestination
}
foreach ($report in @('ui7-package-checks.json','ui7-launchers.json','ui7-validation-checks.json')) {
    $path = Join-Path $PSScriptRoot ('evidence\'+$report)
    if (Test-Path -LiteralPath $path) { Copy-Item -LiteralPath $path -Destination $evidenceDestination }
}
if (!$soundEvidence.Success -or $soundEvidence.AccountUsed -or $soundEvidence.AudioDeviceOpened) { throw 'Los sonidos requieren evidencia offline sin cuenta ni salida de audio.' }
foreach ($row in $soundEvidence.Rows) {
    $source = Get-Item -LiteralPath $row.Directory
    $soundPrefix = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'evidence\ui6-sounds-'))
    if (!$source.FullName.StartsWith($soundPrefix,[StringComparison]::OrdinalIgnoreCase) -or ($source.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Directorio de medición de sonidos no válido.' }
    $name = $row.Mode+'-'+$row.Run
    if ($source.Name -ne $name -or $name -notmatch '^(baseline|idle|default|custom|distinct|warm_idle)-[0-2]$') { throw 'Caso de sonido no válido.' }
    $destination = Join-Path $evidenceDestination ('ui6-sounds\'+$name)
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $source.FullName 'result.json') -Destination $destination
}
foreach ($report in @('ui6-package-checks.json','ui6-launchers.json','ui6-validation-checks.json')) {
    $source = Join-Path $PSScriptRoot ('evidence\'+$report)
    if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $evidenceDestination }
}
New-Item -ItemType Directory -Path $ui5PreviewDestination -Force | Out-Null
foreach ($preview in @('chat.png','friends.png')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\ui5-preview\' + $preview)) -Destination $ui5PreviewDestination
}
if ($closureEvidence.Synthetic -ne $true -or $closureEvidence.RealAccountUsed -ne $false) { throw 'El cierre sólo puede empaquetar evidencia sintética.' }
$allowedClosureFiles = @('report.json','contract.json','process-samples.jsonl','progress.json','emoji-metrics.jsonl','multimedia-metrics.jsonl','stdout.log','stderr.log','panic.txt','preview.png','test.txt','binary-hash.json','emoji-cold-phase-summary.json','emoji-cold-phase-samples.jsonl','p2_offline_soak.rs','Measure-P2Soak.ps1')
foreach ($directory in $closureEvidence.EvidenceDirectories) {
    if ($directory -notmatch '^p2-[A-Za-z0-9_-]+$') { throw 'Directorio de evidencia no válido.' }
    $source = Get-Item -LiteralPath (Join-Path $PSScriptRoot ('evidence\' + $directory))
    if (!$source.PSIsContainer -or ($source.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'La evidencia debe ser un directorio local propio.' }
    $destination = Join-Path $evidenceDestination $directory
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    foreach ($file in Get-ChildItem -LiteralPath $source.FullName -File) {
        if ($file.Name -notin $allowedClosureFiles) { continue }
        if ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'La evidencia no admite enlaces.' }
        Copy-Item -LiteralPath $file.FullName -Destination $destination
    }
}
$incomingEvidence = Join-Path $evidenceDestination 'incoming-popup-native'
New-Item -ItemType Directory -Path $incomingEvidence -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'evidence\incoming-popup-native\result.json') -Destination $incomingEvidence
$blurEvidence = Join-Path $evidenceDestination 'blur-native'
New-Item -ItemType Directory -Path $blurEvidence -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'evidence\blur-native\result.json') -Destination $blurEvidence
$runtimeSource = Join-Path $PSScriptRoot 'media-host\runtime'
$runtimeDestination = Join-Path $resolvedDestination 'media-runtime'
if (!(Test-Path -LiteralPath (Join-Path $runtimeSource 'NotDiscord.Media.exe'))) { throw 'Falta preparar el runtime multimedia independiente.' }
New-Item -ItemType Directory -Path $runtimeDestination -Force | Out-Null
Get-ChildItem -LiteralPath $runtimeSource -Force | Where-Object Extension -ne '.log' | Copy-Item -Destination $runtimeDestination -Recurse -Force
@{ MediaHostExe = 'media-runtime/NotDiscord.Media.exe'; NativeRoot = 'media-runtime' } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $resolvedDestination 'runtime.json')
@'
@echo off
start "" "%~dp0NotDiscord.exe" --runtime-root "%~dp0." --data-dir "%~dp0data"
'@ | Set-Content -LiteralPath (Join-Path $resolvedDestination 'ABRIR_NOTDISCORD.cmd') -Encoding ascii
if (Test-Path -LiteralPath (Join-Path $resolvedDestination 'media-runtime\resources\app.asar')) { throw 'La distribución no debe cargar el helper anterior.' }
Get-ChildItem -LiteralPath $resolvedDestination -File | Where-Object Name -ne 'manifest.json' | ForEach-Object { [pscustomobject]@{ Name=$_.Name; Sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash } } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $resolvedDestination 'manifest.json')
Write-Output $resolvedDestination
