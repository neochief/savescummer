param([string]$BuildDirectory = "$PSScriptRoot/../build/explorer", [string]$Generator = 'Visual Studio 16 2019', [string]$DevDataDirectory = '')
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path
$cmake = Get-Command cmake -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $cmake) { $cmake = Join-Path $root '.runtime/qt-tools/cmake/data/bin/cmake.exe' }
if (-not (Test-Path -LiteralPath $cmake)) { throw 'Install CMake 3.21 or newer.' }
& $cmake -S (Join-Path $root 'integrations/windows-explorer') -B $BuildDirectory -G $Generator -A x64 "-DSAVESCUMMER_EXPLORER_DEV_DATA_DIR=$DevDataDirectory"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& $cmake --build $BuildDirectory --config Release --parallel
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& (Join-Path (Split-Path $cmake) 'ctest.exe') --test-dir $BuildDirectory -C Release --output-on-failure
exit $LASTEXITCODE
