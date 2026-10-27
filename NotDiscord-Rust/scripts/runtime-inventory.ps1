# Static runtime inventory. Dot-source from preparation, packaging and validation.
# Never loads an executable/native module or reads application data.
$script:RuntimeInventoryName='runtime-inventory.json'
$script:RuntimeProjectFiles=@('resources\app\main.cjs','resources\app\package.json')
$script:RuntimeVendorFiles=@(
    'chrome_100_percent.pak','chrome_200_percent.pak','d3dcompiler_47.dll',
    'discord_wer.dll','dxcompiler.dll','dxil.dll','ffmpeg.dll','icudtl.dat',
    'libEGL.dll','libGLESv2.dll','NotDiscord.Media.exe','resources.pak',
    'snapshot_blob.bin','v8_context_snapshot.bin','vk_swiftshader_icd.json',
    'vk_swiftshader.dll','vulkan-1.dll','locales\en-US.pak','locales\es-419.pak',
    'locales\es.pak','resources\standalone_modules\discord_voice\audio_effects_helper.exe',
    'resources\standalone_modules\discord_voice\discord_voice.node',
    'resources\standalone_modules\discord_voice\gpu_encoder_helper.exe',
    'resources\standalone_modules\discord_voice\index.js',
    'resources\standalone_modules\discord_voice\manifest.json',
    'resources\standalone_modules\discord_voice\mediapipe.dll',
    'resources\standalone_modules\discord_voice\package.json',
    'resources\standalone_modules\discord_voice\selfie_segmentation_landscape.tflite',
    'resources\standalone_modules\discord_voice\selfie_segmentation.tflite'
)
function Assert-RuntimeNoLinks([string]$Path) {
    $current=[IO.Path]::GetFullPath($Path)
    while($current){
        if((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'El runtime contiene una ruta redirigida.'}
        $parent=[IO.Path]::GetDirectoryName($current)
        if($parent -eq $current){break};$current=$parent
    }
}
function Get-RuntimeTree([string]$Root,[switch]$AllowReferenceDebugLog) {
    $base=(Resolve-Path -LiteralPath $Root).Path.TrimEnd('\')
    Assert-RuntimeNoLinks $base
    $allowed=@($script:RuntimeVendorFiles)+@($script:RuntimeProjectFiles)+@($script:RuntimeInventoryName)
    if($AllowReferenceDebugLog){$allowed+=@('debug.log')}
    $files=New-Object 'System.Collections.Generic.List[object]'
    $pending=New-Object 'System.Collections.Generic.Stack[string]'
    $pending.Push($base)
    [long]$bytes=0
    $directories=0
    while($pending.Count){
        $directory=$pending.Pop();$directories++
        if($directories -gt 64){throw 'El runtime contiene demasiadas carpetas.'}
        foreach($entry in Get-ChildItem -LiteralPath $directory -Force){
            if($entry.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'El runtime no permite enlaces.'}
            $relative=[IO.Path]::GetRelativePath($base,$entry.FullName)
            if($entry.PSIsContainer){
                if(!($allowed | Where-Object { $_.StartsWith($relative+'\',[StringComparison]::OrdinalIgnoreCase) })){throw "Carpeta ajena al runtime permitido: $relative"}
                $pending.Push($entry.FullName)
            }else{
                if($allowed -cnotcontains $relative){throw "Archivo ajeno al runtime permitido: $relative"}
                # Known development diagnostic; it is never inventoried or copied.
                if($relative -ceq 'debug.log'){continue}
                $bytes+=$entry.Length
                if($bytes -gt 2GB -or $files.Count -ge 128){throw 'El runtime supera su presupuesto de inventario.'}
                $files.Add([pscustomobject]@{Relative=$relative;File=$entry})
            }
        }
    }
    return $files.ToArray()
}
function Get-RuntimePeMachine([string]$Path) {
    $reader=New-Object IO.BinaryReader([IO.File]::OpenRead($Path))
    try {
        if($reader.BaseStream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5a4d){throw 'Un componente no es un PE válido.'}
        $reader.BaseStream.Position=0x3c;$offset=$reader.ReadUInt32()
        if($offset -gt $reader.BaseStream.Length-6){throw 'Cabecera PE fuera del archivo.'}
        $reader.BaseStream.Position=$offset
        if($reader.ReadUInt32() -ne 0x4550){throw 'Cabecera PE no válida.'}
        return ('{0:x4}' -f $reader.ReadUInt16())
    } finally {$reader.Dispose()}
}
function Get-RuntimeStaticIdentity([string]$HostSha256,[string]$NativeSha256) {
    # Authored offline findings apply only to these exact artifact bytes. Never
    # infer Electron from Product/FileVersion or assign the host's classic ABI
    # to a module registered through Node-API. No artifact is loaded here.
    $electron=$null;$classicAbi=$null;$nativeApi=$null
    $hostEvidence=$null;$nativeEvidence=$null
    if($HostSha256 -ceq 'f92e752e394cfb28901e67321f7aa5205113643c11c0737c8e71b0c97adc671a') {
        $electron='42.11.11';$classicAbi=146
        $hostEvidence=[ordered]@{
            sha256=$HostSha256
            pe_resources_evidence_id='ev_7dbc2cc42371ea57598eba53f30dd735a2f29535221646f1fa8aa5bd05b48c40'
            electron_string_offsets=@(179639316,179639365,179551874)
            classic_module_abi_string_offset=189783748
        }
    }
    if($NativeSha256 -ceq '7c41c446a2ef29fa04d8f62a00193416cebebb562caa8a4d4626839729833607') {
        $nativeApi=6
        $nativeEvidence=[ordered]@{
            sha256=$NativeSha256
            pe_resources_evidence_id='ev_a9d25bc874dfc88a63fbf5a24c0eda9d8df5e5954c5c9c9bcc5972e02dc61326'
            api_version_export='node_api_module_get_api_version_v1'
            api_version_export_rva=112288
            api_version_export_offset=109216
            api_version_return_bytes='b806000000c3'
        }
    }
    $evidence=$null;$description=$null
    if($hostEvidence -or $nativeEvidence) {
        $description='Static hash-bound declarations; host classic ABI and native Node-API are distinct; runtime compatibility not verified'
        $evidence=[ordered]@{schema=1;kind='static-hash-bound';document='docs/runtime-static-0.5.0.md';host=$hostEvidence;native=$nativeEvidence}
    }
    [ordered]@{electron_version=$electron;node_module_abi=$classicAbi;native_node_api_version=$nativeApi;abi_evidence=$description;static_identity_evidence=$evidence}
}
function New-RuntimeInventory([string]$Root,[string]$Reference='explicit-local-reference') {
    $base=(Resolve-Path -LiteralPath $Root).Path.TrimEnd('\')
    $tree=@(Get-RuntimeTree $base)
    $required=@($script:RuntimeVendorFiles)+@($script:RuntimeProjectFiles)
    $records=foreach($relative in $required | Sort-Object){
        $entry=@($tree | Where-Object {$_.Relative -ceq $relative})
        if($entry.Count -ne 1){throw "Falta un componente permitido: $relative"}
        $file=$entry[0].File
        [ordered]@{path=$relative.Replace('\','/');bytes=$file.Length;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant();owner=$(if($script:RuntimeProjectFiles -ccontains $relative){'project'}else{'vendor'})}
    }
    $exe=Join-Path $base 'NotDiscord.Media.exe'
    $native=Join-Path $base 'resources\standalone_modules\discord_voice\discord_voice.node'
    $hostMachine=Get-RuntimePeMachine $exe;$nativeMachine=Get-RuntimePeMachine $native
    if($hostMachine -cne '8664' -or $nativeMachine -cne '8664'){throw 'El runtime debe ser Windows x64 en ambos motores.'}
    $hostHash=($records | Where-Object {$_.path -ceq 'NotDiscord.Media.exe'}).sha256
    $nativeHash=($records | Where-Object {$_.path -ceq 'resources/standalone_modules/discord_voice/discord_voice.node'}).sha256
    $identity=Get-RuntimeStaticIdentity -HostSha256 $hostHash -NativeSha256 $nativeHash
    [ordered]@{schema=1;reference=$Reference;host_file_version=(Get-Item -LiteralPath $exe).VersionInfo.FileVersion;host_machine=$hostMachine;native_machine=$nativeMachine;electron_version=$identity.electron_version;node_module_abi=$identity.node_module_abi;native_node_api_version=$identity.native_node_api_version;abi_evidence=$identity.abi_evidence;static_identity_evidence=$identity.static_identity_evidence;files=@($records)}
}
function Write-RuntimeInventory([string]$Root,[string]$Reference='explicit-local-reference') {
    $inventory=New-RuntimeInventory $Root $Reference
    $file=Join-Path $Root $script:RuntimeInventoryName
    Assert-RuntimeNoLinks $file
    [IO.File]::WriteAllText($file,($inventory|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
    return $inventory
}
function Assert-RuntimeInventory([string]$Root) {
    $file=Join-Path $Root $script:RuntimeInventoryName
    Assert-RuntimeNoLinks $file
    if(!(Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).Length -gt 128KB){throw 'Falta el inventario completo del runtime o supera su límite.'}
    $expected=Get-Content -LiteralPath $file -Raw -Encoding UTF8|ConvertFrom-Json
    if($expected.schema -ne 1 -or @($expected.files).Count -ne (@($script:RuntimeVendorFiles)+@($script:RuntimeProjectFiles)).Count){throw 'Inventario de runtime incompatible.'}
    $actual=New-RuntimeInventory $Root
    $seen=New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::Ordinal)
    foreach($record in @($expected.files)){
        if(!$seen.Add([string]$record.path)){throw 'El inventario contiene rutas duplicadas.'}
        $match=@($actual.files | Where-Object {$_.path -ceq $record.path})
        if($match.Count -ne 1 -or $record.bytes -ne $match[0].bytes -or $record.sha256 -cne $match[0].sha256 -or $record.owner -cne $match[0].owner){throw "Componente diferente del inventario: $($record.path)"}
    }
    if($expected.host_machine -cne $actual.host_machine -or $expected.native_machine -cne $actual.native_machine -or $expected.host_file_version -cne $actual.host_file_version){throw 'La identidad del motor difiere del inventario.'}
    foreach($field in @('electron_version','node_module_abi','native_node_api_version','abi_evidence','static_identity_evidence')) {
        if(!$expected.PSObject.Properties[$field]){throw "Falta un campo de identidad estática: $field"}
        $declared=ConvertTo-Json -InputObject ($expected.$field) -Depth 8 -Compress
        $computed=ConvertTo-Json -InputObject ($actual[$field]) -Depth 8 -Compress
        if($declared -cne $computed){throw "Declaración estática incompatible con los hashes del runtime: $field"}
    }
    return $expected
}
