param([string]$Package = (Join-Path $PSScriptRoot 'dist\NotDiscord-Rust'))
$ErrorActionPreference = 'Stop'
$packageRoot = (Resolve-Path -LiteralPath $Package).Path
function Read-NativeHeader([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($reader.ReadUInt16() -ne 0x5A4D) { throw 'Cabecera DOS inválida.' }
        $stream.Position = 0x3C
        $peOffset = $reader.ReadInt32()
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x4550) { throw 'Cabecera PE inválida.' }
        $machine = $reader.ReadUInt16()
        $stream.Position = $peOffset + 24
        $magic = $reader.ReadUInt16()
        $directoryOffset = switch ($magic) { 0x20B {112} 0x10B {96} default {throw 'Formato PE no reconocido.'} }
        $stream.Position = $peOffset + 24 + $directoryOffset + 14 * 8
        [pscustomobject]@{ Machine=$machine; ClrHeader=$reader.ReadUInt32() }
    } finally { $reader.Dispose(); $stream.Dispose() }
}
$binaries = @(Get-ChildItem -LiteralPath $packageRoot -File -Recurse | Where-Object Extension -in @('.exe','.dll','.node'))
$managed = @($binaries | Where-Object { (Read-NativeHeader $_.FullName).ClrHeader -ne 0 })
if ($managed.Count) { throw 'Se encontraron ensamblados administrados en la distribución.' }
foreach ($ownName in @('NotDiscord.exe','NotDiscord.Media.node')) {
    $ownHeader=Read-NativeHeader (Join-Path $packageRoot $ownName)
    if ($ownHeader.Machine -ne 0x8664) { throw 'El binario propio debe ser Windows x64.' }
}
$configuration=Get-Content -LiteralPath (Join-Path $packageRoot 'runtime.json') -Raw | ConvertFrom-Json
if ([IO.Path]::IsPathRooted($configuration.MediaHostExe) -or [IO.Path]::IsPathRooted($configuration.NativeRoot)) { throw 'El runtime debe usar rutas portables.' }
foreach ($required in @($configuration.MediaHostExe,(Join-Path $configuration.NativeRoot 'resources\standalone_modules\discord_voice\discord_voice.node'))) {
    if (!(Test-Path -LiteralPath (Join-Path $packageRoot $required))) { throw 'Falta un componente multimedia del paquete.' }
}
$oldHelper=Test-Path -LiteralPath (Join-Path $packageRoot 'media-runtime\resources\app.asar')
$privateData=@(Get-ChildItem -LiteralPath $packageRoot -File -Recurse | Where-Object { $_.Extension -eq '.dpapi' -or $_.Name -eq 'session.dpapi' })
if ($oldHelper -or $privateData.Count) { throw 'El paquete contiene código anterior o datos privados.' }
$report=[pscustomobject]@{
    Success=$true
    NativeBinariesChecked=$binaries.Count
    ManagedAssemblies=0
    OwnBinariesX64=$true
    RuntimePathsPortable=$true
    PreviousOwnedHelperIncluded=$false
    EncryptedUserDataIncluded=$false
    ExecutableSha256=(Get-FileHash -LiteralPath (Join-Path $packageRoot 'NotDiscord.exe') -Algorithm SHA256).Hash
    AddonSha256=(Get-FileHash -LiteralPath (Join-Path $packageRoot 'NotDiscord.Media.node') -Algorithm SHA256).Hash
    PerformanceMeasured=$false
}
$report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'evidence\release-package-checks.json')
$report | ConvertTo-Json
