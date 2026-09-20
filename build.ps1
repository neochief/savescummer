param(
    [Parameter(Position = 0)][ValidateSet('dev', 'release')][string]$Mode = 'dev',
    [switch]$Run,
    [switch]$Demo,
    [switch]$Package,
    [switch]$Test,
    [string]$QtPrefix = "$PSScriptRoot/.runtime/Qt/6.5.3/msvc2019_64",
    [string]$Generator = 'Visual Studio 16 2019'
)
$ErrorActionPreference = 'Stop'
if ($Run -and $Mode -ne 'dev') { throw '-Run is for development. Open the packaged executable for a release.' }
if ($Demo -and -not $Run) { throw '-Demo requires -Run.' }
$root = $PSScriptRoot
$modeDirectory = Join-Path $root "build/$Mode"
$desktopBuild = Join-Path $modeDirectory 'desktop'
$configuration = if ($Mode -eq 'dev') { 'RelWithDebInfo' } else { 'Release' }
$rustProfile = if ($Mode -eq 'dev') { 'debug' } else { 'release' }
$hostBinary = Join-Path $root "target/$rustProfile/savescummer-host.exe"
$cliBinary = Join-Path $root "target/$rustProfile/savescummer.exe"
$desktopBinary = Join-Path $desktopBuild "apps/desktop/$configuration/savescummer-desktop.exe"
$sessionFile = Join-Path $root 'build/dev/session.json'
$devData = Join-Path $root '.runtime/dev'
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

# Only stop processes launched and recorded by this dev runner. A normal app
# instance and untracked hosts are never terminated by a build.
if ($Mode -eq 'dev' -and (Test-Path -LiteralPath $sessionFile)) {
    $session = Get-Content -LiteralPath $sessionFile -Raw | ConvertFrom-Json
    foreach ($kind in @('desktop', 'host')) {
        $record = $session.$kind
        if (-not $record) { continue }
        $process = Get-Process -Id $record.pid -ErrorAction SilentlyContinue
        if (-not $process -or $process.StartTime.ToUniversalTime().Ticks -ne $record.started -or
            $process.Path -ne $record.path) { continue }
        if ($kind -eq 'desktop') {
            $null = $process.CloseMainWindow()
        } else {
            & $cliBinary --data-dir $devData shutdown
            if ($LASTEXITCODE -ne 0) { throw 'Could not stop the previous development host safely.' }
        }
        if (-not $process.WaitForExit(30000)) {
            throw 'Development process is still finishing work. Retry the build after it exits.'
        }
    }
    Remove-Item -LiteralPath $sessionFile
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:PATH = "$(Join-Path $env:USERPROFILE '.cargo/bin');$env:PATH"
}
Push-Location $root
try {
    Invoke-BuildStep 'Rust compilation' {
        $cargoArgs = @('build', '--manifest-path', (Join-Path $root 'Cargo.toml'),
            '--target-dir', (Join-Path $root 'target'), '-p', 'savescummer-host')
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
                $testTemp = Join-Path $root '.runtime/test-temp'
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
            & "$root/scripts/package-windows.ps1" -Mode $Mode -Configuration $configuration `
                -DesktopBuildDirectory $desktopBuild -HostBinary $hostBinary -CliBinary $cliBinary `
                -OutputDirectory (Join-Path $modeDirectory 'SaveScummer') -QtPrefix $QtPrefix
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
            # Dedicated settings/history and no OS integrations in the dev runner.
            # Configured game directories are still real: use -Demo for simulated operations.
            $hostLog = Join-Path $modeDirectory 'host.stdout.log'
            $hostErrorLog = Join-Path $modeDirectory 'host.stderr.log'
            $hostProcess = Start-Process -FilePath $hostBinary -WindowStyle Hidden -PassThru `
                -RedirectStandardOutput $hostLog -RedirectStandardError $hostErrorLog `
                -ArgumentList @('--data-dir', "`"$devData`"", '--no-integrations', '--minimized', '--desktop', "`"$desktopBinary`"")
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
