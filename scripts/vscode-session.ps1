param(
    [Parameter(Mandatory)][ValidateSet('prepare', 'demo', 'stop')][string]$Action
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path
$stateDirectory = Join-Path $root 'build/vscode'
$dataDirectory = Join-Path $root '.runtime/vscode'
$sessionFile = Join-Path $stateDirectory 'host.json'
$hostBinary = Join-Path $root 'target/debug/savescummer-host.exe'
$cliBinary = Join-Path $root 'target/debug/savescummer.exe'

# Stop only the host this task owns; PID + start time guard against PID reuse.
if (Test-Path -LiteralPath $sessionFile) {
    $record = Get-Content -LiteralPath $sessionFile -Raw | ConvertFrom-Json
    $previous = Get-Process -Id $record.pid -ErrorAction SilentlyContinue
    if ($previous -and $previous.Path -eq $hostBinary -and
        $previous.StartTime.ToUniversalTime().Ticks -eq $record.started) {
        & $cliBinary --data-dir $dataDirectory shutdown
        if ($LASTEXITCODE -ne 0) { throw 'Could not gracefully shut down the VS Code host.' }
        if (-not $previous.WaitForExit(30000)) {
            throw 'The VS Code host is still finishing work. Retry after it exits.'
        }
    }
    Remove-Item -LiteralPath $sessionFile
}
if ($Action -eq 'stop') { return }

& (Join-Path $root 'build.ps1') dev
if ($Action -eq 'demo') { return }

New-Item -ItemType Directory -Force -Path $stateDirectory, $dataDirectory | Out-Null
$stdout = Join-Path $stateDirectory 'host.stdout.log'
$stderr = Join-Path $stateDirectory 'host.stderr.log'
$desktopBinary = Join-Path $root 'build/dev/desktop/apps/desktop/RelWithDebInfo/savescummer-desktop.exe'
$hostProcess = Start-Process -FilePath $hostBinary -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput $stdout -RedirectStandardError $stderr `
    -ArgumentList @('--data-dir', "`"$dataDirectory`"", '--no-integrations', '--minimized',
        '--desktop', "`"$desktopBinary`"")
@{ pid = $hostProcess.Id; started = $hostProcess.StartTime.ToUniversalTime().Ticks } |
    ConvertTo-Json | Set-Content -LiteralPath $sessionFile
$deadline = [DateTime]::UtcNow.AddSeconds(30)
do {
    if ($hostProcess.HasExited) { throw "The VS Code host exited. See $stderr" }
    if (Select-String -LiteralPath $stdout -Pattern '"ready":true' -Quiet) {
        Write-Host 'VS Code host ready. Launching the Qt debugger.'
        return
    }
    Start-Sleep -Milliseconds 100
} while ([DateTime]::UtcNow -lt $deadline)
throw "The VS Code host is not ready. See $stderr. Run the stop-debug-host task to clean up."
