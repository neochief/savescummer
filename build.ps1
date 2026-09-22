param(
    [Parameter(Position = 0)][ValidateSet('dev', 'release', 'clean')][string]$Mode = 'dev',
    [switch]$Run,
    [switch]$Demo,
    [switch]$Package,
    [switch]$Test,
    [switch]$Deep,
    [switch]$KeepProduction,
    [string]$QtPrefix = "$PSScriptRoot/.runtime/Qt/6.5.3/msvc2019_64",
    [string]$Generator = 'Visual Studio 16 2019'
)
$ErrorActionPreference = 'Stop'
if ($Run -and $Mode -ne 'dev') { throw '-Run is for development. Open the packaged executable for a release.' }
if ($Demo -and -not $Run) { throw '-Demo requires -Run.' }
if ($KeepProduction -and -not $Run) { throw '-KeepProduction requires -Run.' }
$root = $PSScriptRoot

# Stops the recorded development host gracefully so accepted save/restore work
# can finish. Never forcibly terminates a host that may be mid-operation.
function Stop-RecordedDevHost {
    param(
        [string]$CliBinary,
        [string]$DataDirectory,
        [string]$SessionFile,
        [switch]$AllowMissingCli
    )
    if (-not (Test-Path -LiteralPath $SessionFile)) { return }
    $session = Get-Content -LiteralPath $SessionFile -Raw | ConvertFrom-Json
    $record = $session.host
    if (-not $record) { return }
    $process = Get-Process -Id $record.pid -ErrorAction SilentlyContinue
    if (-not $process -or $process.StartTime.ToUniversalTime().Ticks -ne $record.started -or
        $process.Path -ne $record.path) { return }
    if (-not (Test-Path -LiteralPath $CliBinary)) {
        if ($AllowMissingCli) {
            Write-Warning 'Cannot gracefully stop the recorded development host (CLI missing); it will keep running.'
            return
        }
        throw 'The development CLI is missing; cannot stop the recorded host safely.'
    }
    & $CliBinary --data-dir $DataDirectory shutdown
    if ($LASTEXITCODE -ne 0) { throw 'Could not stop the previous development host safely.' }
    if (-not $process.WaitForExit(30000)) {
        throw 'Development process is still finishing work. Retry the build after it exits.'
    }
    Remove-Item -LiteralPath $SessionFile
}

# Stops processes running from output folders (build/, dist/) so clean can
# remove them. Desktops are closed first, then packaged hosts are asked to shut
# down gracefully (accepted operations finish); the final pass terminates
# stragglers. Only processes whose executable lives under the given folders are
# affected; machine-local target/ and .runtime/ processes are never touched.
function Stop-OutputProcesses {
    param(
        [string]$Root,
        [string[]]$OutputDirectories
    )
    $bases = @($OutputDirectories | ForEach-Object {
        [IO.Path]::GetFullPath((Join-Path $Root $_)) + [IO.Path]::DirectorySeparatorChar
    })
    $running = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $p = $_
        $p.Path -and ($bases | Where-Object { $p.Path.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) })
    })
    if ($running.Count -eq 0) { return }
    Write-Host 'Stopping processes running from build/ and dist/ before cleaning.'

    $cli = @{}
    foreach ($hostProcess in @($running | Where-Object {
        [IO.Path]::GetFileName($_.Path) -eq 'SaveScummer.Host.exe'
    })) {
        $siblingCli = Join-Path (Split-Path -Parent $hostProcess.Path) 'SaveScummer.CLI.exe'
        if (Test-Path -LiteralPath $siblingCli) { $cli[$hostProcess.Id] = $siblingCli }
    }
    foreach ($desktop in @($running | Where-Object {
        [IO.Path]::GetFileName($_.Path) -eq 'SaveScummer.exe'
    })) {
        Write-Host "Closing desktop (PID $($desktop.Id))."
        try {
            if ($desktop.CloseMainWindow()) { $null = $desktop.WaitForExit(5000) }
            if (-not $desktop.HasExited) { Stop-Process -InputObject $desktop -Force -ErrorAction Stop }
        } catch {
            if (-not $desktop.HasExited) { throw }
        }
    }
    foreach ($hostProcess in @($running | Where-Object {
        [IO.Path]::GetFileName($_.Path) -eq 'SaveScummer.Host.exe'
    })) {
        if ($cli.ContainsKey($hostProcess.Id)) {
            Write-Host "Requesting graceful shutdown of host (PID $($hostProcess.Id))."
            $start = [Diagnostics.ProcessStartInfo]::new()
            $start.FileName = $cli[$hostProcess.Id]
            $start.UseShellExecute = $false
            $start.CreateNoWindow = $true
            $start.RedirectStandardOutput = $true
            $start.RedirectStandardError = $true
            $start.ArgumentList.Add('--no-start')
            $start.ArgumentList.Add('shutdown')
            try {
                $client = [Diagnostics.Process]::Start($start)
                $null = $client.WaitForExit(10000)
                $null = $hostProcess.WaitForExit(30000)
            } catch { }
        }
    }
    foreach ($process in @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $p = $_
        $p.Path -and ($bases | Where-Object { $p.Path.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) })
    })) {
        try { Stop-Process -InputObject $process -Force -ErrorAction SilentlyContinue } catch { }
    }
}

