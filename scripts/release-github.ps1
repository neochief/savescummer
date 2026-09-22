# release-github.ps1 — publishes the current release to GitHub as a DRAFT for
# human review, draft-first. Requires the GitHub CLI (gh), an authenticated
# account, and a pushed tag matching the Cargo.toml version.
#
#   git tag v0.1.0 && git push origin v0.1.0
#   ./build.ps1 release          # easy path: produces the ZIP and the installer
#   ./scripts/release-github.ps1 # creates/updates the DRAFT release
#
# The installer is attached by default; pass -AllowMissingInstaller to publish a
# portable-only release when Inno Setup was unavailable.
#
# Re-running after a rebuild uploads fresh assets to the existing draft with
# --clobber. A release that has already been published is never modified.
param(
    # Publish without the installer when Inno Setup was unavailable.
    [switch]$AllowMissingInstaller
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot/..").Path
. (Join-Path $PSScriptRoot 'package-common.ps1')

$version = Get-AppVersion -CargoManifest (Join-Path $root 'Cargo.toml')
$tag = "v$version"

# --- preconditions ----------------------------------------------------------
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw 'The GitHub CLI (gh) is required. Install it from https://cli.github.com.'
}
$origin = git remote get-url origin 2>$null
if ($LASTEXITCODE -ne 0 -or -not $origin) {
    throw 'No origin remote is configured; cannot publish to GitHub.'
}
gh auth status *> $null
if ($LASTEXITCODE -ne 0) {
    throw 'gh is not authenticated. Run: gh auth login'
}
if (-not (git rev-parse -q --verify "refs/tags/$tag")) {
    throw "Tag $tag does not exist. Run: git tag $tag  then: git push origin $tag"
}
if ((git rev-parse HEAD) -ne (git rev-list -n 1 $tag)) {
    throw "Tag $tag does not point at HEAD. Release the tagged commit, not local uncommitted changes."
}

# --- artifacts --------------------------------------------------------------
$archive = Join-Path $root "dist/SaveScummer-windows-x64-$version.zip"
if (-not (Test-Path -LiteralPath $archive)) {
    throw "Release archive not found: $archive. Build it first with: ./build.ps1 release"
}
$installer = Join-Path $root ('dist/' + (Get-InstallerArtifactName -Os 'windows' -Arch 'x64' -Version $version))
$installerExists = Test-Path -LiteralPath $installer
if (-not $installerExists -and -not $AllowMissingInstaller) {
    throw "Installer not found: $installer. Build it first with: ./build.ps1 release, or pass -AllowMissingInstaller for a portable-only release."
}
if (-not $installerExists) {
    Write-Warning "Publishing without the installer: $installer is missing."
}
$artifacts = @($archive)
if ($installerExists) { $artifacts += $installer }
$assets = @()
foreach ($artifact in $artifacts) {
    $shaFile = "$artifact.sha256"
    if (-not (Test-Path -LiteralPath $shaFile)) {
        (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant() |
            Set-Content -LiteralPath $shaFile -Encoding ascii
    }
    $assets += (Get-Item -LiteralPath $artifact).FullName
    $assets += (Get-Item -LiteralPath $shaFile).FullName
}

# --- publish (draft-first) --------------------------------------------------
$existing = gh release view $tag --json isDraft,url 2>$null
if ($LASTEXITCODE -eq 0 -and $existing) {
    $release = $existing | ConvertFrom-Json
    if (-not $release.isDraft) {
        throw "Release $tag already exists and is published. Refusing to modify it."
    }
    Write-Host "Updating the existing draft release $tag."
    gh release upload $tag $assets --clobber
    if ($LASTEXITCODE -ne 0) { throw 'Uploading assets to the draft release failed.' }
} else {
    Write-Host "Creating draft release $tag."
    gh release create $tag $assets --draft --generate-notes --title "SaveScummer $version"
    if ($LASTEXITCODE -ne 0) { throw 'Creating the draft release failed.' }
}
$info = gh release view $tag --json url | ConvertFrom-Json
Write-Host "Draft release ready: $($info.url)" -ForegroundColor Green
