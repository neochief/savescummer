$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $HOME '.cargo' 'bin'
    $cargoExe = if ($IsWindows) { 'cargo.exe' } else { 'cargo' }
    if (Test-Path -LiteralPath (Join-Path $cargoBin $cargoExe)) {
        $env:PATH = $cargoBin + [IO.Path]::PathSeparator + $env:PATH
    }
}
cargo fmt --all --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo clippy --workspace --all-targets --locked -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo test --workspace --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
cargo build --workspace --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& (Join-Path $PSScriptRoot 'catalog.ps1') -Check
exit $LASTEXITCODE
