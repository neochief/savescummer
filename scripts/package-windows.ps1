param(
    [ValidateSet('dev', 'release')][string]$Mode = 'release',
    [ValidateSet('RelWithDebInfo', 'Release')][string]$Configuration = 'Release',
    [string]$DesktopBuildDirectory,
    [string]$HostBinary,
    [string]$CliBinary,
    [string]$ExplorerDll,
    [string]$OutputDirectory,
    [string]$QtPrefix = "$PSScriptRoot/../.runtime/Qt/6.5.3/msvc2019_64"
)
$ErrorActionPreference = 'Stop'
if ($Mode -eq 'dev' -and -not $PSBoundParameters.ContainsKey('Configuration')) {
    $Configuration = 'RelWithDebInfo'
}
$root = (Resolve-Path "$PSScriptRoot/..").Path
. (Join-Path $PSScriptRoot 'package-common.ps1')
$profile = if ($Mode -eq 'dev') { 'debug' } else { 'release' }
$version = Get-AppVersion -CargoManifest (Join-Path $root 'Cargo.toml')
if (-not $DesktopBuildDirectory) { $DesktopBuildDirectory = Join-Path $root "build/$Mode/desktop" }
if (-not $HostBinary) { $HostBinary = Join-Path $root "target/$profile/savescummer-host.exe" }
if (-not $CliBinary) { $CliBinary = Join-Path $root "target/$profile/savescummer-cli.exe" }
if (-not $OutputDirectory) {
    $OutputDirectory = if ($Mode -eq 'release') {
        Join-Path $root 'dist/SaveScummer-windows-x64'
    } else {
        Join-Path $root 'build/dev/SaveScummer-windows-x64'
    }
}
$cmake = Get-Command cmake -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $cmake) { $cmake = Join-Path $root '.runtime/qt-tools/cmake/data/bin/cmake.exe' }
$package = [IO.Path]::GetFullPath($OutputDirectory)
$portableName = 'SaveScummer-windows-x64'

# Distributables live only in dist/ (release) or build/dev (dev packages).
$allowedParents = @(
    [IO.Path]::GetFullPath((Join-Path $root 'dist')),
    [IO.Path]::GetFullPath((Join-Path $root 'build/dev'))
)
if ((Split-Path -Parent $package) -notin $allowedParents -or (Split-Path -Leaf $package) -ne $portableName) {
    throw 'Package output must be dist/SaveScummer-windows-x64 (release) or build/dev/SaveScummer-windows-x64 (dev).'
}
$archiveName = Get-PortableArtifactName -Os 'windows' -Arch 'x64' -Version $version `
    -Suffix $(if ($Mode -eq 'dev') { 'dev' } else { $null })
$archive = Join-Path (Split-Path -Parent $package) $archiveName
$stage = New-PackageStage -PackageDirectory $package

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
$installPrefix = Join-Path $stage $portableName
try {
    & $cmake --install $DesktopBuildDirectory --config $Configuration --prefix $installPrefix
    if ($LASTEXITCODE -ne 0) { throw 'Qt deployment failed.' }
    $bin = Join-Path $installPrefix 'bin'
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
    # The Explorer extension ships in release packages and is registered on
    # demand (the installer enables it by default; the portable package opts in
    # through the helper scripts beside SaveScummer.exe).
    if ($ExplorerDll) {
        $explorerFile = (Resolve-Path -LiteralPath $ExplorerDll).Path
        if ([IO.Path]::GetExtension($explorerFile) -ne '.dll') { throw 'ExplorerDll must be a .dll file.' }
        Copy-Item -LiteralPath $explorerFile -Destination (Join-Path $bin 'savescummer-explorer.dll')
        Copy-Item -LiteralPath (Join-Path $root 'scripts/register-explorer.ps1') -Destination (Join-Path $bin 'register-explorer.ps1')
        foreach ($helper in @(
            'Enable Explorer integration.cmd',
            'Disable Explorer integration.cmd'
        )) {
            Copy-Item -LiteralPath (Join-Path $root "packaging/windows/portable/$helper") -Destination (Join-Path $installPrefix $helper)
        }
    }
    Copy-Item -LiteralPath (Join-Path $root 'packaging/windows/licenses') -Destination $installPrefix -Recurse
    $explorerNote = if ($ExplorerDll) {
        @"

The Explorer context-menu integration (bin\savescummer-explorer.dll) is included.
Enable it for the current user with "Enable Explorer integration.cmd"; disable it
with "Disable Explorer integration.cmd". No administrator rights are required.
"@
    } else { '' }
    @"
SaveScummer — Windows x64 ($Mode), version $version

Open bin\SaveScummer.exe. Keep this entire folder together.
Qt and the Visual C++ runtime are included; no SDK or PowerShell launcher is needed.
The background host continues running after the UI closes.
For a simulation that changes no game files, run the desktop with --demo.

Rust profile: $profile. Qt configuration: $Configuration.
Rebuild both components and the package from source with: ./build.ps1 $Mode -Package
Developer symbols are included in dev packages and omitted from release packages.
$explorerNote
Qt $qtVersion is dynamically linked. Copyright The Qt Company Ltd. and contributors.
Qt is used under LGPL version 3; see licenses\LGPL-3.0-only.txt and GPL-3.0-only.txt.
Qt source: https://github.com/qt/qtbase/tree/v$qtVersion
Qt SVG source: https://github.com/qt/qtsvg/tree/v$qtVersion
The Qt DLLs can be replaced with interface-compatible builds.
"@ | Set-Content -LiteralPath (Join-Path $installPrefix 'README.txt') -Encoding utf8
    Write-PackageManifest -PackageDirectory $installPrefix -Fields @{
        mode = $Mode; version = $version; platform = 'windows-x64';
        qtVersion = $qtVersion; qtConfiguration = $Configuration;
        explorer = [bool]$ExplorerDll;
        rustProfile = $profile; createdAt = [DateTime]::UtcNow.ToString('o')
    }
    Write-Sha256Sums -PackageDirectory $installPrefix
    Set-PortablePackage -Package $package -Stage $stage -PortableFolder $portableName `
        -Archive $archive -PreReplace { Stop-PackageProcess }
    $folderBytes = (Get-ChildItem -LiteralPath $package -Recurse -File | Measure-Object Length -Sum).Sum
    Write-Output "Executable: $(Join-Path $package 'bin/SaveScummer.exe')"
    Write-Output "Archive: $archive"
    Write-Output ('Package: {0:N1} MiB unpacked; {1:N1} MiB ZIP' -f ($folderBytes / 1MB), ((Get-Item $archive).Length / 1MB))
} finally {
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
}
