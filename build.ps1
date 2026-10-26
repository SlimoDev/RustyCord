param([switch]$SelfContained)
$ErrorActionPreference = 'Stop'
$ndRoot = $PSScriptRoot
if (-not (Test-Path -LiteralPath (Join-Path $ndRoot '.notdiscord-root'))) { throw 'No se encontró la raíz NotDiscord.' }
$env:DOTNET_CLI_HOME = Join-Path $ndRoot 'build\dotnet-home'
$env:NUGET_PACKAGES = Join-Path $ndRoot 'build\nuget'
$env:NUGET_HTTP_CACHE_PATH = Join-Path $ndRoot 'build\http-cache'
$env:TEMP = Join-Path $ndRoot 'build\temp'
$env:TMP = $env:TEMP
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_SKIP_FIRST_TIME_EXPERIENCE = '1'
$env:DOTNET_NOLOGO = '1'
$ndPaths = @($env:DOTNET_CLI_HOME,$env:NUGET_PACKAGES,$env:NUGET_HTTP_CACHE_PATH,$env:TEMP)
foreach ($ndPath in $ndPaths) { New-Item -ItemType Directory -Path $ndPath -Force | Out-Null }
$ndProject = Join-Path $ndRoot 'src\NotDiscord\NotDiscord.csproj'
$ndMode = if ($SelfContained) { 'true' } else { 'false' }
& dotnet publish $ndProject -c Release -r win-x64 --self-contained $ndMode -o (Join-Path $ndRoot 'bin') -p:NuGetAudit=false
if ($LASTEXITCODE -ne 0) { throw "La compilación falló ($LASTEXITCODE)." }
Write-Output (Join-Path $ndRoot 'bin\NotDiscord.exe')
