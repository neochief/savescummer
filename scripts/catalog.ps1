# Regenerate or verify catalog/catalog.json.
#
#   ./scripts/catalog.ps1          # regenerate the bundle and report
#   ./scripts/catalog.ps1 -Check   # fail when the committed bundle is stale
#   ./scripts/catalog.ps1 -Strict  # fail while any Keep row lacks a save dir
#
# The current Keep list still has rows without a verified save directory, so the
# default uses the builder's bootstrap mode; -Strict enforces the full contract.
param(
    [switch]$Check,
    [switch]$Strict
)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

& (Join-Path $PSScriptRoot 'fetch-catalog-manifest.ps1')

$arguments = @('run', '-q', '-p', 'savescummer-catalog-build', '--bin', 'catalog-gen', '--')
if (-not $Strict) { $arguments += '--allow-partial' }
if ($Check) { $arguments += '--check' }
if (-not $Check -and -not $Strict) { $arguments += @('--report', 'build/catalog/build-report.json') }
& cargo @arguments
exit $LASTEXITCODE