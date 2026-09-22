# package-common.ps1 — platform-neutral packaging core shared by every
# package-<os>.ps1 script (package-windows.ps1 today; macos/linux later).
# Nothing here may call Windows-only APIs; keep it usable on any OS where pwsh
# runs. Dot-source it:  . (Join-Path $PSScriptRoot 'package-common.ps1')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Reads the application version from the Cargo workspace manifest
# ([workspace.package] version). It is the first three-part version field in
# the file; dependency versions like "1" or "0.37" do not match.
function Get-AppVersion {
    param([string]$CargoManifest)
    $manifest = Get-Content -LiteralPath $CargoManifest -Raw
    if ($manifest -notmatch '(?m)^\s*version\s*=\s*"(\d+\.\d+\.\d+)"') {
        throw 'Cannot read the application version from Cargo.toml.'
    }
    return $Matches[1]
}

# Returns the OS/arch-tagged portable artifact name:
# SaveScummer-<os>-<arch>-<version>[-<suffix>].<extension>
function Get-PortableArtifactName {
    param(
        [string]$Os,
        [string]$Arch,
        [string]$Version,
        [string]$Suffix,
        [string]$Extension = 'zip'
    )
    $stem = "SaveScummer-$Os-$Arch-$Version"
    if ($Suffix) { $stem += "-$Suffix" }
    return "$stem.$Extension"
}

# Creates a fresh sibling staging directory for an atomic package replace and
# returns its path. A failed packaging run therefore never disturbs the
# existing package or archive.
function New-PackageStage {
    param([string]$PackageDirectory)
    $parent = Split-Path -Parent $PackageDirectory
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    return Join-Path $parent ('.staging-' + [guid]::NewGuid().ToString('N'))
}

# Writes the package manifest marker (.savescummer-package.json) that identifies
# a folder as SaveScummer-generated output.
function Write-PackageManifest {
    param(
        [string]$PackageDirectory,
        [hashtable]$Fields
    )
    $Fields | ConvertTo-Json -Depth 4 |
        Set-Content -LiteralPath (Join-Path $PackageDirectory '.savescummer-package.json')
}

# Writes SHA256SUMS.txt for every file under $PackageDirectory, relative to it.
function Write-Sha256Sums {
    param([string]$PackageDirectory)
    Get-ChildItem -LiteralPath $PackageDirectory -Recurse -File | ForEach-Object {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(),
            $_.FullName.Substring($PackageDirectory.Length + 1)
    } | Set-Content -LiteralPath (Join-Path $PackageDirectory 'SHA256SUMS.txt') -Encoding ascii
}

# Atomically installs a staged portable package. $Stage holds a single
# top-level folder named $PortableFolder that becomes the final $Package
# directory; the folder is archived first (so the archive matches the package
# exactly) and then moved into place. $PreReplace, when supplied, stops running
# packaged processes before the old folder is replaced.
function Set-PortablePackage {
    param(
        [string]$Package,
        [string]$Stage,
        [string]$PortableFolder,
        [string]$Archive,
        [scriptblock]$PreReplace
    )
    if ($PreReplace) { & $PreReplace }
    if (Test-Path -LiteralPath $Package) { Remove-Item -LiteralPath $Package -Recurse -Force }
    $temporaryZip = $Archive + '.' + [guid]::NewGuid().ToString('N') + '.zip'
    try {
        # Compress-Archive stores the directory itself as the archive root entry,
        # which yields the required single top-level folder inside the archive.
        Compress-Archive -Path (Join-Path $Stage $PortableFolder) -DestinationPath $temporaryZip
        Move-Item -LiteralPath (Join-Path $Stage $PortableFolder) -Destination $Package
        Move-Item -LiteralPath $temporaryZip -Destination $Archive -Force
    } finally {
        if (Test-Path -LiteralPath $temporaryZip) { Remove-Item -LiteralPath $temporaryZip }
    }
}
