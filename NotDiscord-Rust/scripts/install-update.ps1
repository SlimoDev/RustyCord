param(
    [Parameter(Mandatory=$true)][ValidateSet('Prepare','Install')][string]$Mode,
    [Parameter(Mandatory=$true)][string]$PlanPath
)
$ErrorActionPreference = 'Stop'
$plan = $null
$rollback = @()
$workValidated = $false
$admissionMutex = $null
$ownerMutex = $null
$admissionHeld = $false
$ownerHeld = $false
$installationCommitted = $false
$cancelled = $false
function Take-UpdateMutex([Threading.Mutex]$Mutex) {
    try { return $Mutex.WaitOne(60000) }
    catch [Threading.AbandonedMutexException] { return $true }
}
function Release-UpdateLocks {
    if ($script:ownerHeld) { $script:ownerMutex.ReleaseMutex(); $script:ownerHeld=$false }
    if ($script:ownerMutex) { $script:ownerMutex.Dispose(); $script:ownerMutex=$null }
    if ($script:admissionHeld) { $script:admissionMutex.ReleaseMutex(); $script:admissionHeld=$false }
    if ($script:admissionMutex) { $script:admissionMutex.Dispose(); $script:admissionMutex=$null }
}
function Assert-NoLinks([string]$Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'La ruta contiene un enlace o punto de redireccionamiento.' }
        }
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -eq $current) { break }
        $current = $parent
    }
}
function Inside([string]$Root, [string]$Relative) {
    if ([IO.Path]::IsPathRooted($Relative) -or $Relative.Contains(':')) { throw 'Ruta absoluta no permitida en el paquete.' }
    $pieces = $Relative.Replace('\','/').Split('/')
    foreach ($piece in $pieces) {
        if (!$piece -or $piece -eq '.' -or $piece -eq '..' -or $piece.EndsWith('.') -or $piece.EndsWith(' ') -or $piece -match '[<>:"|?*\x00-\x1F]') { throw 'Nombre de archivo no válido en el paquete.' }
        if ($piece -match '^(?i:CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])(?:\.|$)') { throw 'Nombre reservado de Windows en el paquete.' }
    }
    $full = [IO.Path]::GetFullPath((Join-Path $Root $Relative))
    if (!$full.StartsWith($Root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Ruta fuera de la carpeta autorizada.' }
    Assert-NoLinks $full
    return $full
}
function Allowed([string]$Name) {
    return $Name -match '^(NotDiscord\.exe|NotDiscord\.Media\.node|runtime\.json|version\.json|package-files\.json|discord\.ico|UNICODE-LICENSE\.txt|SYMPHONIA-LICENSE-MPL-2\.0\.txt|ABRIR_NOTDISCORD_(PLUS|RUST)\.cmd|media-runtime/resources/app/(main\.cjs|package\.json))$'
}
function Verify-Payload {
    $version = Get-Content -LiteralPath (Inside $payload 'version.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($version.schema -ne 1 -or $version.variant -cne $plan.variant -or $version.branch -cne $plan.branch -or $version.version -cne $plan.version) { throw 'La versión del paquete no coincide con la publicación.' }
    $catalogFile = Inside $payload 'package-files.json'
    if ((Get-Item -LiteralPath $catalogFile).Length -gt 2MB) { throw 'El índice de archivos del paquete es demasiado grande.' }
    $catalog = Get-Content -LiteralPath $catalogFile -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($catalog.schema -ne 1 -or !$catalog.files -or @($catalog.files.PSObject.Properties).Count -gt 10000 -or !$catalog.runtime) { throw 'El índice de archivos del paquete no es válido.' }
    $names = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in $catalog.files.PSObject.Properties) {
        $name = $entry.Name.Replace('\','/')
        if (!(Allowed $name) -or $name -eq 'package-files.json' -or !$names.Add($name)) { throw 'Archivo duplicado o no permitido en el índice.' }
        $file = Inside $payload $name
        if (!(Test-Path -LiteralPath $file -PathType Leaf)) { throw "Falta un archivo del paquete: $name" }
        if ($entry.Value.sha256 -notmatch '^[0-9a-fA-F]{64}$' -or [long]$entry.Value.bytes -lt 0 -or (Get-Item -LiteralPath $file).Length -ne [long]$entry.Value.bytes -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ine $entry.Value.sha256) { throw "El archivo no coincide con su SHA-256: $name" }
    }
    foreach ($file in Get-ChildItem -LiteralPath $payload -Recurse -File -Force) {
        $name = $file.FullName.Substring($payload.Length + 1).Replace('\','/')
        Assert-NoLinks $file.FullName
        if ($name -ne 'package-files.json' -and !$names.Contains($name)) { throw 'El paquete contiene archivos que no figuran en el índice.' }
    }
    foreach ($name in @('NotDiscord.exe','NotDiscord.Media.node','version.json','runtime.json','media-runtime/resources/app/main.cjs','media-runtime/resources/app/package.json')) {
        if (!$names.Contains($name)) { throw "Falta un componente imprescindible: $name" }
    }
    $runtime = Get-Content -LiteralPath (Inside $payload 'runtime.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($runtime.MediaHostExe.Replace('\','/') -cne 'media-runtime/NotDiscord.Media.exe' -or $runtime.NativeRoot.Replace('\','/') -cne 'media-runtime') { throw 'El paquete no utiliza la estructura de runtime admitida.' }
    # Third-party engines are not distributed in application updates. Reuse only
    # the compatible installed files declared by the publisher; never download
    # or replace an engine as a side effect of this updater.
    $runtimeNames = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
    if (@($catalog.runtime.PSObject.Properties).Count -gt 10000) { throw 'El índice del runtime es demasiado grande.' }
    foreach ($entry in $catalog.runtime.PSObject.Properties) {
        $name = $entry.Name.Replace('\','/')
        if (!$name.StartsWith('media-runtime/') -or $name.StartsWith('media-runtime/resources/app/') -or !$runtimeNames.Add($name)) { throw 'Ruta de runtime incompatible en el paquete.' }
        $file = Inside $target $name
        if (!(Test-Path -LiteralPath $file -PathType Leaf) -or $entry.Value.sha256 -notmatch '^[0-9a-fA-F]{64}$' -or [long]$entry.Value.bytes -lt 0 -or (Get-Item -LiteralPath $file).Length -ne [long]$entry.Value.bytes -or (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ine $entry.Value.sha256) { throw "El motor multimedia instalado no es compatible con esta actualización: $name. Instala un paquete completo compatible." }
    }
    foreach ($name in @('media-runtime/NotDiscord.Media.exe','media-runtime/resources/standalone_modules/discord_voice/discord_voice.node')) {
        if (!$runtimeNames.Contains($name)) { throw 'El índice no identifica el motor multimedia compatible.' }
    }
    return $catalog
}
function Start-Client {
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = [string]$plan.executable
    $start.Arguments = [string]$plan.arguments
    $start.WorkingDirectory = $target
    $start.UseShellExecute = $false
    [Diagnostics.Process]::Start($start) | Out-Null
}
try {
    $planFile = [IO.Path]::GetFullPath($PlanPath)
    Assert-NoLinks $planFile
    if ((Get-Item -LiteralPath $planFile).Length -gt 64KB) { throw 'El plan de actualización es demasiado grande.' }
    $plan = Get-Content -LiteralPath $planFile -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($plan.schema -ne 1 -or !(($plan.variant -ceq 'plus' -and $plan.branch -ceq 'codex/notdiscord-plus') -or ($plan.variant -ceq 'standard' -and $plan.branch -ceq 'NotDiscord'))) { throw 'El plan no corresponde a una variante de NotDiscord.' }
    $target = [IO.Path]::GetFullPath([string]$plan.target_root).TrimEnd('\')
    $work = [IO.Path]::GetFullPath([string]$plan.work_root).TrimEnd('\')
    Assert-NoLinks $target
    Assert-NoLinks $work
    if ($work -ine [IO.Path]::GetDirectoryName($planFile) -or [IO.Path]::GetDirectoryName($work) -ine (Join-Path $target '.notdiscord-update') -or [IO.Path]::GetFileName($work) -notmatch '^[a-f0-9]{32}$') { throw 'La carpeta temporal no pertenece a la instalación.' }
    if ([IO.Path]::GetFullPath([string]$plan.executable) -ine (Join-Path $target 'NotDiscord.exe') -or [IO.Path]::GetFullPath([string]$plan.archive) -ine (Join-Path $work 'package.zip')) { throw 'El ejecutable o el archivo de descarga no corresponde a esta instalación.' }
    $workValidated = $true
    $archive = [string]$plan.archive
    $payload = Join-Path $work 'payload'
    $backup = Join-Path $work 'backup'
    if ([long]$plan.bytes -le 0 -or [long]$plan.bytes -gt 1GB -or $plan.sha256 -notmatch '^[a-f0-9A-F]{64}$' -or (Get-Item -LiteralPath $archive).Length -ne [long]$plan.bytes -or (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ine $plan.sha256) { throw 'La descarga no coincide con el tamaño y SHA-256 publicados.' }
    if ($Mode -eq 'Prepare') {
        if (Test-Path -LiteralPath $payload) { throw 'La carpeta de preparación ya existe.' }
        New-Item -ItemType Directory -Path $payload | Out-Null
        Add-Type -AssemblyName System.IO.Compression
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $stream = [IO.File]::OpenRead($archive)
        $zip = New-Object IO.Compression.ZipArchive($stream, [IO.Compression.ZipArchiveMode]::Read, $false)
        try {
            if ($zip.Entries.Count -gt 10000) { throw 'El paquete contiene demasiadas entradas.' }
            $seen = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
            [long]$total = 0
            foreach ($entry in $zip.Entries) {
                $name = $entry.FullName.Replace('\','/')
                $isDirectory = $name.EndsWith('/')
                $name = $name.TrimEnd('/')
                if (!$name) { continue }
                $destination = Inside $payload $name
                if (!$seen.Add($name) -or (($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) { throw 'Enlace o entrada duplicada en el paquete.' }
                if ($isDirectory) {
                    if ($name -ne 'media-runtime' -and !$name.StartsWith('media-runtime/')) { throw 'Carpeta no permitida en el paquete.' }
                    New-Item -ItemType Directory -Path $destination -Force | Out-Null
                    continue
                }
                if (!(Allowed $name) -or $entry.Length -lt 0 -or $entry.Length -gt 1GB) { throw 'Archivo no permitido o demasiado grande en el paquete.' }
                $total += $entry.Length
                if ($total -gt 2GB) { throw 'El paquete supera el tamaño descomprimido permitido.' }
                New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($destination)) -Force | Out-Null
                $input = $entry.Open()
                $output = [IO.File]::Open($destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
                try { $input.CopyTo($output) } finally { $output.Dispose(); $input.Dispose() }
            }
        } finally { $zip.Dispose(); $stream.Dispose() }
        $null = Verify-Payload
        [IO.File]::WriteAllText((Join-Path $work 'prepared'), 'validated')
    } else {
        if (!(Test-Path -LiteralPath (Join-Path $work 'prepared') -PathType Leaf)) { throw 'No existe una preparación validada.' }
        $catalog = Verify-Payload
        if ($plan.admission_mutex -cnotmatch '^Local\\NotDiscord\.Shared\.[a-f0-9]{24}\.Admission$' -or $plan.owner_mutex -cne $plan.admission_mutex.Replace('.Admission','.Owner')) { throw 'El plan no identifica la coordinación compartida entre variantes.' }
        # Normal startup takes Admission before Owner. Holding Admission across
        # this handoff prevents another launch from opening data or this runtime
        # while files are being replaced or restored.
        $admissionMutex = New-Object Threading.Mutex($false, [string]$plan.admission_mutex)
        $admissionHeld = Take-UpdateMutex $admissionMutex
        if (!$admissionHeld) { throw 'Otra instancia está iniciándose. No se modificó la instalación.' }
        # Acquire the original process handle before acknowledging readiness. No
        # process is killed; installation waits for normal controller shutdown.
        $parentProcess = $null
        try {
            $parentProcess = [Diagnostics.Process]::GetProcessById([int]$plan.parent_pid)
            if ($parentProcess.MainModule.FileName -ine [string]$plan.executable) { throw 'El proceso cambió antes de instalar.' }
            if ($parentProcess.HasExited) { throw 'La aplicación que solicitó actualizar ya se cerró.' }
        } catch [ArgumentException] { throw 'La aplicación que solicitó actualizar ya se cerró. No se modificó la instalación.' }
        [IO.File]::WriteAllText((Join-Path $work 'installer-ready'), 'waiting-for-orderly-exit')
        $acknowledgement = [Diagnostics.Stopwatch]::StartNew()
        while (!(Test-Path -LiteralPath (Join-Path $work 'commit-install') -PathType Leaf)) {
            if (Test-Path -LiteralPath (Join-Path $work 'cancel-install') -PathType Leaf) { $cancelled=$true; throw 'La aplicación canceló la entrega al instalador para conservar sus cambios.' }
            if ($acknowledgement.Elapsed.TotalSeconds -gt 45) { throw 'La aplicación no confirmó la entrega al instalador. No se modificó la instalación.' }
            Start-Sleep -Milliseconds 100
        }
        if ($parentProcess) {
            try {
                $closing = [Diagnostics.Stopwatch]::StartNew()
                while (!$parentProcess.WaitForExit(100)) {
                    if (Test-Path -LiteralPath (Join-Path $work 'cancel-install') -PathType Leaf) { $cancelled=$true; throw 'La aplicación canceló el cierre para conservar sus cambios.' }
                    if ($closing.Elapsed.TotalSeconds -gt 120) { throw 'La aplicación anterior no terminó su cierre. No se modificó la instalación.' }
                }
            }
            finally { $parentProcess.Dispose() }
        }
        if (Test-Path -LiteralPath (Join-Path $work 'cancel-install') -PathType Leaf) { $cancelled=$true; throw 'La aplicación canceló la instalación.' }
        $ownerMutex = New-Object Threading.Mutex($false, [string]$plan.owner_mutex)
        $ownerHeld = Take-UpdateMutex $ownerMutex
        if (!$ownerHeld) { throw 'La sesión compartida no quedó disponible. No se modificó la instalación.' }
        New-Item -ItemType Directory -Path $backup | Out-Null
        foreach ($entry in $catalog.files.PSObject.Properties) {
            $name = $entry.Name.Replace('\','/')
            $source = Inside $payload $name
            $destination = Inside $target $name
            $saved = Inside $backup $name
            $existed = Test-Path -LiteralPath $destination -PathType Leaf
            if (Test-Path -LiteralPath $destination -PathType Container) { throw 'Una carpeta ocupa la ruta de un archivo de la actualización.' }
            if ($existed) {
                New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($saved)) -Force | Out-Null
                [IO.File]::Copy($destination, $saved, $false)
            }
            $rollback += [pscustomobject]@{ Destination=$destination; Saved=$saved; Existed=$existed }
            New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($destination)) -Force | Out-Null
            $temporary = $destination + '.update-' + [IO.Path]::GetFileName($work)
            [IO.File]::Copy($source, $temporary, $false)
            try {
                if ($existed) { [IO.File]::Replace($temporary, $destination, $null) }
                else { [IO.File]::Move($temporary, $destination) }
            } finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force } }
        }
        [IO.File]::WriteAllText((Join-Path $work 'installed'), [string]$plan.version)
        $installationCommitted = $true
        Release-UpdateLocks
        Start-Client
    }
    exit 0
} catch {
    $message = $_.Exception.Message
    for ($index=$(if ($installationCommitted) {-1} else {$rollback.Count-1}); $index -ge 0; $index--) {
        $entry=$rollback[$index]
        try {
            Assert-NoLinks $entry.Destination
            if ($entry.Existed) { [IO.File]::Copy($entry.Saved, $entry.Destination, $true) }
            elseif (Test-Path -LiteralPath $entry.Destination -PathType Leaf) { Remove-Item -LiteralPath $entry.Destination -Force }
        } catch { $message += "`r`nNo se pudo restaurar: $($entry.Destination)" }
    }
    Release-UpdateLocks
    if ($workValidated -and $plan -and $work -and (Test-Path -LiteralPath $work -PathType Container)) {
        try { [IO.File]::WriteAllText((Join-Path $work 'error.txt'), $message) } catch {}
    }
    if ($Mode -eq 'Install' -and !$cancelled) {
        try {
            Add-Type -AssemblyName System.Windows.Forms
            $summary = if ($installationCommitted) {'La actualización se instaló, pero no se pudo volver a abrir la aplicación. Ábrela desde su acceso directo.'} else {'No se completó la actualización. Se intentó restaurar la instalación anterior.'}
            [Windows.Forms.MessageBox]::Show("$summary`r`n$message", 'Actualización de NotDiscord') | Out-Null
        } catch {}
    }
    exit 1
} finally { Release-UpdateLocks }
