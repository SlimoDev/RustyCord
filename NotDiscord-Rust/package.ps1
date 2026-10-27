param(
    [string]$Destination = (Join-Path $PSScriptRoot 'target\installer-payload'),
    [Parameter(Mandatory = $true)][string]$RuntimeRoot
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scripts\runtime-inventory.ps1')
$packageRoot = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $packageRoot) { throw 'El destino ya existe. Elige una carpeta nueva para conservar las versiones anteriores.' }
$runtimeBase = (Resolve-Path -LiteralPath $RuntimeRoot).Path
$inventory=Assert-RuntimeInventory $runtimeBase
$runtimeEntries = @(Get-Item -LiteralPath $runtimeBase) + @(Get-ChildItem -LiteralPath $runtimeBase -Recurse -Force)
if ($runtimeEntries | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { throw 'El runtime debe contener archivos y carpetas locales, sin enlaces.' }
$exe = Join-Path $PSScriptRoot 'target\release\NotDiscord.exe'
$addon = Join-Path $PSScriptRoot 'target\release\notdiscord_media_addon.dll'
foreach ($required in @($exe, $addon, (Join-Path $runtimeBase 'NotDiscord.Media.exe'), (Join-Path $runtimeBase 'resources\standalone_modules\discord_voice\discord_voice.node'))) {
    if (!(Test-Path -LiteralPath $required -PathType Leaf)) { throw "Falta el componente: $required" }
}
function Copy-PlusFile([string]$Source, [string]$Relative) {
    $target = [IO.Path]::GetFullPath((Join-Path $packageRoot $Relative))
    if (!$target.StartsWith($packageRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Ruta de paquete fuera del destino.' }
    New-Item -ItemType Directory -Path (Split-Path $target -Parent) -Force | Out-Null
    Copy-Item -LiteralPath $Source -Destination $target
    if ((Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash) { throw "Copia diferente: $Relative" }
}
Copy-PlusFile $exe 'NotDiscord.exe'
Copy-PlusFile $addon 'NotDiscord.Media.node'
Copy-PlusFile (Join-Path $PSScriptRoot 'version.json') 'version.json'
foreach ($record in @($inventory.files)) {
    $relative = $record.path.Replace('/','\')
    Copy-PlusFile (Join-Path $runtimeBase $relative) (Join-Path 'media-runtime' $relative)
}
# The loader belongs to this source revision; third-party engines stay external.
Copy-PlusFile (Join-Path $PSScriptRoot 'crates\media-addon\host\main.cjs') 'media-runtime\resources\app\main.cjs'
Copy-PlusFile (Join-Path $PSScriptRoot 'crates\media-addon\host\package.json') 'media-runtime\resources\app\package.json'
$null=Write-RuntimeInventory (Join-Path $packageRoot 'media-runtime') ('prepared-reference-sha256:'+(Get-FileHash -LiteralPath (Join-Path $runtimeBase $script:RuntimeInventoryName) -Algorithm SHA256).Hash.ToLowerInvariant())
$null=Assert-RuntimeInventory (Join-Path $packageRoot 'media-runtime')
foreach ($asset in @('discord.ico','UNICODE-LICENSE.txt','SYMPHONIA-LICENSE-MPL-2.0.txt','WINIT-LICENSE-APACHE-2.0.txt')) { Copy-PlusFile (Join-Path $PSScriptRoot ('assets\' + $asset)) $asset }
$utf8 = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText((Join-Path $packageRoot 'runtime.json'), "{`r`n  `"MediaHostExe`": `"media-runtime/NotDiscord.Media.exe`",`r`n  `"NativeRoot`": `"media-runtime`"`r`n}`r`n", $utf8)
[IO.File]::WriteAllText((Join-Path $packageRoot 'ABRIR_NOTDISCORD_RUST.cmd'), "@echo off`r`nstart `"`" `"%~dp0NotDiscord.exe`" --runtime-root `"%~dp0.`" --data-dir `"%~dp0data`"`r`n", $utf8)
[pscustomobject]@{
    Package = $packageRoot
    Files = @(Get-ChildItem -LiteralPath $packageRoot -Recurse -File).Count
    ExeSha256 = (Get-FileHash -LiteralPath (Join-Path $packageRoot 'NotDiscord.exe') -Algorithm SHA256).Hash
    AddonSha256 = (Get-FileHash -LiteralPath (Join-Path $packageRoot 'NotDiscord.Media.node') -Algorithm SHA256).Hash
} | ConvertTo-Json -Compress
