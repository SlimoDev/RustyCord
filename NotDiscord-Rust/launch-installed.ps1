# Launch the registered standard installation. This source launcher is not a
# portable distribution and never searches or opens a Discord account file.
$ErrorActionPreference = 'Stop'

function Get-StandardInstallation([string]$Location) {
    if ([string]::IsNullOrWhiteSpace($Location)) { return $null }
    try {
        $path = $Location.Trim().Trim('"')
        if (![IO.Path]::IsPathRooted($path)) { return $null }
        $path = [IO.Path]::GetFullPath($path)
        if (!(Test-Path -LiteralPath $path -PathType Container)) { return $null }
        $path = (Resolve-Path -LiteralPath $path).ProviderPath
        if (!(Test-Path -LiteralPath (Join-Path $path 'NotDiscord.exe') -PathType Leaf)) { return $null }
        $standard = $false
        $ini = Join-Path $path 'installation.ini'
        if (Test-Path -LiteralPath $ini -PathType Leaf) {
            if ((Get-Item -LiteralPath $ini).Length -gt 16KB) { return $null }
            $section = ''
            foreach ($line in [IO.File]::ReadAllLines($ini)) {
                $line = $line.Trim()
                if ($line -match '^\[([^\]]+)\]$') { $section = $Matches[1]; continue }
                if ($section -ieq 'NotDiscord' -and $line -match '^Variant\s*=\s*(.*?)\s*$') {
                    if ($Matches[1] -cne 'standard') { return $null }
                    $standard = $true
                }
            }
        }
        $manifest = Join-Path $path 'version.json'
        if (Test-Path -LiteralPath $manifest -PathType Leaf) {
            if ((Get-Item -LiteralPath $manifest).Length -gt 64KB) { return $null }
            $version = [IO.File]::ReadAllText($manifest) | ConvertFrom-Json
            if ($version.variant -cne 'standard') { return $null }
            $standard = $true
        }
        if ($standard) { return $path }
    } catch {
        # A stale/unreadable registration must not launch an unverified edition.
    }
    return $null
}

function Find-StandardInstallation {
    foreach ($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryHive]::LocalMachine)) {
        $base = $null
        $key = $null
        try {
            $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey($hive, [Microsoft.Win32.RegistryView]::Registry64)
            $key = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\NotDiscord.Standard_is1', $false)
            if ($key) {
                $root = Get-StandardInstallation ([string]$key.GetValue('InstallLocation', ''))
                if ($root) { return $root }
            }
        } catch {
            # Continue from a removed per-user registration to the machine one.
        } finally {
            if ($key) { $key.Dispose() }
            if ($base) { $base.Dispose() }
        }
    }
    return $null
}

function Quote-PathArgument([string]$Path) {
    if ($Path.Contains('"')) { throw 'La ruta de la instalación no es válida.' }
    # Windows doubles a trailing backslash before the closing argument quote.
    return '"' + [regex]::Replace($Path, '(\\+)$', '$1$1') + '"'
}

try {
    $installation = Find-StandardInstallation
    if ($installation) {
        $arguments = '--runtime-root ' + (Quote-PathArgument $installation)
        $legacySource = Join-Path $PSScriptRoot 'data'
        # The app treats this only as a migration source. Its canonical shared
        # configuration/session location is resolved by normal desktop startup.
        if (Test-Path -LiteralPath $legacySource -PathType Container) {
            $arguments += ' --data-dir ' + (Quote-PathArgument $legacySource)
        }
        $start = New-Object Diagnostics.ProcessStartInfo
        $start.FileName = Join-Path $installation 'NotDiscord.exe'
        $start.Arguments = $arguments
        $start.WorkingDirectory = $installation
        $start.UseShellExecute = $false
        [Diagnostics.Process]::Start($start) | Out-Null
    } else {
        $setup = Join-Path $PSScriptRoot 'dist\NotDiscord-Setup-0.3.2.exe'
        if (!(Test-Path -LiteralPath $setup -PathType Leaf)) {
            throw 'No se encontró una instalación de NotDiscord. Ejecuta NotDiscord-Setup-0.3.2.exe para elegir su carpeta de instalación.'
        }
        # This is the interactive installer: the user chooses its destination.
        Start-Process -FilePath $setup | Out-Null
    }
} catch {
    Add-Type -AssemblyName System.Windows.Forms
    [Windows.Forms.MessageBox]::Show($_.Exception.Message, 'NotDiscord',
        [Windows.Forms.MessageBoxButtons]::OK, [Windows.Forms.MessageBoxIcon]::Error) | Out-Null
    exit 1
}