# Stops any SaveScummer host that is not this development instance — typically a
# packaged or installed production host. Development enables OS integrations, so
# a running production host would otherwise own the tray icon and the global
# shortcuts (and pressing Ctrl+F5 would act on the production instance).
# Hosts are asked to shut down gracefully first so accepted save/restore work can
# finish; only a host that refuses is terminated. Pass -KeepProduction to skip.
function Stop-OtherHosts {
    param(
        [string]$DevDataDirectory,
        [string]$FallbackCli,
        [int]$TimeoutSeconds = 30
    )
    $devDataFull = [IO.Path]::GetFullPath($DevDataDirectory)
    $hosts = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $_.Path -and ([IO.Path]::GetFileName($_.Path) -in @('SaveScummer.Host.exe', 'savescummer-host.exe'))
    })
    if ($hosts.Count -eq 0) { return }
    # Close other desktops first so they do not reconnect while their host exits.
    foreach ($desktop in @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        $_.Path -and [IO.Path]::GetFileName($_.Path) -eq 'SaveScummer.exe' -and
        -not $_.Path.StartsWith((Join-Path $PSScriptRoot 'build'), [StringComparison]::OrdinalIgnoreCase)
    })) {
        Write-Host "Closing the other SaveScummer desktop (PID $($desktop.Id))."
        try {
            if ($desktop.CloseMainWindow()) { $null = $desktop.WaitForExit(5000) }
            if (-not $desktop.HasExited) {
                Stop-Process -InputObject $desktop -Force -ErrorAction SilentlyContinue
            }
        } catch { }
    }
    foreach ($hostProcess in $hosts) {
        $dataDirectory = $null
        try {
            $commandLine = (Get-CimInstance Win32_Process -Filter "ProcessId = $($hostProcess.Id)" `
                -ErrorAction Stop).CommandLine
            if ($commandLine -match '(?i)(?:^|\s)--data-dir(?:\s+|=)(?:"([^"]+)"|(\S+))') {
                $dataDirectory = if ($Matches[1]) { $Matches[1] } else { $Matches[2] }
            }
        } catch { }
        # Skip this instance and any other development instance (its data lives
        # under a .runtime directory); everything else is treated as production.
        if ($dataDirectory) {
            $dataFull = [IO.Path]::GetFullPath($dataDirectory)
            if ($dataFull -eq $devDataFull -or $dataFull -match '[\\/]\.runtime[\\/]') { continue }
        }
        $directory = Split-Path -Parent $hostProcess.Path
        $cli = @('SaveScummer.CLI.exe', 'savescummer-cli.exe') |
            ForEach-Object { Join-Path $directory $_ } |
            Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
        if (-not $cli) { $cli = $FallbackCli }
        Write-Host "Stopping the other SaveScummer host (PID $($hostProcess.Id))."
        $arguments = @('--no-start')
        if ($dataDirectory) { $arguments = @('--data-dir', $dataDirectory) + $arguments }
        $arguments += 'shutdown'
        $stopped = $false
        if ($cli -and (Test-Path -LiteralPath $cli)) {
            & $cli @arguments 2>$null | Out-Null
            $stopped = $hostProcess.WaitForExit($TimeoutSeconds * 1000)
        }
        if (-not $stopped) {
            Write-Warning 'The other SaveScummer host did not exit; terminating it. An in-flight operation may need recovery.'
            Stop-Process -InputObject $hostProcess -Force -ErrorAction SilentlyContinue
            $null = $hostProcess.WaitForExit(10000)
        }
    }
}

$modeDirectory = Join-Path $root "build/$Mode"
$desktopBuild = Join-Path $modeDirectory 'desktop'
$configuration = if ($Mode -eq 'dev') { 'RelWithDebInfo' } else { 'Release' }
$rustProfile = if ($Mode -eq 'dev') { 'debug' } else { 'release' }
$hostBinary = Join-Path $root "target/$rustProfile/savescummer-host.exe"
$cliBinary = Join-Path $root "target/$rustProfile/savescummer-cli.exe"
$desktopBinary = Join-Path $desktopBuild "apps/desktop/$configuration/SaveScummer.exe"
$sessionFile = Join-Path $root 'build/dev/session.json'
$devData = Join-Path $root '.runtime/dev'

# clean removes regenerable outputs only: build/ and dist/ by default, and the
# Cargo cache (target/) with -Deep. Machine-local .runtime/ is never touched.
if ($Mode -eq 'clean') {
    if ($Run -or $Demo -or $Package -or $Test) { throw 'clean accepts only -Deep.' }
    Stop-RecordedDevHost -CliBinary (Join-Path $root 'target/debug/savescummer-cli.exe') `
        -DataDirectory $devData -SessionFile $sessionFile -AllowMissingCli
    Stop-OutputProcesses -Root $root -OutputDirectories @('build', 'dist')
    $removed = @()
    foreach ($dir in @('build', 'dist')) {
        $path = Join-Path $root $dir
        if (-not (Test-Path -LiteralPath $path)) { continue }
        # A process that was just stopped may still hold its file handle for a
        # moment; retry briefly before reporting a locked-folder failure.
        $attempt = 0
        while ($true) {
            try {
                Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction Stop
                break
            } catch {
                $attempt++
                if ($attempt -ge 3) { throw }
                Start-Sleep -Milliseconds 1000
            }
        }
        $removed += $dir
    }
    if ($Deep) {
        $path = Join-Path $root 'target'
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Recurse -Force
            $removed += 'target'
        }
    }
    Write-Host ("Clean: removed {0}." -f ($(if ($removed.Count) { $removed -join ', ' } else { 'nothing' }))) -ForegroundColor Green
    exit 0
}
New-Item -ItemType Directory -Force -Path $modeDirectory | Out-Null
$timings = [ordered]@{}
$total = [Diagnostics.Stopwatch]::StartNew()
function Invoke-BuildStep([string]$Name, [scriptblock]$Action) {
    Write-Host "`n$Name" -ForegroundColor Cyan
    $watch = [Diagnostics.Stopwatch]::StartNew()
    & $Action
    $watch.Stop()
    $timings[$Name] = [math]::Round($watch.Elapsed.TotalSeconds, 2)
}

# Windows cannot replace a running executable. Match the exact dev output path
# so hidden/manual/debugger launches are covered without closing packaged apps.
# The desktop owns no file operations; hosts must still shut down gracefully.
if ($Mode -eq 'dev') {
    $expectedDesktopPath = [IO.Path]::GetFullPath($desktopBinary)
    foreach ($desktop in @(Get-Process -Name 'SaveScummer' -ErrorAction SilentlyContinue)) {
        if ($desktop.HasExited -or $desktop.Path -ne $expectedDesktopPath) { continue }
        Write-Host "Closing development desktop (PID $($desktop.Id)) before rebuilding."
        try {
            if ($desktop.CloseMainWindow()) {
                $null = $desktop.WaitForExit(5000)
            }
            if (-not $desktop.HasExited) {
                Write-Host "Terminating hidden or unresponsive development desktop (PID $($desktop.Id))."
                # Use the captured process object, not a fresh lookup by name/PID.
                Stop-Process -InputObject $desktop -Force -ErrorAction Stop
            }
            if (-not $desktop.WaitForExit(10000)) {
                throw "Development desktop PID $($desktop.Id) has not exited."
            }
        } catch {
            if (-not $desktop.HasExited) { throw }
        }
    }
}
# Only recorded hosts are stopped automatically; never forcibly terminate a host
# that may be finishing a save/restore operation.
if ($Mode -eq 'dev') {
    Stop-RecordedDevHost -CliBinary $cliBinary -DataDirectory $devData -SessionFile $sessionFile
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH = "$(Join-Path $env:USERPROFILE '.cargo/bin');$env:PATH"
}
Push-Location $root
try {
    Invoke-BuildStep 'Rust compilation' {
        $cargoArgs = @('build', '--manifest-path', (Join-Path $root 'Cargo.toml'),
            '--target-dir', (Join-Path $root 'target'),
            '-p', 'savescummer-host', '-p', 'savescummer-cli')
        if ($Mode -eq 'release') { $cargoArgs += @('--release', '--locked') }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw 'Rust compilation failed. No package was produced from old binaries.' }
    }
    if ($Test) {
        Invoke-BuildStep 'Rust tests' {
            # Avoid packaged-shell AppData redirection changing the spelling of
            # paths between parent and child processes in integration tests.
            $previousTemp = $env:TEMP
            $previousTmp = $env:TMP
            try {
                $testTemp = Join-Path $root 'build/tmp/test-temp'
                if (Test-Path -LiteralPath $testTemp) {
                    Remove-Item -LiteralPath $testTemp -Recurse -Force
                }
                New-Item -ItemType Directory -Force -Path $testTemp | Out-Null
                $env:TEMP = $testTemp
                $env:TMP = $testTemp
                & cargo test --workspace --locked --target-dir (Join-Path $root 'target')
                if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
            } finally { $env:TEMP = $previousTemp; $env:TMP = $previousTmp }
        }
    }
    Invoke-BuildStep 'Qt configuration, compilation and optional tests' {
        & "$root/scripts/build-desktop.ps1" -QtPrefix $QtPrefix -BuildDirectory $desktopBuild `
            -Generator $Generator -Configuration $configuration -SkipTests:(-not $Test) -TestHost $hostBinary
    }
    if ($Package -or $Mode -eq 'release') {
        Invoke-BuildStep 'Portable folder and ZIP' {
            # Output paths default to dist/SaveScummer-windows-x64 (release) or
            # build/dev/SaveScummer-windows-x64 (dev); see package-windows.ps1.
            & "$root/scripts/package-windows.ps1" -Mode $Mode -Configuration $configuration `
                -DesktopBuildDirectory $desktopBuild -HostBinary $hostBinary -CliBinary $cliBinary `
                -QtPrefix $QtPrefix
        }
    }
    $total.Stop()
    $binaries = [ordered]@{}
    foreach ($entry in @(@('desktop', $desktopBinary), @('host', $hostBinary), @('cli', $cliBinary))) {
        $item = Get-Item -LiteralPath $entry[1]
        $binaries[$entry[0]] = @{ path = $item.FullName; bytes = $item.Length }
    }
    $report = [ordered]@{ mode = $Mode; qtConfiguration = $configuration; testsRequested = [bool]$Test;
        builtAt = [DateTime]::UtcNow.ToString('o'); seconds = [math]::Round($total.Elapsed.TotalSeconds, 2);
        steps = $timings; binaries = $binaries }
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $modeDirectory 'build-report.json')
    Write-Host "`n$Mode build completed in $($report.seconds) seconds." -ForegroundColor Green
    Write-Output "Desktop: $desktopBinary"
    if ($Run) {
        $env:PATH = "$(Join-Path $QtPrefix 'bin');$env:PATH"
        $session = @{}
        if (-not $Demo) {
            # Dedicated settings/history; OS integrations (tray, global shortcuts,
            # notifications) stay enabled so the development app behaves like the
            # real one. Any already-running production host is stopped first so it
            # cannot own the tray or the global shortcuts; use -KeepProduction to
            # leave it alone. Configured game directories are real: use -Demo for
            # simulated operations.
            if (-not $KeepProduction) {
                Stop-OtherHosts -DevDataDirectory $devData -FallbackCli $cliBinary
            }
            $hostLog = Join-Path $modeDirectory 'host.stdout.log'
            $hostErrorLog = Join-Path $modeDirectory 'host.stderr.log'
            $hostProcess = Start-Process -FilePath $hostBinary -WindowStyle Hidden -PassThru `
                -RedirectStandardOutput $hostLog -RedirectStandardError $hostErrorLog `
                -ArgumentList @('--data-dir', "`"$devData`"", '--minimized', '--desktop', "`"$desktopBinary`"")
            $session.host = @{ pid = $hostProcess.Id; path = $hostBinary;
                started = $hostProcess.StartTime.ToUniversalTime().Ticks }
            $session | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $sessionFile
            $deadline = [DateTime]::UtcNow.AddSeconds(30)
            $ready = $null
            do {
                if ($hostProcess.HasExited) { throw "Development host exited. See $hostErrorLog" }
                foreach ($line in @(Get-Content -LiteralPath $hostLog -ErrorAction SilentlyContinue)) {
                    if ($line -match '"ready":true') { $ready = $line | ConvertFrom-Json; break }
                }
                if (-not $ready) { Start-Sleep -Milliseconds 100 }
            } while (-not $ready -and [DateTime]::UtcNow -lt $deadline)
            if (-not $ready) { throw "Development host is not ready yet. See $hostErrorLog and retry." }
        }
        $uiArgs = if ($Demo) { @('--demo') } else { @('--endpoint', $ready.endpoint) }
        $uiProcess = Start-Process -FilePath $desktopBinary -ArgumentList $uiArgs -WindowStyle Normal -PassThru
        $session.desktop = @{ pid = $uiProcess.Id; path = $desktopBinary;
            started = $uiProcess.StartTime.ToUniversalTime().Ticks }
        $session | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $sessionFile
    }
} finally { Pop-Location }
