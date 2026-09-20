param(
    [switch]$Demo,
    [ValidateSet('system','dark','light')][string]$Theme = 'system',
    [string]$DataDirectory,
    [string]$QtPrefix = "$PSScriptRoot/../.runtime/Qt/6.5.3/msvc2019_64"
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path
$desktop = Join-Path $root 'build/desktop/apps/desktop/Release/savescummer-desktop.exe'
if (-not (Test-Path -LiteralPath $desktop)) { throw 'Run scripts/build-desktop.ps1 first.' }
$env:PATH = "$(Join-Path $QtPrefix 'bin');$env:PATH"
$desktopArgs = @('--theme', $Theme)
if ($Demo) { $desktopArgs += '--demo' }
else {
    $hostExecutable = Join-Path $root 'target/debug/savescummer-host.exe'
    if (-not (Test-Path -LiteralPath $hostExecutable)) { throw 'Run cargo build --bin savescummer-host first.' }
    $desktopArgs += @('--host', $hostExecutable)
    if ($DataDirectory) { $desktopArgs += @('--data-dir', $DataDirectory) }
}
& $desktop @desktopArgs
