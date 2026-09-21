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
if (-not $CliBinary) { $CliBinary = Join-Path $root "target/$profile/savescummer-cli.exe" }
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
function Get-PackageProcess {
    @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $_.Path -and $_.Path.StartsWith($packagePrefix, [StringComparison]::OrdinalIgnoreCase)
    })
}
function Wait-PackageProcess([object[]]$Process, [int]$TimeoutMilliseconds) {
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    foreach ($item in $Process) {
        try {
            if ($item.HasExited) { continue }
            $remaining = [math]::Max(0, [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds)
            if ($remaining -gt 0) { $null = $item.WaitForExit($remaining) }
        } catch [InvalidOperationException] {
            # The process exited between the snapshot and the wait.
        }
    }
}
function Get-HostDataDirectory([Diagnostics.Process]$Process) {
    try {
        $commandLine = (Get-CimInstance Win32_Process -Filter "ProcessId = $($Process.Id)" `
            -ErrorAction Stop).CommandLine
    } catch {
        return $null
    }
    if ($commandLine -match '(?i)(?:^|\s)--data-dir(?:\s+|=)(?:"([^"]+)"|(\S+))') {
        if ($Matches[1]) { return $Matches[1] }
        return $Matches[2]
    }
    return $null
}
function Request-HostShutdown([string]$Cli, [string]$DataDirectory) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Cli
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @('--data-dir', $DataDirectory, '--no-start', 'shutdown')) {
        $start.ArgumentList.Add($argument)
    }
    $client = $null
    try {
        $client = [Diagnostics.Process]::Start($start)
        if (-not $client.WaitForExit(10000)) {
            $client.Kill()
            $null = $client.WaitForExit(5000)
            return $false
        }
        return $client.ExitCode -eq 0
    } catch {
        return $false
    } finally {
        if ($client) { $client.Dispose() }
    }
}
function Stop-PackageProcess {
    $running = @(Get-PackageProcess)
    if ($running.Count -eq 0) { return }
    Write-Host 'Stopping the existing portable package before replacing it.'

    # Closing the UI first prevents it from reconnecting while its host exits.
    foreach ($desktop in @($running | Where-Object {
        [IO.Path]::GetFileName($_.Path) -eq 'SaveScummer.exe'
    })) {
        Write-Host "Closing packaged desktop (PID $($desktop.Id))."
        try {
            if ($desktop.CloseMainWindow()) { $null = $desktop.WaitForExit(5000) }
            if (-not $desktop.HasExited) {
                Stop-Process -InputObject $desktop -Force -ErrorAction Stop
                $null = $desktop.WaitForExit(10000)
            }
        } catch {
            if (-not $desktop.HasExited) { throw }
        }
    }

    $hosts = @(Get-PackageProcess | Where-Object {
        [IO.Path]::GetFileName($_.Path) -eq 'SaveScummer.Host.exe'
    })
    $packageCli = Join-Path $package 'bin/SaveScummer.CLI.exe'
    if ($hosts.Count -gt 0 -and (Test-Path -LiteralPath $packageCli)) {
        $gracefulHosts = @()
        foreach ($hostProcess in $hosts) {
            $dataDirectory = Get-HostDataDirectory -Process $hostProcess
            if (-not $dataDirectory) {
                Write-Warning "Cannot resolve the data directory for packaged host PID $($hostProcess.Id); it will be terminated."
                continue
            }
            Write-Host "Requesting graceful shutdown of packaged host PID $($hostProcess.Id)."
            if (-not (Request-HostShutdown -Cli $packageCli -DataDirectory $dataDirectory)) {
                Write-Warning "Packaged host PID $($hostProcess.Id) did not accept graceful shutdown; it will be terminated."
            } else {
                $gracefulHosts += $hostProcess
            }
        }
        Wait-PackageProcess -Process $gracefulHosts -TimeoutMilliseconds 30000
    }

    # This final exact-path pass covers hidden/unresponsive desktops, hosts whose
    # data directory could not be resolved, and any newly started packaged process.
    foreach ($process in @(Get-PackageProcess)) {
        Write-Host "Terminating packaged process $($process.ProcessName) (PID $($process.Id))."
        try {
            Stop-Process -InputObject $process -Force -ErrorAction Stop
        } catch {
            if (-not $process.HasExited) { throw }
        }
    }
    $remaining = @(Get-PackageProcess)
    Wait-PackageProcess -Process $remaining -TimeoutMilliseconds 10000
    $remaining = @(Get-PackageProcess)
    if ($remaining.Count -gt 0) {
        throw "Could not stop packaged process PID(s): $($remaining.Id -join ', ')."
    }
}
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
    Copy-Item -LiteralPath $hostFile -Destination (Join-Path $bin 'SaveScummer.Host.exe')
    Copy-Item -LiteralPath $cliFile -Destination (Join-Path $bin 'SaveScummer.CLI.exe')
    if ($Mode -eq 'dev') {
        foreach ($symbol in @(
            @((Join-Path (Split-Path $hostFile) 'savescummer_host.pdb'), 'SaveScummer.Host.pdb'),
            @((Join-Path (Split-Path $cliFile) 'savescummer_cli.pdb'), 'SaveScummer.CLI.pdb'),
            @((Join-Path $DesktopBuildDirectory "apps/desktop/$Configuration/SaveScummer.pdb"), 'SaveScummer.pdb')
        )) {
            if (Test-Path -LiteralPath $symbol[0]) {
                Copy-Item -LiteralPath $symbol[0] -Destination (Join-Path $bin $symbol[1])
            }
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
SaveScummer — Windows x64 ($Mode)

Open bin\SaveScummer.exe. Keep this entire folder together.
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
    Stop-PackageProcess
    if (Test-Path -LiteralPath $package) { Remove-Item -LiteralPath $package -Recurse -Force }
    Move-Item -LiteralPath $stage -Destination $package
    Move-Item -LiteralPath $temporaryZip -Destination $archive -Force
    $folderBytes = (Get-ChildItem -LiteralPath $package -Recurse -File | Measure-Object Length -Sum).Sum
    Write-Output "Executable: $(Join-Path $package 'bin/SaveScummer.exe')"
    Write-Output "Archive: $archive"
    Write-Output ('Package: {0:N1} MiB unpacked; {1:N1} MiB ZIP' -f ($folderBytes / 1MB), ((Get-Item $archive).Length / 1MB))
} finally {
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    if (Test-Path -LiteralPath $temporaryZip) { Remove-Item -LiteralPath $temporaryZip }
}
