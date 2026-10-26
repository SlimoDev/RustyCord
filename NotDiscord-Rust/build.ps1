param([switch]$Test, [switch]$Release, [switch]$Offline, [ValidateRange(1,64)][int]$Jobs=1)
$ErrorActionPreference = 'Stop'
$toolRoot = Join-Path (Split-Path $PSScriptRoot -Parent) '.tools'
if (Test-Path -LiteralPath (Join-Path $toolRoot 'cargo\bin\cargo.exe')) {
    $env:CARGO_HOME = Join-Path $toolRoot 'cargo'
    $env:RUSTUP_HOME = Join-Path $toolRoot 'rustup'
    $cargo = Join-Path $toolRoot 'cargo\bin\cargo.exe'
} else { $cargo = 'cargo' }
Push-Location $PSScriptRoot
try {
    $commonArgs = @('--workspace', '--jobs', $Jobs)
    if ($Offline) { $commonArgs += '--offline' }
    if ($Test) { & $cargo test @commonArgs; if ($LASTEXITCODE) { throw 'Pruebas Rust fallidas.' } }
    $cargoArgs = @('build') + $commonArgs
    if ($Release) { $cargoArgs += '--release' }
    & $cargo @cargoArgs
    if ($LASTEXITCODE) { throw 'Compilación Rust fallida.' }
} finally { Pop-Location }
