param([string]$Root=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path)
$ErrorActionPreference='Stop'
$data=Join-Path $Root '.test-data\mls-probe'
New-Item -ItemType Directory -Path $data -Force | Out-Null
$reportPath=Join-Path $data 'native-mls-probe.json'
if(Test-Path -LiteralPath $reportPath){Remove-Item -LiteralPath $reportPath -Force}
$info=[System.Diagnostics.ProcessStartInfo]::new((Join-Path $Root 'media-host\runtime\NotDiscord.Media.exe'))
$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.WorkingDirectory=$Root
$info.Environment.Remove('ELECTRON_RUN_AS_NODE') | Out-Null
$info.Environment.Remove('NODE_OPTIONS') | Out-Null
$info.Environment['NOTDISCORD_NATIVE_ROOT']=Join-Path $Root 'media-host\runtime'
$info.Environment['NOTDISCORD_RUST_ADDON']=Join-Path $Root '.test-bin\NotDiscord.MediaProbe.node'
$info.Environment['NOTDISCORD_MEDIA_PIPE']='\\.\pipe\NotDiscord.Media.'+[guid]::NewGuid().ToString('N')
$info.Environment['NOTDISCORD_MEDIA_DATA']=$data
$info.Environment['NOTDISCORD_MEDIA_PROBE']='MLS'
$process=[System.Diagnostics.Process]::new();$process.StartInfo=$info
try{
  if(!$process.Start()){throw 'Could not launch native MLS probe'}
  $discardOut=$process.StandardOutput.ReadToEndAsync();$discardErr=$process.StandardError.ReadToEndAsync()
  if(!$process.WaitForExit(12000)){throw 'Native MLS probe timed out'}
  $report=Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
  if($process.ExitCode -ne 0 -or !$report.Success -or !$report.LoopbackOnly -or !$report.FakeAudioCapture -or !$report.FakeVideoCapture -or $report.MlsKeyPackageBytes -le 0 -or !$report.NativeReferenceReleased){throw 'Native MLS probe failed'}
  New-Item -ItemType Directory -Path (Join-Path $Root 'evidence') -Force | Out-Null
  Copy-Item -LiteralPath (Join-Path $data 'native-mls-probe.json') -Destination (Join-Path $Root 'evidence\native-mls-probe.json') -Force
  $report | ConvertTo-Json -Compress
}finally{
  if($process.Id -and !$process.HasExited){$process.Kill($true);$process.WaitForExit(5000) | Out-Null}
  $process.Dispose()
}
