param(
    [ValidateSet('dev', 'release')][string]$Mode = 'release',
    [ValidateSet('RelWithDebInfo', 'Release')][string]$Configuration = 'Release',
    [string]$DesktopBuildDirectory,
    [string]$HostBinary,
    [string]$CliBinary,
    [string]$OutputDirectory,
    [string]$QtPrefix = "$PSScriptRoot/../.runtime/Qt/6.5.3/msvc2019_64"
)
$ErrorActionPreference = 'Stop'
if ($Mode -eq 'dev' -and -not $PSBoundParameters.ContainsKey('Configuration')) {
    $Configuration = 'RelWithDebInfo'
}
$root = (Resolve-Path "$PSScriptRoot/..").Path
$profile = if ($Mode -eq 'dev') { 'debug' } else { 'release' }
if (-not $DesktopBuildDirectory) { $DesktopBuildDirectory = Join-Path $root "build/$Mode/desktop" }
if (-not $HostBinary) { $HostBinary = Join-Path $root "target/$profile/savescummer-host.exe" }
if (-not $CliBinary) { $CliBinary = Join-Path $root "target/$profile/savescummer.exe" }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root "build/$Mode/SaveScummer" }
$cmake = Get-Command cmake -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $cmake) { $cmake = Join-Path $root '.runtime/qt-tools/cmake/data/bin/cmake.exe' }
$package = [IO.Path]::GetFullPath($OutputDirectory)
$buildRoot = [IO.Path]::GetFullPath((Join-Path $root 'build')) + [IO.Path]::DirectorySeparatorChar
if (-not $package.StartsWith($buildRoot, [StringComparison]::OrdinalIgnoreCase) -or
    (Split-Path $package -Leaf) -ne 'SaveScummer') {
    throw 'Package output must be a SaveScummer folder inside this repository/build.'
}
if ((Test-Path -LiteralPath $package) -and
    -not (Test-Path -LiteralPath (Join-Path $package '.savescummer-package.json'))) {
    throw "Refusing to replace an unmanaged directory: $package"
}
$packagePrefix = $package + [IO.Path]::DirectorySeparatorChar
$running = Get-Process | Where-Object {
    $_.Path -and $_.Path.StartsWith($packagePrefix, [StringComparison]::OrdinalIgnoreCase)
}
if ($running) { throw 'Close the existing packaged app and shut down its host before replacing this package.' }
$hostFile = (Resolve-Path -LiteralPath $HostBinary).Path
$cliFile = (Resolve-Path -LiteralPath $CliBinary).Path
$qtVersion = & (Join-Path $QtPrefix 'bin/qmake.exe') -query QT_VERSION
if ($LASTEXITCODE -ne 0) { throw 'Cannot determine the Qt version.' }
$stage = $package + '.staging-' + [guid]::NewGuid().ToString('N')
$archive = Join-Path (Split-Path $package) "SaveScummer-windows-x64-$Mode.zip"
$temporaryZip = $archive + '.' + [guid]::NewGuid().ToString('N') + '.zip'
try {
    & $cmake --install $DesktopBuildDirectory --config $Configuration --prefix $stage
    if ($LASTEXITCODE -ne 0) { throw 'Qt deployment failed.' }
    $bin = Join-Path $stage 'bin'
    Copy-Item -LiteralPath $hostFile -Destination (Join-Path $bin 'savescummer-host.exe')
    Copy-Item -LiteralPath $cliFile -Destination (Join-Path $bin 'savescummer.exe')
    if ($Mode -eq 'dev') {
        foreach ($symbol in @((Join-Path (Split-Path $hostFile) 'savescummer_host.pdb'),
            [IO.Path]::ChangeExtension($cliFile, '.pdb'),
            (Join-Path $DesktopBuildDirectory "apps/desktop/$Configuration/savescummer-desktop.pdb"))) {
            if (Test-Path -LiteralPath $symbol) { Copy-Item -LiteralPath $symbol -Destination $bin }
        }
    }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $visualStudio = & $vswhere -latest -products '*' -property installationPath
    if (-not $visualStudio) { throw 'Cannot locate the Visual C++ redistributable runtime.' }
    $redist = Get-ChildItem -LiteralPath (Join-Path $visualStudio 'VC/Redist/MSVC') -Directory |
        Where-Object { $_.Name -match '^\d+\.' } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    $crt = Get-ChildItem -LiteralPath (Join-Path $redist.FullName 'x64') -Directory |
        Where-Object Name -Like 'Microsoft.VC*.CRT' | Select-Object -First 1
    if (-not $crt) { throw 'Cannot locate x64 CRT DLLs.' }
    Get-ChildItem -LiteralPath $crt.FullName -Filter '*.dll' | Copy-Item -Destination $bin
    Copy-Item -LiteralPath (Join-Path $root 'packaging/licenses') -Destination $stage -Recurse
    @"
Save Scummer — Windows x64 ($Mode)

Open bin\savescummer-desktop.exe. Keep this entire folder together.
Qt and the Visual C++ runtime are included; no SDK or PowerShell launcher is needed.
The background host continues running after the UI closes.
For a simulation that changes no game files, run the desktop with --demo.

Rust profile: $profile. Qt configuration: $Configuration.
Rebuild both components and the package from source with: ./build.ps1 $Mode -Package
Developer symbols are included in dev packages and omitted from release packages.

Qt $qtVersion is dynamically linked. Copyright The Qt Company Ltd. and contributors.
Qt is used under LGPL version 3; see licenses\LGPL-3.0-only.txt and GPL-3.0-only.txt.
Qt source: https://github.com/qt/qtbase/tree/v$qtVersion
Qt SVG source: https://github.com/qt/qtsvg/tree/v$qtVersion
The Qt DLLs can be replaced with interface-compatible builds.
"@ | Set-Content -LiteralPath (Join-Path $stage 'README.txt') -Encoding utf8
    @{ mode = $Mode; qtVersion = $qtVersion; qtConfiguration = $Configuration;
        rustProfile = $profile; createdAt = [DateTime]::UtcNow.ToString('o') } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $stage '.savescummer-package.json')
    Get-ChildItem -LiteralPath $stage -Recurse -File | ForEach-Object {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(),
            $_.FullName.Substring($stage.Length + 1)
    } | Set-Content -LiteralPath (Join-Path $stage 'SHA256SUMS.txt') -Encoding ascii
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $temporaryZip
    # Only replace marked generated output after every build/deployment step succeeds.
    # Both absolute paths were validated as descendants of repository/build above.
    if (Test-Path -LiteralPath $package) { Remove-Item -LiteralPath $package -Recurse -Force }
    Move-Item -LiteralPath $stage -Destination $package
    Move-Item -LiteralPath $temporaryZip -Destination $archive -Force
    $folderBytes = (Get-ChildItem -LiteralPath $package -Recurse -File | Measure-Object Length -Sum).Sum
    Write-Output "Executable: $(Join-Path $package 'bin/savescummer-desktop.exe')"
    Write-Output "Archive: $archive"
    Write-Output ('Package: {0:N1} MiB unpacked; {1:N1} MiB ZIP' -f ($folderBytes / 1MB), ((Get-Item $archive).Length / 1MB))
} finally {
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    if (Test-Path -LiteralPath $temporaryZip) { Remove-Item -LiteralPath $temporaryZip }
}
