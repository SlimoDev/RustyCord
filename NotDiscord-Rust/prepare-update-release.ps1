param(
    [Parameter(Mandatory=$true)][string]$Installer,
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$Destination
)
# Reads compiled files and writes only release metadata. No executable is run,
# no ZIP/portable distribution is created, and no account/network is used.
$ErrorActionPreference='Stop'
function Assert-NoLinks([string]$Path) {
    $current=[IO.Path]::GetFullPath($Path)
    while($current){
        if((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'La ruta contiene enlaces o redirecciones.'}
        $parent=[IO.Path]::GetDirectoryName($current)
        if($parent -eq $current){break};$current=$parent
    }
}
$setup=(Resolve-Path -LiteralPath $Installer).Path
$exe=(Resolve-Path -LiteralPath $Executable).Path
$destinationFile=[IO.Path]::GetFullPath($Destination)
foreach($path in @($setup,$exe,$destinationFile)){Assert-NoLinks $path}
if(Test-Path -LiteralPath $destinationFile){throw 'El manifiesto de destino ya existe. No se sobrescribe una publicación.'}
$version=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'version.json') -Raw -Encoding UTF8|ConvertFrom-Json
if($version.schema -ne 2 -or $version.variant -cne 'standard' -or $version.branch -cne 'NotDiscord' -or $version.version -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$'){throw 'Sólo se publica el instalador de una versión estándar válida con esquema 2.'}
$assetName="NotDiscord-Setup-$($version.version).exe"
if([IO.Path]::GetFileName($setup) -cne $assetName){throw "El archivo debe llamarse $assetName."}
$bytes=(Get-Item -LiteralPath $setup).Length
if($bytes -le 0 -or $bytes -gt 1GB){throw 'El instalador supera el tamaño admitido.'}
foreach($path in @($setup,$exe)){
    $stream=[IO.File]::OpenRead($path)
    try{if($stream.ReadByte() -ne 77 -or $stream.ReadByte() -ne 90){throw 'Un archivo no es un ejecutable de Windows.'}}finally{$stream.Dispose()}
}
$tag="NotDiscord-v$($version.version)"
$manifest=[ordered]@{
    schema=2;variant='standard';branch='NotDiscord';version=$version.version
    release=[ordered]@{
        kind='installer';tag=$tag
        url="https://github.com/SlimoDev/RustyCord/releases/download/$tag/$assetName"
        bytes=$bytes
        sha256=(Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
        executable_sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
$parent=[IO.Path]::GetDirectoryName($destinationFile)
New-Item -ItemType Directory -Path $parent -Force|Out-Null
Assert-NoLinks $parent
[IO.File]::WriteAllText($destinationFile,($manifest|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
[pscustomobject]@{Installer=$setup;Manifest=$destinationFile;Tag=$tag;Branch='NotDiscord';Schema=2;InstallerExecuted=$false;PortableCreated=$false;Published=$false}|ConvertTo-Json -Compress