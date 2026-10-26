# Creates only the two requested NotDiscord shortcuts. Does not launch the client.
# Uses the Shell's IPropertyStore so the shortcut AUMID matches the main process.
[CmdletBinding()]
param([switch]$VerifyOnly)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$candidateRoot = Join-Path $projectRoot 'native-candidate'
$target = Join-Path $candidateRoot 'runtime\NotDiscord.exe'
$desktop = [Environment]::GetFolderPath([Environment+SpecialFolder]::DesktopDirectory)
$appId = 'com.notdiscord.NativeCandidate'
$destinations = @(
    [pscustomobject]@{Label='project';Path=(Join-Path $projectRoot 'NotDiscord.lnk')},
    [pscustomobject]@{Label='desktop';Path=(Join-Path $desktop 'NotDiscord.lnk')}
)
if (-not [IO.File]::Exists($target)) { throw "Missing executable: $target" }
if (-not [IO.Directory]::Exists($desktop)) { throw "Desktop directory does not exist: $desktop" }
if (-not ('NotDiscord.Tools.Shortcut' -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot 'NotDiscord.Shortcut.cs')
}
function Assert-Shortcut([string]$Path) {
    $read = [NotDiscord.Tools.Shortcut]::Read($Path)
    if ($read.TargetPath -ine $target -or $read.Arguments -ne '' -or
        $read.WorkingDirectory -ine $candidateRoot -or $read.IconPath -ine $target -or
        $read.IconIndex -ne 0 -or $read.AppUserModelId -cne $appId) {
        throw "Shortcut verification failed: $Path"
    }
    return $read
}
if ($VerifyOnly) {
    $verified = @($destinations | ForEach-Object { Assert-Shortcut $_.Path })
    [pscustomobject]@{Verified=$true;ClientLaunched=$false;Shortcuts=$verified} | ConvertTo-Json -Depth 5
    return
}
# Validate ownership of any existing shortcut before writing either destination.
foreach ($destination in $destinations) {
    if ([IO.File]::Exists($destination.Path)) {
        if ((Get-Item -LiteralPath $destination.Path).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw "Refusing redirected shortcut: $($destination.Path)"
        }
        $old = [NotDiscord.Tools.Shortcut]::Read($destination.Path)
        $oldTarget = [IO.Path]::GetFullPath($old.TargetPath)
        if (-not $oldTarget.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Existing shortcut belongs to another target: $($destination.Path)"
        }
    }
}
$evidenceRoot = Join-Path $projectRoot 'evidence\native'
$tempRoot = Join-Path $projectRoot '.tmp\shortcut-helper'
$backupRoot = Join-Path $evidenceRoot 'shortcut-backups'
foreach ($directory in @($evidenceRoot,$tempRoot,$backupRoot)) {
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    if ((Get-Item -LiteralPath $directory).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Refusing redirected output directory: $directory"
    }
}
$stamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssfffZ')
$backups = @()
$created = @()
foreach ($destination in $destinations) {
    if ([IO.File]::Exists($destination.Path)) {
        $backup = Join-Path $backupRoot ($destination.Label + '-NotDiscord-' + $stamp + '.lnk')
        Copy-Item -LiteralPath $destination.Path -Destination $backup
        $backups += [pscustomobject]@{Original=$destination.Path;Backup=$backup;Sha256=(Get-FileHash -LiteralPath $backup -Algorithm SHA256).Hash.ToLowerInvariant()}
    }
    $staged = Join-Path $tempRoot ($destination.Label + '-' + [Guid]::NewGuid().ToString('N') + '.lnk')
    try {
        [NotDiscord.Tools.Shortcut]::Create($staged,$target,$candidateRoot,$target,$appId)
        Assert-Shortcut $staged | Out-Null
        Copy-Item -LiteralPath $staged -Destination $destination.Path -Force
        $created += Assert-Shortcut $destination.Path
    } finally {
        if ([IO.File]::Exists($staged)) { Remove-Item -LiteralPath $staged }
    }
}
$report = [pscustomobject]@{
    SchemaVersion=1;Utc=(Get-Date).ToUniversalTime().ToString('o')
    Verified=$true;ClientLaunched=$false;OriginalDiscordModified=$false
    DesktopResolved=$desktop;Backups=$backups;Shortcuts=$created
    TaskManagerVisualGroupingVerified=$false
}
$json = $report | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText((Join-Path $evidenceRoot 'shortcuts-identity.json'),$json+[Environment]::NewLine,[Text.UTF8Encoding]::new($false))
$json
