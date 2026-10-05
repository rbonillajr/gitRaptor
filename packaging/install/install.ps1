# GitRaptor installer for Windows (INF-GRP-004, ADR-GRP-014 § 5).
#
#   irm https://github.com/rbonillajr/gitRaptor/releases/latest/download/install.ps1 | iex
#   ./install.ps1 -Version 0.1.0
#
# Downloads the archive for this machine and SHA256SUMS from the GitHub Release, verifies the
# checksum and only then installs raptor.exe and raptor-mcp.exe into
# %LOCALAPPDATA%\Programs\GitRaptor\bin (PQ-7). It never edits PATH, never registers autostart
# and never touches GitRaptor's data.
#
# Environment:
#   RAPTOR_VERSION        version to install (default: the latest release)
#   RAPTOR_INSTALL_DIR    destination directory
#   RAPTOR_DOWNLOAD_BASE  base URL, or a local directory, that holds the archives and SHA256SUMS
#
# SHA256SUMS comes from the same origin as the archive: it proves integrity, not authenticity.
# For authenticity, verify the archive with `gh attestation verify <archive> --repo rbonillajr/gitRaptor`.
[CmdletBinding()]
param(
    [string]$Version = $env:RAPTOR_VERSION,
    [string]$InstallDir = $env:RAPTOR_INSTALL_DIR
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Repo = 'rbonillajr/gitRaptor'

function Say([string]$Message) { Write-Host "raptor-install: $Message" }

if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\GitRaptor\bin' }

$osArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ($osArch.ToString()) {
    'X64' { $cpu = 'x86_64' }
    'Arm64' { $cpu = 'aarch64' }
    default { throw "raptor-install: unsupported CPU architecture: $osArch" }
}
$target = "$cpu-pc-windows-msvc"

if (-not $Version) {
    # The latest release redirects to .../releases/tag/v<version>; no API token needed.
    $request = [System.Net.HttpWebRequest]::Create("https://github.com/$Repo/releases/latest")
    $request.AllowAutoRedirect = $false
    $response = $request.GetResponse()
    $location = $response.Headers['Location']
    $response.Close()
    if ($location -notmatch '/releases/tag/v(.+)$') { throw 'raptor-install: could not resolve the latest release; pass -Version' }
    $Version = $Matches[1]
}
$Version = $Version.TrimStart('v')

$base = $env:RAPTOR_DOWNLOAD_BASE
if (-not $base) { $base = "https://github.com/$Repo/releases/download/v$Version" }
$archive = "raptor-$Version-$target.zip"

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("raptor-install-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    function Fetch([string]$Name) {
        $dest = Join-Path $tmp $Name
        if ($base -match '^https://') {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$Name" -OutFile $dest
        } elseif ($base -match '^[a-z]+://') {
            throw "raptor-install: only https:// or a local directory is accepted as download base"
        } else {
            Copy-Item -LiteralPath (Join-Path $base $Name) -Destination $dest
        }
        return $dest
    }

    Say "downloading $archive"
    $archivePath = Fetch $archive
    $sumsPath = Fetch 'SHA256SUMS'

    $expected = $null
    foreach ($line in Get-Content -LiteralPath $sumsPath) {
        $parts = $line -split '\s+', 2
        if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $archive) { $expected = $parts[0].ToLowerInvariant() }
    }
    if (-not $expected) { throw "raptor-install: $archive is not listed in SHA256SUMS" }
    $actual = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expected -ne $actual) {
        throw "raptor-install: checksum mismatch for $archive (expected $expected, got $actual); nothing was installed"
    }
    Say "checksum verified (sha256 $actual)"

    Expand-Archive -LiteralPath $archivePath -DestinationPath $tmp
    $src = Join-Path $tmp "raptor-$Version-$target"
    foreach ($bin in 'raptor.exe', 'raptor-mcp.exe') {
        if (-not (Test-Path -LiteralPath (Join-Path $src $bin))) { throw 'raptor-install: unexpected archive layout' }
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($bin in 'raptor.exe', 'raptor-mcp.exe') {
        $dest = Join-Path $InstallDir $bin
        if (Test-Path -LiteralPath $dest) {
            # A running exe (the engine is `raptor daemon`) cannot be overwritten but can be
            # renamed; the old copy is removed on the next install.
            $old = "$dest.old"
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
            Move-Item -LiteralPath $dest -Destination $old -Force
        }
        Copy-Item -LiteralPath (Join-Path $src $bin) -Destination $dest
    }
    Say "installed raptor $Version ($target) into $InstallDir"

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($userPath -split ';') -contains $InstallDir)) {
        Say "note: $InstallDir is not on your PATH; add it to run 'raptor'"
    }
} finally {
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
