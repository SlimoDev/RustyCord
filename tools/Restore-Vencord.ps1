$ErrorActionPreference = 'Stop'
$ndRoot = Split-Path -Parent $PSScriptRoot
$ndArchive = Join-Path $ndRoot 'vendor\downloads\vencord-extension.zip'
$ndTarget = Join-Path $ndRoot 'vendor\vencord\current'
$ndExpectedHash = '37fc172d1ef1816e8640fb59f353543d8cb8ce7527172b4f5c288bbffc8a70ef'
if (Test-Path -LiteralPath $ndTarget) { throw 'La extensión ya existe. Este script no sobrescribe una instalación ni hace actualizaciones.' }
if (-not (Test-Path -LiteralPath $ndArchive)) { throw 'Falta el ZIP local verificado. Consulta evidence/vencord-web-provenance.json.' }
$ndActualHash = (Get-FileHash -LiteralPath $ndArchive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($ndActualHash -ne $ndExpectedHash) { throw 'El ZIP local no coincide con el paquete oficial verificado.' }
Expand-Archive -LiteralPath $ndArchive -DestinationPath $ndTarget
Write-Output "Extensión restaurada en $ndTarget."
