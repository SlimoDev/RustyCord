param([string]$Root=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path,[string]$AddonPath='')
$ErrorActionPreference='Stop'
if(!$AddonPath){$AddonPath=Join-Path $Root '.test-bin/NotDiscord.MediaProbe.node'}
$pipeName='NotDiscord.Media.'+[guid]::NewGuid().ToString('N')
$options=[System.IO.Pipes.PipeOptions]::Asynchronous -bor [System.IO.Pipes.PipeOptions]::CurrentUserOnly
$pipe=[System.IO.Pipes.NamedPipeServerStream]::new($pipeName,[System.IO.Pipes.PipeDirection]::InOut,1,[System.IO.Pipes.PipeTransmissionMode]::Byte,$options,16384,16384)
$video=[System.IO.Pipes.NamedPipeServerStream]::new($pipeName+'.Frames',[System.IO.Pipes.PipeDirection]::In,1,[System.IO.Pipes.PipeTransmissionMode]::Byte,$options,16384,16384)
$process=[System.Diagnostics.Process]::new()
$info=[System.Diagnostics.ProcessStartInfo]::new((Join-Path $Root 'media-host/runtime/NotDiscord.Media.exe'))
$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.WorkingDirectory=$Root
$info.Environment.Remove('ELECTRON_RUN_AS_NODE')|Out-Null;$info.Environment.Remove('NODE_OPTIONS')|Out-Null
$info.Environment['NOTDISCORD_NATIVE_ROOT']=Join-Path $Root 'media-host/runtime'
$info.Environment['NOTDISCORD_RUST_ADDON']=$AddonPath
$info.Environment['NOTDISCORD_MEDIA_PIPE']='\\.\pipe\'+$pipeName
$info.Environment['NOTDISCORD_VIDEO_PIPE']='\\.\pipe\'+$pipeName+'.Frames'
$info.Environment['NOTDISCORD_MEDIA_DATA']=Join-Path $Root '.test-data/owned-preview'
$info.Environment['NOTDISCORD_MEDIA_PROBE']='OWNED_SCREEN'
$info.Environment['NOTDISCORD_MEDIA_TRACE']='1'
$process.StartInfo=$info;$reader=$null;$writer=$null;$script:metadata=@{};$script:request=0;$script:previewErrors=@()
$report=[ordered]@{Success=$false;RealCalls=0;CameraActivated=$false;MicrophoneActivated=$false;FullDesktopCaptured=$false;OtherWindowsCaptured=$false;NativeCatalogVerified=$false;OwnWindowIdentityVerified=$false;NativeFrame=$false;RgbaBinaryFrame=$false;Cleanup=$false}
function Read-Control {
    $task=$reader.ReadLineAsync();if(!$task.Wait(8000)){throw 'Control response timed out'}
    if($null -eq $task.Result){throw 'Host closed'}
    $frame=$task.Result|ConvertFrom-Json
    if($frame.event -eq 'videoStream'){$script:metadata[$frame.id.ToString()]=$frame;if($frame.state -eq 'error'){$script:previewErrors+=,$frame.message}}
    return $frame
}
function Command($kind,$parameters) {
    $script:request++;$id='owned-'+$script:request
    $writer.WriteLine((@{id=$id;command=$kind;params=$parameters}|ConvertTo-Json -Compress -Depth 8))
    do{$result=Read-Control}while($result.id -ne $id)
    if(!$result.ok){throw ('Native command '+$kind+' rejected: '+$result.error.code)}
    return $result.result
}
function Read-Exact([int]$count) {
    $buffer=[byte[]]::new($count);$offset=0
    while($offset -lt $count){$task=$video.ReadAsync($buffer,$offset,$count-$offset);if(!$task.Wait(10000)){throw 'Binary frame timed out'};if($task.Result -eq 0){throw 'Binary pipe closed'};$offset+=$task.Result}
    return ,$buffer
}
try {
    if(!$process.Start()){throw 'Could not start isolated helper'}
    $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
    if(!$pipe.WaitForConnectionAsync().Wait(10000)-or !$video.WaitForConnectionAsync().Wait(10000)){throw 'Private pipes timed out'}
    $reader=[IO.StreamReader]::new($pipe,[Text.UTF8Encoding]::new($false,$true),$false,2048,$true)
    $writer=[IO.StreamWriter]::new($pipe,[Text.UTF8Encoding]::new($false),2048,$true);$writer.AutoFlush=$true
    $ready=Read-Control;if($ready.event -ne 'ready'){throw 'Unexpected ready'}
    $source=Command 'probeOwnedSource' @{}
    if(!$source.identityVerified -or $source.pid -ne $process.Id -or !$source.id.StartsWith('window:')){throw 'Owned window identity mismatch'}
    $report.OwnWindowIdentityVerified=$true;$report.SourceId=$source.id;$report.HostPid=$process.Id
    $start=Command 'startScreenPreview' @{sourceId=$source.id;userId='1';token=1}
    $header=Read-Exact 36
    if([Text.Encoding]::ASCII.GetString($header,0,4)-ne 'NDVF' -or [BitConverter]::ToUInt16($header,4)-ne 1){throw 'Invalid binary header'}
    $id=[BitConverter]::ToUInt64($header,8);$width=[BitConverter]::ToUInt32($header,24);$height=[BitConverter]::ToUInt32($header,28);$length=[BitConverter]::ToUInt32($header,32)
    if($width -eq 0 -or $height -eq 0 -or $width -gt 1280 -or $height -gt 720 -or $length -ne ([uint64]$width*$height*4)){throw 'Invalid dimensions'}
    if(!$script:metadata.ContainsKey($id.ToString()) -or $script:metadata[$id.ToString()].sourceId -ne $source.id -or !$script:metadata[$id.ToString()].preview){throw 'Frame does not match selected source'}
    $pixels=Read-Exact $length;$samples=@()
    foreach($point in @(@(0.25,0.25),@(0.75,0.25),@(0.25,0.75),@(0.75,0.75))){$index=4*(([int][Math]::Floor($height*$point[1]))*$width+[int][Math]::Floor($width*$point[0]));$samples+=,@([int]$pixels[$index],[int]$pixels[$index+1],[int]$pixels[$index+2],[int]$pixels[$index+3])}
    if(@($samples|Where-Object{$_[3] -ne 255}).Count -ne 0){throw 'Opaque native preview became transparent in RGBA'}
    if(!($samples[0][0]-gt 180 -and $samples[0][1]-lt 90 -and $samples[0][2]-lt 90 -and $samples[1][1]-gt 180 -and $samples[1][0]-lt 90 -and $samples[1][2]-lt 90 -and $samples[2][2]-gt 180 -and $samples[2][0]-lt 90 -and $samples[2][1]-lt 90 -and $samples[3][0]-gt 180 -and $samples[3][1]-gt 180 -and $samples[3][2]-gt 180)){throw 'Synthetic quadrants not found in native preview'}
    $report.NativeFrame=$true;$report.RgbaBinaryFrame=$true;$report.Width=$width;$report.Height=$height;$report.SampleRgba=$samples
    $stop=Command 'stopMediaPreview' @{token=2}
    $cleanup=Command 'probeOwnedCleanup' @{}
    if(!$cleanup.windowDestroyed -or $cleanup.videos.sources -ne 0 -or $cleanup.videos.sinks -ne 0 -or $cleanup.videos.active -ne 0 -or $cleanup.voiceSessions -ne 0 -or !$cleanup.reentrantDetachSafe){throw 'Owned preview resources not released'}
    $report.ReentrantDetachSafe=$cleanup.reentrantDetachSafe
    $report.Cleanup=$true
    $shutdown=Command 'shutdown' @{}
    if(!$process.WaitForExit(5000) -or $process.ExitCode -ne 0){throw 'Helper did not shut down'}
    $report.Success=$true
}catch {
    $report.Failure=$_.Exception.Message
    if($writer -and !$process.HasExited){try{$null=Command 'stopMediaPreview' @{token=99};$cleanup=Command 'probeOwnedCleanup' @{};$report.ReentrantDetachSafe=$cleanup.reentrantDetachSafe;$report.Cleanup=$cleanup.windowDestroyed -and $cleanup.videos.sources -eq 0 -and $cleanup.videos.sinks -eq 0 -and $cleanup.voiceSessions -eq 0 -and $cleanup.reentrantDetachSafe;$null=Command 'shutdown' @{};$null=$process.WaitForExit(3000)}catch{}}
}finally {
    if($process.Id -and !$process.HasExited){$process.Kill($true);$null=$process.WaitForExit(5000)}
    $report.ExitCode=$process.ExitCode
    if($stderr -and $stderr.IsCompleted){$stderr.Result|Set-Content -LiteralPath (Join-Path $Root 'evidence/owned-window-preview-stderr.txt') -Encoding utf8}
    if($stdout -and $stdout.IsCompleted){$stdout.Result|Set-Content -LiteralPath (Join-Path $Root 'evidence/owned-window-preview-stdout.txt') -Encoding utf8}
    if($writer){$writer.Dispose()};if($reader){$reader.Dispose()};$video.Dispose();$pipe.Dispose();$process.Dispose()
    $report.NativePreviewErrors=$script:previewErrors
    $evidence=Join-Path $Root 'evidence';New-Item -ItemType Directory -Path $evidence -Force|Out-Null
    $report|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $evidence 'owned-window-preview-probe.json') -Encoding utf8
}
$report|ConvertTo-Json -Depth 8
if(!$report.Success){throw 'Owned-window native preview gate failed; see exact report'}
