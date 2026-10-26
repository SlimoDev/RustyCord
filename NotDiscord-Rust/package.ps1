param(
    [string]$Destination = (Join-Path $PSScriptRoot 'dist\NotDiscord-Plus-20261025'),
    [string]$RuntimeRoot = (Join-Path $PSScriptRoot 'media-host\runtime')
)
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'package-plus.ps1') -Destination $Destination -RuntimeRoot $RuntimeRoot
