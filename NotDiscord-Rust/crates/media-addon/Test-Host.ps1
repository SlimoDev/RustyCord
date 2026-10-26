param([string]$Root=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,[string]$NativeRoot='',[string]$AddonPath='')
$ErrorActionPreference='Stop'
if(!$NativeRoot){
  $NativeRoot=Join-Path $Root 'media-host\runtime'
  if(!(Test-Path -LiteralPath $NativeRoot)){$NativeRoot=Join-Path $Root 'media-runtime'}
}
if(!$AddonPath){$AddonPath=Join-Path $Root 'bin\NotDiscord.Media.node'}
$pipeName='NotDiscord.Media.'+[guid]::NewGuid().ToString('N')
$pipe=[System.IO.Pipes.NamedPipeServerStream]::new($pipeName,[System.IO.Pipes.PipeDirection]::InOut,1,[System.IO.Pipes.PipeTransmissionMode]::Byte,([System.IO.Pipes.PipeOptions]::Asynchronous -bor [System.IO.Pipes.PipeOptions]::CurrentUserOnly),16384,16384)
$process=[System.Diagnostics.Process]::new()
$info=[System.Diagnostics.ProcessStartInfo]::new((Join-Path $NativeRoot 'NotDiscord.Media.exe'))
$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.WorkingDirectory=$Root
$info.Environment.Remove('ELECTRON_RUN_AS_NODE') | Out-Null
$info.Environment.Remove('NODE_OPTIONS') | Out-Null
$info.Environment.Remove('NOTDISCORD_MEDIA_PROBE') | Out-Null
$info.Environment['NOTDISCORD_NATIVE_ROOT']=$NativeRoot
$info.Environment['NOTDISCORD_RUST_ADDON']=$AddonPath
$info.Environment['NOTDISCORD_MEDIA_PIPE']='\\.\pipe\'+$pipeName
$info.Environment['NOTDISCORD_MEDIA_DATA']=Join-Path $Root '.test-data\media'
$info.Environment['NOTDISCORD_MEDIA_TRACE']='1'
$process.StartInfo=$info
$stages=0;$checks=0;$results=@{};$reader=$null;$writer=$null
try{
  if(!$process.Start()){throw 'Host start failed'}
  $discardOut=$process.StandardOutput.ReadToEndAsync();$discardErr=$process.StandardError.ReadToEndAsync()
  $connected=$pipe.WaitForConnectionAsync()
  if(!$connected.Wait(12000)){throw 'Private pipe connect timed out'}
  $reader=[System.IO.StreamReader]::new($pipe,[System.Text.UTF8Encoding]::new($false,$true),$false,2048,$true)
  $writer=[System.IO.StreamWriter]::new($pipe,[System.Text.UTF8Encoding]::new($false),2048,$true);$writer.AutoFlush=$true
  function Read-Frame{
    $task=$reader.ReadLineAsync()
    if(!$task.Wait(6000)){throw 'Host response timed out'}
    if($null -eq $task.Result){throw 'Host pipe closed early'}
    return $task.Result | ConvertFrom-Json
  }
  $ready=Read-Frame
  if($ready.event -ne 'ready' -or $ready.nativeInitialized -ne $false -or $ready.backend -ne 'rust-napi-electron-main'){throw 'Unexpected ready frame'}
  $checks++
  foreach($command in @('capabilities','stats','shutdown')){
    $id='test-'+$command
    $writer.WriteLine((@{id=$id;command=$command;params=@{}} | ConvertTo-Json -Compress))
    do{
      $frame=Read-Frame
      if($frame.event -eq 'mediaStage'){$stages++}
    }while($frame.id -ne $id)
    if(!$frame.ok){throw 'Host command rejected'}
    $results[$command]=$frame.result
    $checks++
  }
  if(!$process.WaitForExit(5000)){throw 'Host did not shut down'}
  if($process.ExitCode -ne 0 -or $stages -ne 0){throw 'Unexpected exit or native initialization'}
  if($results.stats.voice -ne $null -or $results.stats.share -ne $null){throw 'Unexpected synthetic media session'}
  if($results.capabilities.dave -lt 1 -or !$results.capabilities.electronHost){throw 'Native addon compatibility missing'}
  $checks+=3
  $report=[pscustomobject]@{Success=$true;Checks=$checks;NativeInitializationStages=$stages;RealCalls=0;MicrophoneActivated=$false;CameraActivated=$false;ScreenCaptureStarted=$false;ExitCode=$process.ExitCode;Backend=$ready.backend;CSharpDependency=$false;OriginalOwnedHelperLoaded=$false}
  $report | Add-Member -NotePropertyName AddonSha256 -NotePropertyValue (Get-FileHash -LiteralPath $AddonPath -Algorithm SHA256).Hash
  $evidence=Join-Path $Root 'evidence'
  New-Item -ItemType Directory -Path $evidence -Force | Out-Null
  $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence 'media-host-checks.json') -Encoding UTF8
  $report | ConvertTo-Json -Compress
}catch{
  if(!$process.HasExited){$process.Kill($true);$process.WaitForExit(5000) | Out-Null}
  if($discardErr.IsCompleted){[Console]::Error.WriteLine($discardErr.Result)}
  throw
}finally{
  if($writer){$writer.Dispose()};if($reader){$reader.Dispose()};$pipe.Dispose()
  if($process.Id -and !$process.HasExited){$process.Kill($true);$process.WaitForExit(5000) | Out-Null}
  $process.Dispose()
}
