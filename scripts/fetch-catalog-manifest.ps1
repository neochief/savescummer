# Fetch and verify the pinned Ludusavi manifest into .runtime/catalog.
# The manifest is intentionally not committed; manifest.lock pins the revision
# and sha256, and the builder runs against the verified local copy.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$lock = Get-Content 'catalog/manifest.lock' -Raw
$revision = [regex]::Match($lock, 'revision:\s*(\S+)').Groups[1].Value
$sha = [regex]::Match($lock, 'sha256:\s*(\S+)').Groups[1].Value.ToLower()
if (-not $revision -or -not $sha) { throw 'catalog/manifest.lock is missing revision or sha256' }

$destination = '.runtime/catalog/manifest.yaml'
if (Test-Path -LiteralPath $destination) {
    $existing = (Get-FileHash $destination -Algorithm SHA256).Hash.ToLower()
    if ($existing -eq $sha) {
        Write-Output "manifest current ($revision)"
        exit 0
    }
}

New-Item -ItemType Directory -Force -Path '.runtime/catalog' | Out-Null
$url = "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/$revision/data/manifest.yaml"
Write-Output "fetching $url"
Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $destination -TimeoutSec 300
$actual = (Get-FileHash $destination -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $sha) {
    throw "manifest sha256 mismatch: expected $sha, got $actual"
}
Write-Output "manifest verified ($revision)"