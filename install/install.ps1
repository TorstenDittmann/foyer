# Installs the Foyer CLI (`foyer.exe`) on Windows.
#
#   irm https://foyer.lol/install.ps1 | iex
#
# Settings:
#   $env:FOYER_VERSION       CLI version, default the newest release
#   $env:FOYER_INSTALL_DIR   where foyer.exe goes, default ~\.foyer\bin
#   $env:FOYER_REPO          GitHub repository, default TorstenDittmann/foyer
#   $env:GITHUB_TOKEN        optional, avoids GitHub API rate limits in CI
#
# The archive is checked against the release's SHA256SUMS before anything is
# installed. The install directory is added to the user's PATH, and in
# GitHub Actions to GITHUB_PATH.

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = if ($env:FOYER_REPO) { $env:FOYER_REPO } else { 'TorstenDittmann/foyer' }
$dir = if ($env:FOYER_INSTALL_DIR) { $env:FOYER_INSTALL_DIR } else { Join-Path $HOME '.foyer\bin' }

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
# Windows on Arm runs the x64 build through emulation.
if ($arch -ne 'X64' -and $arch -ne 'Arm64') { throw "unsupported CPU $arch" }
$target = 'x86_64-pc-windows-msvc'

if ($env:FOYER_VERSION) {
    $tag = 'cli-v' + $env:FOYER_VERSION.TrimStart('v')
} else {
    $headers = @{ 'User-Agent' = 'foyer-install' }
    if ($env:GITHUB_TOKEN) { $headers['Authorization'] = "Bearer $env:GITHUB_TOKEN" }
    # Desktop releases share the repository, so pick the newest CLI tag.
    $releases = Invoke-RestMethod -Headers $headers "https://api.github.com/repos/$repo/releases?per_page=50"
    $tag = ($releases | Where-Object { $_.tag_name -like 'cli-v*' -and -not $_.draft } | Select-Object -First 1).tag_name
    if (-not $tag) { throw "could not find a CLI release of $repo" }
}

$base = "https://github.com/$repo/releases/download/$tag"
$archive = "foyer-$target.zip"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("foyer-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "Downloading foyer $($tag.Substring(5)) for $target"
    Invoke-WebRequest -UseBasicParsing "$base/$archive" -OutFile (Join-Path $tmp $archive)
    Invoke-WebRequest -UseBasicParsing "$base/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')

    $line = Get-Content (Join-Path $tmp 'SHA256SUMS') | Where-Object { $_ -match " $([regex]::Escape($archive))$" } | Select-Object -First 1
    if (-not $line) { throw "$archive is not listed in SHA256SUMS" }
    $expected = ($line -split '\s+')[0]
    $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $tmp $archive)).Hash
    if ($expected -ne $actual.ToLower()) { throw "checksum mismatch for $archive" }

    Expand-Archive -Path (Join-Path $tmp $archive) -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    Move-Item -Force (Join-Path $tmp 'foyer.exe') (Join-Path $dir 'foyer.exe')
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$exe = Join-Path $dir 'foyer.exe'
Write-Host "Installed $(& $exe --version) to $exe"

if ($env:GITHUB_PATH) {
    Add-Content -Path $env:GITHUB_PATH -Value $dir
} else {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($userPath -split ';') -contains $dir)) {
        [Environment]::SetEnvironmentVariable('Path', "$dir;$userPath", 'User')
        Write-Host "Added $dir to your PATH. Open a new terminal to use foyer."
    }
}
