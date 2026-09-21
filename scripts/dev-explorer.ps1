param(
    [switch]$Stop,
    # Build and exercise the real COM handler without registering or restarting Explorer.
    [switch]$CheckOnly,
    [string]$Generator = 'Visual Studio 16 2019'
)
$ErrorActionPreference = 'Stop'
if ($Stop -and $CheckOnly) { throw '-Stop and -CheckOnly are mutually exclusive.' }
$root = (Resolve-Path "$PSScriptRoot/..").Path
$buildDirectory = Join-Path $root 'build/explorer-dev'
$runtimeDirectory = Join-Path $root '.runtime/explorer-dev'
$sessionFile = Join-Path $runtimeDirectory 'session.json'
$devClassKey = 'HKCU:\Software\Classes\CLSID\{43BFBA41-D0AB-44D3-A5D6-600EB5C74D18}\InprocServer32'
$registrationScript = Join-Path $PSScriptRoot 'register-explorer.ps1'

function Stop-OwnedHost($record) {
    if (-not $record) { return }
    $process = Get-Process -Id $record.pid -ErrorAction SilentlyContinue
    if ($process -and $process.Path -eq $record.hostPath -and
        $process.StartTime.ToUniversalTime().Ticks -eq $record.started) {
        & $record.cliPath --no-start --data-dir $record.dataDirectory shutdown | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Could not gracefully stop the Explorer dev host.' }
        if (-not $process.WaitForExit(30000)) {
            throw 'The Explorer dev host is still finishing work. Retry once it exits.'
        }
    }
}

