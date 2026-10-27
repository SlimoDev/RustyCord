param(
    [string]$Root=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,
    [string]$ReferenceRuntime='',
    [string]$ReferenceNativeRoot='',
    [string]$Destination=''
)
$ErrorActionPreference='Stop'
. (Join-Path $Root 'scripts\runtime-inventory.ps1')
if(!$ReferenceRuntime){$ReferenceRuntime=Join-Path $Root 'media-host\runtime'}
if(!$ReferenceNativeRoot){$ReferenceNativeRoot=$ReferenceRuntime}
if(!$Destination){$Destination=Join-Path $Root 'target\prepared-runtime'}
$sourceRuntime=(Resolve-Path -LiteralPath $ReferenceRuntime).Path
$sourceNative=(Resolve-Path -LiteralPath (Join-Path $ReferenceNativeRoot 'resources\standalone_modules\discord_voice')).Path
$runtime=[IO.Path]::GetFullPath($Destination).TrimEnd('\')
Assert-RuntimeNoLinks $runtime
Assert-RuntimeNoLinks $sourceNative
$null=Get-RuntimeTree $sourceRuntime -AllowReferenceDebugLog
if(Test-Path -LiteralPath $runtime){throw 'El staging de destino ya existe; usa una carpeta nueva para conservar el runtime anterior.'}
if($sourceRuntime.TrimEnd('\').Equals($runtime,[StringComparison]::OrdinalIgnoreCase) -or
    $runtime.StartsWith($sourceRuntime.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)){
    throw 'La preparación debe estar fuera del runtime de referencia.'
}
$nativeNames=@($script:RuntimeVendorFiles | Where-Object {$_ -like 'resources\standalone_modules\discord_voice\*'} | ForEach-Object {[IO.Path]::GetFileName($_)})
foreach($entry in Get-ChildItem -LiteralPath $sourceNative -Force){
    if($entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $nativeNames -cnotcontains $entry.Name){throw "Componente nativo inesperado: $($entry.Name)"}
}
New-Item -ItemType Directory -Path $runtime -Force|Out-Null
foreach($relative in @($script:RuntimeVendorFiles)+@($script:RuntimeProjectFiles)){
    $source=if($script:RuntimeProjectFiles -ccontains $relative){Join-Path $PSScriptRoot ('host\'+[IO.Path]::GetFileName($relative))}
        elseif($relative -like 'resources\standalone_modules\discord_voice\*'){Join-Path $sourceNative ([IO.Path]::GetFileName($relative))}
        else{Join-Path $sourceRuntime $relative}
    Assert-RuntimeNoLinks $source
    if(!(Test-Path -LiteralPath $source -PathType Leaf)){throw "Falta el componente de referencia: $relative"}
    $expected=(Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
    $target=Join-Path $runtime $relative
    New-Item -ItemType Directory -Path (Split-Path $target -Parent) -Force|Out-Null
    Copy-Item -LiteralPath $source -Destination $target
    if((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -cne $expected){throw "La copia no coincide con su referencia: $relative"}
}
# Written last: interrupted preparations have no valid inventory and cannot pack.
$inventory=Write-RuntimeInventory $runtime 'explicit-local-runtime-and-native-reference'
$null=Assert-RuntimeInventory $runtime
[pscustomobject]@{Runtime=$runtime;Inventory=(Join-Path $runtime $script:RuntimeInventoryName);Files=@($inventory.files).Count;HostFileVersion=$inventory.host_file_version;HostMachine=$inventory.host_machine;NativeMachine=$inventory.native_machine;ReferencePreserved=$true;Executed=$false}|ConvertTo-Json -Compress
