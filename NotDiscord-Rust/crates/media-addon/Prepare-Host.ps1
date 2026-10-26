param([string]$Root=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,[string]$ReferenceRuntime='E:\DESCARGAS\REA\NotDiscord\terminal-native\media-host\runtime',[string]$ReferenceNativeRoot='E:\DESCARGAS\REA\NotDiscord\native-candidate\runtime')
$ErrorActionPreference='Stop'
$runtime=Join-Path $Root 'media-host\runtime'
$app=Join-Path $runtime 'resources\app'
New-Item -ItemType Directory -Path $runtime,$app -Force | Out-Null
# Only immutable third-party runtime files. The previous owned app.asar is excluded.
foreach($file in Get-ChildItem -LiteralPath $ReferenceRuntime -File){
  $target=Join-Path $runtime $file.Name
  if(!(Test-Path -LiteralPath $target) -or (Get-Item -LiteralPath $target).Length -ne $file.Length){Copy-Item -LiteralPath $file.FullName -Destination $target}
}
foreach($directory in Get-ChildItem -LiteralPath $ReferenceRuntime -Directory | Where-Object Name -ne 'resources'){
  Copy-Item -LiteralPath $directory.FullName -Destination $runtime -Recurse -Force
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'host\package.json') -Destination (Join-Path $app 'package.json') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'host\main.cjs') -Destination (Join-Path $app 'main.cjs') -Force
$nativeModule=Join-Path $runtime 'resources\standalone_modules\discord_voice'
New-Item -ItemType Directory -Path $nativeModule -Force | Out-Null
foreach($file in Get-ChildItem -LiteralPath (Join-Path $ReferenceNativeRoot 'resources\standalone_modules\discord_voice') -File){
  $target=Join-Path $nativeModule $file.Name
  if(!(Test-Path -LiteralPath $target) -or (Get-Item -LiteralPath $target).Length -ne $file.Length){Copy-Item -LiteralPath $file.FullName -Destination $target}
}
$binary=Join-Path $Root 'target\release\notdiscord_media_addon.dll'
if(Test-Path -LiteralPath $binary){
  $bin=Join-Path $Root 'bin'
  New-Item -ItemType Directory -Path $bin -Force | Out-Null
  Copy-Item -LiteralPath $binary -Destination (Join-Path $bin 'NotDiscord.Media.node') -Force
  Copy-Item -LiteralPath $binary -Destination (Join-Path $Root 'NotDiscord.Media.node') -Force
}
[pscustomobject]@{MediaHostExe='media-host/runtime/NotDiscord.Media.exe';NativeRoot='media-host/runtime'} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Root 'runtime.json') -Encoding UTF8
[pscustomobject]@{Runtime=$runtime;Loader=(Join-Path $app 'main.cjs');OldOwnedHelperIncluded=$false} | ConvertTo-Json -Compress