function Get-OwnedRegistration {
    if (-not (Test-Path -LiteralPath $devClassKey)) { return $null }
    $dll = (Get-Item -LiteralPath $devClassKey).GetValue('')
    $expectedPrefix = [IO.Path]::GetFullPath((Join-Path $buildDirectory 'sessions')) + [IO.Path]::DirectorySeparatorChar
    if (-not $dll -or -not [IO.Path]::GetFullPath($dll).StartsWith($expectedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Another checkout owns the Explorer dev registration. Run its dev-explorer.ps1 -Stop first.'
    }
    return $dll
}

function Restart-Explorer {
    Write-Host 'Restarting Explorer to refresh the extension (open Explorer windows will close).'
    $sessionId = (Get-Process -Id $PID).SessionId
    $explorerPath = Join-Path $env:WINDIR 'explorer.exe'
    # Never stop a different Windows session or a process merely sharing the name.
    foreach ($process in @(Get-Process -Name explorer -ErrorAction SilentlyContinue)) {
        if ($process.SessionId -eq $sessionId -and $process.Path -eq $explorerPath) {
            Stop-Process -InputObject $process -Force -ErrorAction Stop
            $null = $process.WaitForExit(10000)
        }
    }
    Start-Process -FilePath $explorerPath -WindowStyle Hidden
    Start-Sleep -Seconds 2
}

function Invoke-DevCli([string[]]$Arguments) {
    & $record.cliPath --no-start --data-dir $record.dataDirectory @Arguments | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Explorer dev CLI failed: $($Arguments -join ' ')" }
}

$mutex = [Threading.Mutex]::new($false, 'Local\SaveScummerExplorerDev')
$locked = $false
$record = $null
$registered = $false
$keepHost = $false
try {
    try { $locked = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $locked = $true }
    if (-not $locked) { throw 'Another Explorer dev command is running.' }
    if (-not $CheckOnly) {
        $previousDll = Get-OwnedRegistration
        if (Test-Path -LiteralPath $sessionFile) {
            Stop-OwnedHost (Get-Content -LiteralPath $sessionFile -Raw | ConvertFrom-Json)
            Remove-Item -LiteralPath $sessionFile
        }
        if ($Stop) {
            if ($previousDll) {
                & $registrationScript -Dev -Uninstall
                Restart-Explorer
            }
            Write-Host 'Explorer development integration stopped. Test files and logs are retained.'
            return
        }
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        $env:PATH = "$(Join-Path $env:USERPROFILE '.cargo/bin');$env:PATH"
    }
    $runId = [guid]::NewGuid().ToString('N')
    $runDirectory = Join-Path $runtimeDirectory "sessions/$runId"
    $dataDirectory = Join-Path $runDirectory 'data'
    $saveDirectory = Join-Path $runDirectory 'Test game/Saves'
    $binaryDirectory = Join-Path $buildDirectory "sessions/$runId"
    New-Item -ItemType Directory -Force -Path $dataDirectory, $saveDirectory, $binaryDirectory | Out-Null
    Set-Content -LiteralPath (Join-Path $saveDirectory 'progress.txt') -Value 'before' -Encoding ascii

    Push-Location $root
    try {
        Write-Host 'Building the development host and CLI...'
        & cargo build --locked -p savescummer-host --bins --target-dir (Join-Path $buildDirectory 'host')
        if ($LASTEXITCODE -ne 0) { throw 'Host build failed.' }
        & (Join-Path $PSScriptRoot 'build-explorer.ps1') -BuildDirectory (Join-Path $buildDirectory 'extension') `
            -Generator $Generator -DevDataDirectory $dataDirectory
        if ($LASTEXITCODE -ne 0) { throw 'Explorer build or COM tests failed.' }
    } finally { Pop-Location }

    # Register staged copies: Explorer can keep an old DLL loaded without locking
    # the next build output. Host copies likewise avoid executable linker locks.
    foreach ($name in @('savescummer-host.exe', 'savescummer.exe')) {
        Copy-Item -LiteralPath (Join-Path $buildDirectory "host/debug/$name") -Destination $binaryDirectory
    }
    $dll = Join-Path $binaryDirectory 'savescummer-explorer.dll'
    Copy-Item -LiteralPath (Join-Path $buildDirectory 'extension/Release/savescummer-explorer.dll') -Destination $dll
    $stdout = Join-Path $runDirectory 'host.stdout.log'
    $stderr = Join-Path $runDirectory 'host.stderr.log'
    $hostPath = Join-Path $binaryDirectory 'savescummer-host.exe'
    $hostProcess = Start-Process -FilePath $hostPath -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr `
        -ArgumentList @('--data-dir', "`"$dataDirectory`"", '--no-scan', '--no-monitor',
            '--no-artwork', '--no-audio', '--no-integrations')
    $record = @{ pid = $hostProcess.Id; started = $hostProcess.StartTime.ToUniversalTime().Ticks;
        hostPath = $hostPath; cliPath = (Join-Path $binaryDirectory 'savescummer.exe');
        dataDirectory = $dataDirectory; saveDirectory = $saveDirectory; dll = $dll }
    if (-not $CheckOnly) { $record | ConvertTo-Json | Set-Content -LiteralPath $sessionFile }
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Select-String -LiteralPath $stdout -Pattern '"ready":true' -Quiet)) {
        if ($hostProcess.HasExited) { throw "Host exited. See $stderr" }
        if ([DateTime]::UtcNow -ge $deadline) { throw "Host readiness timed out. See $stderr" }
        Start-Sleep -Milliseconds 100
    }
    # An existing executable is sufficient; process monitoring is disabled.
    Invoke-DevCli -Arguments @('configure', 'explorer-dev', '--name', 'Explorer dev test', '--dir', $saveDirectory, '--exe', $record.cliPath)
    & (Join-Path $buildDirectory 'extension/Release/explorer-smoke.exe') $dll $saveDirectory
    if ($LASTEXITCODE -ne 0) { throw "Explorer Save/Load smoke test failed. See $runDirectory" }

    if ($CheckOnly) {
        Write-Host "Check passed without registration or Explorer restart. Logs and test files: $runDirectory"
    } else {
        # Include partially written registration in failure cleanup.
        $registered = $true
        & $registrationScript -Dev -Dll $dll
        Restart-Explorer
        Invoke-Item -LiteralPath (Split-Path $saveDirectory)
        $keepHost = $true
        Write-Host "Ready: right-click Saves or Saves - Copy > Show more options > Save (dev) / Load (dev)."
        Write-Host "Test data and logs: $runDirectory"
        Write-Host 'Stop and unregister with: ./scripts/dev-explorer.ps1 -Stop'
    }
} finally {
    try {
        if (-not $keepHost -and $record) {
            Stop-OwnedHost $record
            if ($registered) { & $registrationScript -Dev -Uninstall }
            if (-not $CheckOnly -and (Test-Path -LiteralPath $sessionFile)) {
                Remove-Item -LiteralPath $sessionFile
            }
        }
    } finally {
        if ($locked) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
}
