<#
Build Rain from a checkout and install the binary to a fixed path the skill calls.
  -Repo <dir>       : the Rain checkout (default: $env:RAIN_REPO)
  -Branch <name>    : refuse to build unless the checkout is on this branch
                      (default: the branch origin/HEAD points at, when known)
  -TargetDir <dir>  : CARGO_TARGET_DIR for the build (default: <Repo>/target)
  -InstallDir <dir> : where to install (default: Windows %LOCALAPPDATA%\Programs\Rain, else ~/.local/bin)
  -SkipBuild        : copy the last build without running cargo
  -From <file>      : install this binary instead of building (e.g. a worktree build)
Refuses to overwrite while an installed Rain is running (Windows locks the exe); close the window first.
Point RAIN_EXE at the installed binary if you change -InstallDir.
#>
param(
    [string]$Repo = $env:RAIN_REPO,
    [string]$Branch,
    [string]$TargetDir,
    [string]$InstallDir,
    [switch]$SkipBuild,
    [string]$From
)
$ErrorActionPreference = 'Stop'
$exeName = if ($IsWindows) { 'Rain.exe' } else { 'Rain' }
if (-not $InstallDir) {
    $InstallDir = if ($IsWindows) { Join-Path $env:LOCALAPPDATA 'Programs\Rain' } else { Join-Path $HOME '.local/bin' }
}
$dest = Join-Path $InstallDir $exeName

if ($Repo) { $Repo = (Resolve-Path $Repo).Path }
if (-not $From) {
    if (-not $Repo) { throw 'install-rain: pass -Repo <rain checkout> or set RAIN_REPO' }
    if (-not $TargetDir) { $TargetDir = Join-Path $Repo 'target' }
    if (-not $Branch) {
        $originHead = git -C $Repo symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>$null
        if ($LASTEXITCODE -eq 0 -and $originHead) { $Branch = $originHead -replace '^origin/', '' }
    }
    $current = git -C $Repo branch --show-current
    if ($Branch -and $current -ne $Branch) { throw "install-rain: $Repo is on '$current', not $Branch" }
    if (-not $SkipBuild) {
        $env:CARGO_TARGET_DIR = $TargetDir
        cargo build --release -p rain --manifest-path (Join-Path $Repo 'Cargo.toml')
        if ($LASTEXITCODE -ne 0) { throw "install-rain: cargo build failed ($LASTEXITCODE)" }
    }
    $From = Join-Path $TargetDir (Join-Path 'release' $exeName)
}
if (-not (Test-Path $From)) { throw "install-rain: no build at $From" }

$running = Get-Process -Name Rain -ErrorAction SilentlyContinue | Where-Object { $_.Path -and ($_.Path -ieq $dest) }
if ($running) { throw "install-rain: installed Rain is running (pid $($running.Id -join ', ')); close it ($exeName close --session <id>) and rerun" }

New-Item -ItemType Directory -Force $InstallDir | Out-Null
Copy-Item $From $dest -Force
$sha = if ($Repo) { git -C $Repo rev-parse --short HEAD } else { 'unknown' }
Set-Content (Join-Path $InstallDir 'installed-from.txt') "$sha  $From  $(Get-Date -Format s)" -Encoding utf8
Write-Output "installed $dest  ($sha)"
& $dest version
