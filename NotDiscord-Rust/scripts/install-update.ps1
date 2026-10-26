param(
    [Parameter(Mandatory=$true)][ValidateSet('Prepare','Install')][string]$Mode,
    [Parameter(Mandatory=$true)][string]$PlanPath
)
# Orchestrates only the verified product installer, never a portable ZIP.
$ErrorActionPreference='Stop'
$workValidated=$false
$admissionMutex=$null
$ownerMutex=$null
$admissionHeld=$false
$ownerHeld=$false
$cancelled=$false
$parentExited=$false
$installationCommitted=$false
$oldExecutableHash=$null
$oldRuntimeFingerprint=$null
$setupStarted=$false
function Assert-NoLinks([string]$Path) {
    $current=[IO.Path]::GetFullPath($Path)
    while($current) {
        if(Test-Path -LiteralPath $current) {
            if((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'La ruta contiene un enlace o punto de redireccionamiento.'}
        }
        $parent=[IO.Path]::GetDirectoryName($current)
        if($parent -eq $current){break}
        $current=$parent
    }
}
function Take-UpdateMutex([Threading.Mutex]$Mutex) {
    try{return $Mutex.WaitOne(60000)}catch [Threading.AbandonedMutexException]{return $true}
}
function Release-UpdateLocks {
    if($script:ownerHeld){$script:ownerMutex.ReleaseMutex();$script:ownerHeld=$false}
    if($script:ownerMutex){$script:ownerMutex.Dispose();$script:ownerMutex=$null}
    if($script:admissionHeld){$script:admissionMutex.ReleaseMutex();$script:admissionHeld=$false}
    if($script:admissionMutex){$script:admissionMutex.Dispose();$script:admissionMutex=$null}
}
function Verify-Installer {
    Assert-NoLinks $setup
    if([long]$plan.bytes -le 0 -or [long]$plan.bytes -gt 1GB -or $plan.sha256 -notmatch '^[a-fA-F0-9]{64}$' -or (Get-Item -LiteralPath $setup).Length -ne [long]$plan.bytes -or (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash -ine $plan.sha256){throw 'El instalador no coincide con el tamaño y SHA-256 publicados.'}
    if($plan.executable_sha256 -notmatch '^[a-fA-F0-9]{64}$'){throw 'El manifiesto no identifica el ejecutable que se instalará.'}
    $stream=[IO.File]::OpenRead($setup)
    try{if($stream.ReadByte() -ne 77 -or $stream.ReadByte() -ne 90){throw 'La descarga no contiene un instalador de Windows.'}}finally{$stream.Dispose()}
}
function Start-Client {
    $start=New-Object Diagnostics.ProcessStartInfo
    $start.FileName=[string]$plan.executable
    $start.Arguments=[string]$plan.arguments
    $start.WorkingDirectory=$target
    $start.UseShellExecute=$false
    [Diagnostics.Process]::Start($start)|Out-Null
}
function Runtime-Fingerprint {
    # Snapshot only installed program components, never user data or caches.
    $paths=New-Object 'System.Collections.Generic.List[string]'
    foreach($name in @('NotDiscord.exe','NotDiscord.Media.node','runtime.json','version.json','discord.ico')) {
        $file=Join-Path $target $name
        Assert-NoLinks $file
        if(!(Test-Path -LiteralPath $file -PathType Leaf)){throw 'Falta un componente de la instalación anterior.'}
        $paths.Add($file)
    }
    $directories=New-Object 'System.Collections.Generic.Stack[string]'
    $directories.Push((Join-Path $target 'media-runtime'))
    [long]$bytes=0
    while($directories.Count) {
        $directory=$directories.Pop()
        Assert-NoLinks $directory
        foreach($entry in Get-ChildItem -LiteralPath $directory -Force) {
            if($entry.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'El runtime contiene enlaces.'}
            if($entry.PSIsContainer){$directories.Push($entry.FullName)}else{$paths.Add($entry.FullName);$bytes+=$entry.Length}
            if($paths.Count -gt 10000 -or $bytes -gt 5GB){throw 'El runtime instalado supera los límites de verificación.'}
        }
    }
    $lines=foreach($path in $paths|Sort-Object) {
        $relative=$path.Substring($target.Length+1)
        $relative+'|'+(Get-Item -LiteralPath $path).Length+'|'+(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    }
    $sha=[Security.Cryptography.SHA256]::Create()
    try{return ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($lines -join [Environment]::NewLine))))).Replace('-','')}
    finally{$sha.Dispose()}
}
function Quote-Argument([string]$Value) {
    if($Value.Contains('"') -or $Value.Contains([char]13) -or $Value.Contains([char]10)){throw 'Argumento de instalador no válido.'}
    return '"'+$Value.TrimEnd('\')+'"'
}
function Need-Elevation {
    $probe=Join-Path $target ('.notdiscord-write-'+[IO.Path]::GetFileName($work))
    try {
        $file=[IO.File]::Open($probe,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
        $file.Dispose()
        Remove-Item -LiteralPath $probe -Force
        return $false
    }catch [UnauthorizedAccessException]{return $true}
}
function Install-Scope {
    # Preserve an existing uninstall registration at this exact install path.
    # A protected per-user install may elevate without becoming all-users.
    foreach($hive in @([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryHive]::LocalMachine)) {
        foreach($view in @([Microsoft.Win32.RegistryView]::Registry64,[Microsoft.Win32.RegistryView]::Registry32)) {
            $registry=$null;$key=$null
            try {
                $registry=[Microsoft.Win32.RegistryKey]::OpenBaseKey($hive,$view)
                $key=$registry.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\NotDiscord.Standard_is1')
                if($key) {
                    $location=[string]$key.GetValue('InstallLocation','')
                    if($location -and [IO.Path]::GetFullPath($location).TrimEnd('\') -ieq $target) {
                        if($hive -eq [Microsoft.Win32.RegistryHive]::LocalMachine){return '/ALLUSERS'}
                        return '/CURRENTUSER'
                    }
                }
            }catch{}finally{if($key){$key.Dispose()};if($registry){$registry.Dispose()}}
        }
    }
    return '/CURRENTUSER'
}
try {
    $planFile=[IO.Path]::GetFullPath($PlanPath)
    Assert-NoLinks $planFile
    if((Get-Item -LiteralPath $planFile).Length -gt 64KB){throw 'El plan de actualización es demasiado grande.'}
    $plan=Get-Content -LiteralPath $planFile -Raw -Encoding UTF8|ConvertFrom-Json
    if($plan.schema -ne 2 -or $plan.variant -cne 'standard' -or $plan.branch -cne 'NotDiscord'){throw 'El plan no corresponde al instalador de NotDiscord estándar.'}
    $target=[IO.Path]::GetFullPath([string]$plan.target_root).TrimEnd('\')
    $work=[IO.Path]::GetFullPath([string]$plan.work_root).TrimEnd('\')
    $updatesRoot=Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)) 'NotDiscord\updates'
    Assert-NoLinks $target
    Assert-NoLinks $work
    if($target -eq [IO.Path]::GetPathRoot($target).TrimEnd('\')){throw 'No se puede instalar NotDiscord en la raíz de una unidad.'}
    if($work -ine [IO.Path]::GetDirectoryName($planFile) -or [IO.Path]::GetDirectoryName($work) -ine $updatesRoot -or [IO.Path]::GetFileName($work) -notmatch '^[a-f0-9]{32}$'){throw 'La carpeta temporal no pertenece al actualizador de este usuario.'}
    $setup=[IO.Path]::GetFullPath([string]$plan.installer)
    if($setup -ine (Join-Path $work 'setup.exe') -or [IO.Path]::GetFullPath([string]$plan.executable) -ine (Join-Path $target 'NotDiscord.exe')){throw 'El plan identifica rutas de instalación incompatibles.'}
    if($plan.admission_mutex -cnotmatch '^Local\\NotDiscord\.Shared\.[a-f0-9]{24}\.Admission$' -or $plan.owner_mutex -cne $plan.admission_mutex.Replace('.Admission','.Owner')){throw 'El plan no identifica la coordinación de instancias.'}
    $workValidated=$true
    Verify-Installer
    if($Mode -eq 'Prepare') {
        [IO.File]::WriteAllText((Join-Path $work 'prepared'),'verified-windows-installer')
        exit 0
    }
    if(!(Test-Path -LiteralPath (Join-Path $work 'prepared') -PathType Leaf)){throw 'No existe una preparación validada.'}
    $admissionMutex=New-Object Threading.Mutex($false,[string]$plan.admission_mutex)
    $admissionHeld=Take-UpdateMutex $admissionMutex
    if(!$admissionHeld){throw 'Otra instancia está iniciándose. No se modificó la instalación.'}
    try {
        $parentProcess=[Diagnostics.Process]::GetProcessById([int]$plan.parent_pid)
        if($parentProcess.MainModule.FileName -ine [string]$plan.executable -or $parentProcess.HasExited){throw 'El proceso cambió antes de instalar.'}
    }catch [ArgumentException]{throw 'La aplicación que solicitó actualizar ya se cerró. No se modificó la instalación.'}
    [IO.File]::WriteAllText((Join-Path $work 'installer-ready'),'waiting-for-orderly-exit')
    $acknowledgement=[Diagnostics.Stopwatch]::StartNew()
    while(!(Test-Path -LiteralPath (Join-Path $work 'commit-install') -PathType Leaf)) {
        if(Test-Path -LiteralPath (Join-Path $work 'cancel-install') -PathType Leaf){$cancelled=$true;throw 'La aplicación canceló la entrega para conservar sus cambios.'}
        if($acknowledgement.Elapsed.TotalSeconds -gt 45){throw 'La aplicación no confirmó la entrega al instalador.'}
        Start-Sleep -Milliseconds 100
    }
    try {
        $closing=[Diagnostics.Stopwatch]::StartNew()
        while(!$parentProcess.WaitForExit(100)) {
            if(Test-Path -LiteralPath (Join-Path $work 'cancel-install') -PathType Leaf){$cancelled=$true;throw 'La aplicación canceló el cierre para conservar sus cambios.'}
            if($closing.Elapsed.TotalSeconds -gt 120){throw 'La aplicación anterior no terminó su cierre. No se modificó la instalación.'}
        }
    }finally{$parentProcess.Dispose()}
    $parentExited=$true
    if(Test-Path -LiteralPath (Join-Path $work 'cancel-install') -PathType Leaf){$cancelled=$true;throw 'La aplicación canceló la instalación.'}
    $ownerMutex=New-Object Threading.Mutex($false,[string]$plan.owner_mutex)
    $ownerHeld=Take-UpdateMutex $ownerMutex
    if(!$ownerHeld){throw 'La sesión anterior todavía no liberó sus recursos.'}
    $oldExecutableHash=(Get-FileHash -LiteralPath ([string]$plan.executable) -Algorithm SHA256).Hash
    $oldRuntimeFingerprint=Runtime-Fingerprint
    Verify-Installer
    $leaseFile=Join-Path $work 'installer-lease.ini'
    $lease=@('[NotDiscordUpdater]','Schema=1','Variant=standard',('InstallDir='+$target),('HelperPid='+$PID),('AdmissionMutex='+$plan.admission_mutex),('OwnerMutex='+$plan.owner_mutex))-join [Environment]::NewLine
    [IO.File]::WriteAllText($leaseFile,$lease,[Text.Encoding]::Unicode)
    $setupArguments='/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /NOTDISCORDUPDATER=1 /NDLEASE='+(Quote-Argument $leaseFile)+' /DIR='+(Quote-Argument $target)+' '+(Install-Scope)
    $start=New-Object Diagnostics.ProcessStartInfo
    $start.FileName=$setup
    $start.WorkingDirectory=$work
    $start.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
    if(Need-Elevation) {
        $start.UseShellExecute=$true
        $start.Verb='runas'
    }else{
        $start.UseShellExecute=$false
        $start.CreateNoWindow=$true
    }
    $start.Arguments=$setupArguments
    # Hold Admission across the transfer; Inno owns Owner during file changes
    # and rollback, protecting the transaction even if this helper crashes.
    $ownerMutex.ReleaseMutex();$ownerHeld=$false
    $setupProcess=[Diagnostics.Process]::Start($start)
    if(!$setupProcess){throw 'Windows no inició el instalador.'}
    $setupStarted=$true
    try{$setupProcess.WaitForExit();$setupExitCode=$setupProcess.ExitCode}finally{$setupProcess.Dispose()}
    $ownerHeld=Take-UpdateMutex $ownerMutex
    if(!$ownerHeld){throw 'El instalador no liberó la coordinación de archivos.'}
    if($setupExitCode -ne 0){throw "El instalador terminó con código $setupExitCode. Se conserva el registro para revisar el error."}
    Assert-NoLinks ([string]$plan.executable)
    if((Get-FileHash -LiteralPath ([string]$plan.executable) -Algorithm SHA256).Hash -ine $plan.executable_sha256){throw 'El ejecutable instalado no coincide con la versión publicada.'}
    $versionFile=Join-Path $target 'version.json'
    Assert-NoLinks $versionFile
    if((Get-Item -LiteralPath $versionFile).Length -gt 64KB){throw 'El manifiesto instalado es demasiado grande.'}
    $version=Get-Content -LiteralPath $versionFile -Raw -Encoding UTF8|ConvertFrom-Json
    if($version.schema -ne 2 -or $version.variant -cne $plan.variant -or $version.branch -cne $plan.branch -or $version.version -cne $plan.version){throw 'La versión instalada no coincide con la publicación.'}
    [IO.File]::WriteAllText((Join-Path $work 'installed'),[string]$plan.version)
    $installationCommitted=$true
    Release-UpdateLocks
    Start-Client
    exit 0
}catch{
    $message=$_.Exception.Message
    if($workValidated){try{[IO.File]::WriteAllText((Join-Path $work 'error.txt'),$message)}catch{}}
    $canReopenOld=$false
    if($parentExited -and !$installationCommitted -and $oldExecutableHash){
        try{
            if(!$ownerHeld -and $ownerMutex){$ownerHeld=Take-UpdateMutex $ownerMutex}
            if($ownerHeld){
                $unchangedExe=(Get-FileHash -LiteralPath ([string]$plan.executable) -Algorithm SHA256).Hash -ieq $oldExecutableHash
                $canReopenOld=$unchangedExe -and (!$setupStarted -or ($oldRuntimeFingerprint -and (Runtime-Fingerprint) -ceq $oldRuntimeFingerprint))
            }
        }catch{}
    }
    Release-UpdateLocks
    if($canReopenOld){try{Start-Client}catch{}}
    if($Mode -eq 'Install' -and !$cancelled){
        try{
            Add-Type -AssemblyName System.Windows.Forms
            $summary=if($installationCommitted){'La actualización se instaló, pero no se pudo volver a abrir la aplicación. Ábrela desde su acceso directo.'}else{'No se completó la actualización. Revisa el error antes de volver a intentarlo.'}
            [Windows.Forms.MessageBox]::Show(($summary+[Environment]::NewLine+$message),'Actualización de NotDiscord')|Out-Null
        }catch{}
    }
    exit 1
}finally{Release-UpdateLocks}
