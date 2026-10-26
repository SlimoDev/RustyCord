param([string]$Root = (Join-Path $PSScriptRoot 'dist\NotDiscord-0.3.0'))
$ErrorActionPreference = 'Stop'
$packageRoot = (Resolve-Path -LiteralPath $Root).Path
$base = [IO.Path]::GetFullPath($packageRoot).TrimEnd('\')
function Package-Path([string]$Relative) {
    if([IO.Path]::IsPathRooted($Relative)){throw 'La ruta del paquete debe ser relativa.'}
    $path=[IO.Path]::GetFullPath((Join-Path $base $Relative))
    if(!$path.StartsWith($base+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Ruta fuera del paquete.'}
    if(!(Test-Path -LiteralPath $path -PathType Leaf)){throw "Falta $Relative"}
    if((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'El paquete no admite enlaces.'}
    $path
}
$exe=Package-Path 'NotDiscord.exe'
$addon=Package-Path 'NotDiscord.Media.node'
$version=Get-Content -LiteralPath (Package-Path 'version.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($version.schema -ne 1 -or !(($version.variant -ceq 'plus' -and $version.branch -ceq 'codex/notdiscord-plus') -or ($version.variant -ceq 'standard' -and $version.branch -ceq 'NotDiscord')) -or $version.version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$') {throw 'Versionamiento de paquete inválido.'}
foreach($asset in @('discord.ico','UNICODE-LICENSE.txt','SYMPHONIA-LICENSE-MPL-2.0.txt','media-runtime\resources\app\main.cjs','media-runtime\resources\app\package.json','media-runtime\resources\standalone_modules\discord_voice\discord_voice.node')){ $null=Package-Path $asset }
$config=Get-Content -LiteralPath (Package-Path 'runtime.json') -Raw | ConvertFrom-Json
$null=Package-Path $config.MediaHostExe
$native=[IO.Path]::GetFullPath((Join-Path $base $config.NativeRoot))
if(!$native.StartsWith($base+'\',[StringComparison]::OrdinalIgnoreCase) -or !(Test-Path -LiteralPath $native -PathType Container)){throw 'Runtime nativo fuera del paquete.'}
[pscustomobject]@{Package=$base;ExeSha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;AddonSha256=(Get-FileHash -LiteralPath $addon -Algorithm SHA256).Hash;StaticFilesValid=$true}|ConvertTo-Json -Compress
