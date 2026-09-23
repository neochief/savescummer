# release.ps1 — cuts a release in one command.
#
#   ./scripts/release.ps1 0.2.0
#   ./scripts/release.ps1 0.2.0 -SkipChecks   # skip scripts/check.ps1
#   ./scripts/release.ps1 0.2.0 -NoPush       # bump, commit and tag locally only
#
# The script bumps the single-source version in Cargo.toml, refreshes
# Cargo.lock, optionally runs the full checks, commits "Release <version>",
# creates the annotated tag v<version> and pushes both. The tag starts
# .github/workflows/release.yml, which builds the distribution and creates the
# DRAFT release; publishing it stays a manual GitHub action.
param(
    [Parameter(Mandatory = $true, Position = 0)][string]$Version,
    [switch]$NoPush,
    [switch]$SkipChecks
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path
. (Join-Path $PSScriptRoot 'package-common.ps1')

$Version = $Version.TrimStart('vV')
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Pass a three-part version such as 0.2.0 (got '$Version')."
}
$tag = "v$Version"
$manifestPath = Join-Path $root 'Cargo.toml'

Push-Location $root
$committed = $false
try {
    if (-not (Test-Path -LiteralPath (Join-Path $root '.git'))) { throw 'Not inside a Git repository.' }
    $branch = "$(& git branch --show-current)".Trim()
    if (-not $branch) { throw 'Detached HEAD: check out a branch before releasing.' }
    $dirty = @(& git status --porcelain)
    if ($dirty.Count -gt 0) {
        throw "The working tree is not clean. Commit or stash these first:`n  $($dirty -join "`n  ")"
    }
    $current = Get-AppVersion -CargoManifest $manifestPath
    if ($current -eq $Version) { throw "Cargo.toml is already at $Version." }
    if ([version]$Version -lt [version]$current) {
        Write-Warning "Version $Version is lower than the current $current; continuing as requested."
    }
    if (& git rev-parse -q --verify "refs/tags/$tag") {
        throw "Tag $tag exists locally. Delete it first, or rebuild the release with the Release workflow's manual dispatch."
    }
    if (-not $NoPush) {
        $remote = & git ls-remote --tags origin $tag 2>$null
        if ($LASTEXITCODE -eq 0 -and $remote) {
            throw "Tag $tag already exists on origin. Rebuild the release with the Release workflow's manual dispatch, or delete the remote tag first."
        }
    }

    Write-Host "Cutting release $Version from $branch (was $current)." -ForegroundColor Cyan
    $manifest = Get-Content -LiteralPath $manifestPath -Raw
    $versionField = [regex]::new('(?m)^version\s*=\s*"\d+\.\d+\.\d+"')
    if (-not $versionField.IsMatch($manifest)) { throw 'Cannot find the workspace version in Cargo.toml.' }
    Set-Content -LiteralPath $manifestPath -NoNewline `
        -Value $versionField.Replace($manifest, "version = `"$Version`"", 1)

    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        $env:PATH = "$(Join-Path $env:USERPROFILE '.cargo/bin');$env:PATH"
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'cargo is required to refresh Cargo.lock.' }
    Write-Host 'Refreshing Cargo.lock for the new workspace version.'
    & cargo update --workspace --manifest-path $manifestPath
    if ($LASTEXITCODE -ne 0) { throw 'cargo update --workspace failed.' }

    if (-not $SkipChecks) {
        Write-Host 'Running scripts/check.ps1.'
        & (Join-Path $PSScriptRoot 'check.ps1')
        if ($LASTEXITCODE -ne 0) { throw 'scripts/check.ps1 failed; the version bump was reverted.' }
    }

    & git add -- Cargo.toml Cargo.lock
    & git commit -m "Release $Version"
    if ($LASTEXITCODE -ne 0) { throw 'git commit failed.' }
    $committed = $true
    & git tag -a $tag -m "SaveScummer $Version"
    if ($LASTEXITCODE -ne 0) { throw "git tag $tag failed; the release commit is local." }

    if ($NoPush) {
        Write-Host "Created the release commit and tag $tag locally. Push them with:" -ForegroundColor Green
        Write-Host "  git push origin $branch"
        Write-Host "  git push origin $tag"
        return
    }
    & git push origin $branch
    if ($LASTEXITCODE -ne 0) { throw "Pushing $branch failed; the release commit and tag are local." }
    & git push origin $tag
    if ($LASTEXITCODE -ne 0) { throw "Pushing $tag failed; send it with: git push origin $tag" }

    Write-Host "Release $Version pushed." -ForegroundColor Green
    Write-Host 'The Release workflow builds the distribution and creates a DRAFT release now.'
    $origin = "$(& git remote get-url origin)".Trim()
    if ($origin -match 'github\.com[:/](?<slug>[^/]+/[^/]+?)(?:\.git)?$') {
        $slug = $Matches['slug']
        Write-Host "  Runs:  https://github.com/$slug/actions/workflows/release.yml"
        Write-Host "  Draft: https://github.com/$slug/releases"
    }
    Write-Host '  Review the draft and publish it when the assets look right.'
} catch {
    if (-not $committed) {
        & git reset -q -- Cargo.toml Cargo.lock 2>$null
        & git checkout -- Cargo.toml Cargo.lock 2>$null
    }
    throw
} finally {
    Pop-Location
}