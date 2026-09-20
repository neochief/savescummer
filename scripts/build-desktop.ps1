param(
    [string]$QtPrefix = "$PSScriptRoot/../.runtime/Qt/6.5.3/msvc2019_64",
    [string]$BuildDirectory = "$PSScriptRoot/../build/desktop",
    [string]$Generator = 'Visual Studio 16 2019',
    [ValidateSet('Debug', 'RelWithDebInfo', 'Release')][string]$Configuration = 'Release',
    [string]$TestHost,
    [switch]$SkipTests
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path
$cmake = Get-Command cmake -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $cmake) {
    $cmake = Join-Path $root '.runtime/qt-tools/cmake/data/bin/cmake.exe'
}
if (-not (Test-Path -LiteralPath $cmake)) { throw 'Install CMake 3.21 or newer and add it to PATH.' }
& $cmake -S $root -B $BuildDirectory -G $Generator -A x64 "-DCMAKE_PREFIX_PATH=$QtPrefix"
if ($LASTEXITCODE -ne 0) { throw 'Desktop configuration failed.' }
[string[]]$buildTargets = if ($SkipTests) { @('savescummer-desktop') } else { @('savescummer-desktop', 'desktop-tests') }
$buildArguments = @('--build', $BuildDirectory, '--config', $Configuration, '--parallel', '--target') + $buildTargets
& $cmake @buildArguments
if ($LASTEXITCODE -ne 0) { throw 'Desktop compilation failed.' }
$env:PATH = "$(Join-Path $QtPrefix 'bin');$env:PATH"
if (-not $SkipTests) {
    $previousTestHost = $env:SAVESCUMMER_TEST_HOST
    try {
        if ($TestHost) { $env:SAVESCUMMER_TEST_HOST = (Resolve-Path -LiteralPath $TestHost).Path }
        & (Join-Path (Split-Path $cmake) 'ctest.exe') --test-dir $BuildDirectory -C $Configuration --output-on-failure
        if ($LASTEXITCODE -ne 0) {
            $results = Join-Path $BuildDirectory 'apps/desktop/desktop-results.xml'
            if (Test-Path -LiteralPath $results) { Get-Content -LiteralPath $results | Write-Host }
            throw 'Desktop tests failed.'
        }
    } finally { $env:SAVESCUMMER_TEST_HOST = $previousTestHost }
}
