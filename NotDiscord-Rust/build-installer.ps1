param(
    [Parameter(Mandatory=$true)][string]$PackageRoot,
    [string]$Compiler = (Join-Path (Split-Path $PSScriptRoot -Parent) '.tools\inno-setup\ISCC.exe'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'dist'),
    [string]$BuildDirectory = (Join-Path (Split-Path $PSScriptRoot -Parent) '.tools\temp\current\installer-build')
)
# Compile only. Never runs Setup, the application, an account or a device.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'scripts\runtime-inventory.ps1')
function Assert-NoLinks([string]$Path) {
    $current=[IO.Path]::GetFullPath($Path)
    while($current){
        if((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'La ruta contiene un enlace o redirección.'}
        $parent=[IO.Path]::GetDirectoryName($current)
        if($parent -eq $current){break};$current=$parent
    }
}
$package=(Resolve-Path -LiteralPath $PackageRoot).Path.TrimEnd('\')
$null=Assert-RuntimeInventory (Join-Path $package 'media-runtime')
$compilerPath=(Resolve-Path -LiteralPath $Compiler).Path
$output=[IO.Path]::GetFullPath($OutputDirectory).TrimEnd('\')
$build=[IO.Path]::GetFullPath($BuildDirectory).TrimEnd('\')
foreach($path in @($package,$compilerPath,$output,$build)){Assert-NoLinks $path}
if($output.StartsWith($package+'\',[StringComparison]::OrdinalIgnoreCase) -or $build.StartsWith($package+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'La salida y los temporales deben estar fuera del paquete de entrada.'}
$manifest=Get-Content -LiteralPath (Join-Path $package 'version.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if($manifest.variant -cne 'standard' -or $manifest.branch -cne 'NotDiscord' -or $manifest.version -notmatch '^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$'){throw 'Sólo se compila el instalador de una versión estándar válida.'}
$numeric="$($Matches[1]).$($Matches[2]).$($Matches[3]).0"
$version=[string]$manifest.version
$installer=Join-Path $output "NotDiscord-Setup-$version.exe"
if(Test-Path -LiteralPath $installer){throw 'El instalador de destino ya existe. No se sobrescribe una entrega anterior.'}
$required=@('NotDiscord.exe','NotDiscord.Media.node','runtime.json','version.json','discord.ico','UNICODE-LICENSE.txt','SYMPHONIA-LICENSE-MPL-2.0.txt','media-runtime\NotDiscord.Media.exe','media-runtime\resources\app\main.cjs','media-runtime\resources\app\package.json','media-runtime\resources\standalone_modules\discord_voice\discord_voice.node')
foreach($relative in $required){if(!(Test-Path -LiteralPath (Join-Path $package $relative) -PathType Leaf)){throw "Falta el componente: $relative"}}
$allowedRoot=@('NotDiscord.exe','NotDiscord.Media.node','runtime.json','version.json','discord.ico','UNICODE-LICENSE.txt','SYMPHONIA-LICENSE-MPL-2.0.txt','ABRIR_NOTDISCORD_RUST.cmd')
$entries=@(Get-ChildItem -LiteralPath $package -Recurse -File -Force)
$included=@()
foreach($entry in $entries){
    Assert-NoLinks $entry.FullName
    $relative=[IO.Path]::GetRelativePath($package,$entry.FullName)
    if($relative -notlike 'media-runtime\*' -and $allowedRoot -notcontains $relative){throw "Archivo ajeno al payload del instalador: $relative"}
    if($entry.Extension -in @('.log','.tmp','.dmp','.cmd')){continue}
    if($relative -match '(^|\\)(data|data-plus|cache|logs|login-webview|login-handoff)(\\|$)'){throw 'El payload contiene datos o cachés ajenos a la instalación.'}
    if($entry.FullName.IndexOfAny([char[]]@('"',';',[char]13,[char]10)) -ge 0){throw 'Una ruta no puede representarse de forma segura en Inno Setup.'}
    $included += $entry
}
New-Item -ItemType Directory -Path $output,$build -Force | Out-Null
$fileList=Join-Path $build ('payload-'+[Guid]::NewGuid().ToString('N')+'.iss')
$lines=foreach($entry in $included){
    $relative=[IO.Path]::GetRelativePath($package,$entry.FullName)
    $folder=[IO.Path]::GetDirectoryName($relative)
    $destination=if($folder){'{app}\'+$folder}else{'{app}'}
    'Source: "'+$entry.FullName+'"; DestDir: "'+$destination+'"; Flags: ignoreversion; BeforeInstall: CheckInstallerLease'
}
[IO.File]::WriteAllLines($fileList,$lines,[Text.UTF8Encoding]::new($true))
$arguments=@('/Qp',"/DAppVersion=$version","/DNumericVersion=$numeric","/DPackageFiles=$fileList","/DSourceRoot=$PSScriptRoot","/O$output","/FNotDiscord-Setup-$version",(Join-Path $PSScriptRoot 'installer\NotDiscord.iss'))
& $compilerPath @arguments
if($LASTEXITCODE -ne 0){throw "Inno Setup terminó con código $LASTEXITCODE. No se ha ejecutado el instalador."}
if(!(Test-Path -LiteralPath $installer -PathType Leaf)){throw 'El compilador no generó el instalador esperado.'}
[pscustomobject]@{Installer=$installer;Bytes=(Get-Item -LiteralPath $installer).Length;Sha256=(Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant();ExecutableSha256=(Get-FileHash -LiteralPath (Join-Path $package 'NotDiscord.exe') -Algorithm SHA256).Hash.ToLowerInvariant();Variant='standard';Version=$version;InstallerExecuted=$false;PortableDistribution=$false}|ConvertTo-Json -Compress
