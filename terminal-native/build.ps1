param([switch]$Test)
$ErrorActionPreference = 'Stop'
$ndRoot = $PSScriptRoot
$env:DOTNET_CLI_HOME = Join-Path $ndRoot '..\build\dotnet-home'
$env:NUGET_PACKAGES = Join-Path $ndRoot '..\build\nuget'
$env:NUGET_HTTP_CACHE_PATH = Join-Path $ndRoot '..\build\http-cache'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_SKIP_FIRST_TIME_EXPERIENCE = '1'
& dotnet publish (Join-Path $ndRoot 'src\NotDiscord.Terminal.csproj') -c Release -r win-x64 --self-contained false -o (Join-Path $ndRoot 'bin') -p:NuGetAudit=false
if ($LASTEXITCODE -ne 0) { throw 'Falló la compilación del cliente terminal.' }
if ($Test) {
    & dotnet run --project (Join-Path $ndRoot 'tests\FeatureTests.csproj') -c Release --no-restore -p:NuGetAudit=false
    if ($LASTEXITCODE -ne 0) { throw 'Fallaron las pruebas de funciones nativas.' }
}
