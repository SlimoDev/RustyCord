param(
    [Parameter(Mandatory=$true)][string]$PackageRoot,
    [Parameter(Mandatory=$true)][string]$Tag,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)
# Creates first-party update assets only. It does not build, launch, sign in,
# upload, publish, or modify the installed application or its user data.
$ErrorActionPreference='Stop'
if ($Tag -notmatch '^[a-zA-Z0-9._+-]{1,100}$') { throw 'Etiqueta de versión no válida.' }
$package=(Resolve-Path -LiteralPath $PackageRoot).Path.TrimEnd('\')
$output=[IO.Path]::GetFullPath($OutputDirectory).TrimEnd('\')
if (Test-Path -LiteralPath $output) { throw 'El destino ya existe. Elige una carpeta nueva.' }
function Assert-NoLinks([string]$Path) {
    $current=[IO.Path]::GetFullPath($Path)
    while($current){
        if((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'La ruta contiene enlaces.'}
        $parent=[IO.Path]::GetDirectoryName($current)
        if($parent -eq $current){break}
        $current=$parent
    }
}
Assert-NoLinks $package
Assert-NoLinks $output
if ($output.StartsWith($package + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'La salida debe estar fuera del paquete de origen.' }
$version=Get-Content -LiteralPath (Join-Path $package 'version.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($version.schema -ne 1 -or !(($version.variant -ceq 'plus' -and $version.branch -ceq 'codex/notdiscord-plus') -or ($version.variant -ceq 'standard' -and $version.branch -ceq 'NotDiscord')) -or $version.version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$') { throw 'El paquete no identifica su versión y variante.' }
$ownNames=@('NotDiscord.exe','NotDiscord.Media.node','runtime.json','version.json','discord.ico','UNICODE-LICENSE.txt','SYMPHONIA-LICENSE-MPL-2.0.txt','media-runtime/resources/app/main.cjs','media-runtime/resources/app/package.json')
$ownNames += $(if ($version.variant -ceq 'plus') {'ABRIR_NOTDISCORD_PLUS.cmd'} else {'ABRIR_NOTDISCORD_RUST.cmd'})
$entries=@(Get-ChildItem -LiteralPath (Join-Path $package 'media-runtime') -Recurse -File -Force)
$files=[ordered]@{}
$runtime=[ordered]@{}
foreach($name in $ownNames){
    $file=Join-Path $package $name
    Assert-NoLinks $file
    if(!(Test-Path -LiteralPath $file -PathType Leaf)){throw "Falta el archivo del paquete: $name"}
    $files[$name]=[ordered]@{bytes=(Get-Item -LiteralPath $file).Length;sha256=(Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()}
}
foreach($entry in $entries){
    Assert-NoLinks $entry.FullName
    $name=$entry.FullName.Substring($package.Length+1).Replace('\','/')
    if($ownNames -contains $name){continue}
    if($name.StartsWith('media-runtime/resources/app/')){throw "Archivo de aplicación no declarado: $name"}
    $runtime[$name]=[ordered]@{bytes=$entry.Length;sha256=(Get-FileHash -LiteralPath $entry.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
}
foreach($name in @('media-runtime/NotDiscord.Media.exe','media-runtime/resources/standalone_modules/discord_voice/discord_voice.node')){if(!$runtime.Contains($name)){throw 'No se identificó el runtime multimedia.'}}
New-Item -ItemType Directory -Path $output | Out-Null
$stage=Join-Path $output 'payload'
New-Item -ItemType Directory -Path $stage | Out-Null
foreach($name in $ownNames){
    $destination=Join-Path $stage $name
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($destination)) -Force | Out-Null
    [IO.File]::Copy((Join-Path $package $name),$destination,$false)
}
$utf8=[Text.UTF8Encoding]::new($false)
$catalog=[ordered]@{schema=1;files=$files;runtime=$runtime}
[IO.File]::WriteAllText((Join-Path $stage 'package-files.json'),($catalog|ConvertTo-Json -Depth 8),$utf8)
$prefix=if($version.variant -ceq 'plus'){'NotDiscord-Plus'}else{'NotDiscord'}
$assetName="$prefix-$($version.version).zip"
$archive=Join-Path $output $assetName
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::CreateFromDirectory($stage,$archive,[IO.Compression.CompressionLevel]::Optimal,$false)
$manifest=[ordered]@{
    schema=1;variant=$version.variant;branch=$version.branch;version=$version.version
    release=[ordered]@{tag=$Tag;url="https://github.com/SlimoDev/RustyCord/releases/download/$Tag/$assetName";bytes=(Get-Item -LiteralPath $archive).Length;sha256=(Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()}
}
$manifestFile=Join-Path $output 'version.json'
[IO.File]::WriteAllText($manifestFile,($manifest|ConvertTo-Json -Depth 8),$utf8)
[pscustomobject]@{Package=$archive;Manifest=$manifestFile;Branch=$version.branch;ThirdPartyEnginesIncluded=$false;NextStep='Publicar el ZIP como asset de la etiqueta indicada y el manifiesto en la rama indicada. No se ha publicado nada.'}|ConvertTo-Json -Compress
