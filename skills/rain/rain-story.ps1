<#
Open / read back / close Rain for every repo of a story: ONE window per -Session over all its repos.
  open  : diff = <Target> vs merge-base(<Target>, <Base>) (or uncommitted), optional per-repo comment files
          and overview. Reopening the same session reuses the window; Rain skips comments it already has
          (same file, side, line and body), so reopening is safe.
          Repos: -Repos <path>,<path>, or -Key <KEY> to scan the git repos directly under -Root
          (default $env:RAIN_REPOS_ROOT, else the current folder) for a checked-out branch containing the key.
  get   : the session's threads, ALL states (open, addressed, resolved, dismissed) -> <OutDir>/rain-comments.json
  close : save and close this session's window only.
Comment files: <CommentsDir>/<repo-folder>.json, a JSON array of thread imports with repo-relative filePath.
Rain binary: $env:RAIN_EXE, else the per-OS install path of install-rain.ps1, else Rain on PATH.
#>
param(
    [Parameter(Mandatory)] [ValidateSet('open', 'get', 'close')] [string]$Mode,
    [Parameter(Mandatory)] [string]$Session,
    [string[]]$Repos,
    [string]$Key,
    [string]$Root = $env:RAIN_REPOS_ROOT,
    [string]$Base = 'origin/main',
    [string]$Target = 'HEAD',
    [string]$CommentsDir,
    [string]$OutDir,
    [string]$Overview,
    [switch]$Uncommitted
)
$ErrorActionPreference = 'Stop'

function Resolve-RainExe {
    if ($env:RAIN_EXE) { return $env:RAIN_EXE }
    $default = if ($IsWindows) { Join-Path $env:LOCALAPPDATA 'Programs\Rain\Rain.exe' } else { Join-Path $HOME '.local/bin/Rain' }
    if (Test-Path $default) { return $default }
    $onPath = Get-Command Rain -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($onPath) { return $onPath.Source }
    return $default
}
$rain = Resolve-RainExe
if (-not (Test-Path $rain)) { throw "rain-story: Rain not found at $rain (set RAIN_EXE or run install-rain.ps1)" }

# The checked-out branch read straight from HEAD (no git process per folder); $null when not a repo.
function Get-Branch([string]$dir) {
    $git = Join-Path $dir '.git'
    $gitDir = $git
    if ([IO.File]::Exists($git)) {
        $line = [IO.File]::ReadLines($git) | Select-Object -First 1
        if ($line -notmatch '^gitdir:\s*(.+)$') { return $null }
        $gitDir = $Matches[1].Trim()
        if (-not [IO.Path]::IsPathRooted($gitDir)) { $gitDir = Join-Path $dir $gitDir }
    }
    elseif (-not [IO.Directory]::Exists($git)) { return $null }
    $head = Join-Path $gitDir 'HEAD'
    if (-not [IO.File]::Exists($head)) { return $null }
    $ref = [IO.File]::ReadAllText($head).Trim()
    if ($ref -match '^ref:\s*refs/heads/(.+)$') { return $Matches[1] }
    return ''
}

# The last line of a command's output that is a JSON object, parsed; $null when there is none.
function Get-JsonLine([string[]]$lines) {
    $json = $lines | Where-Object { $_ -match '^\s*\{' } | Select-Object -Last 1
    if ($json) { $json | ConvertFrom-Json }
}

function Get-RepoId($folder, $doc, [string[]]$known) {
    if ($doc) {
        $id = ($doc.repos | Where-Object { $_.dir -ieq $folder } | Select-Object -First 1).id
        if (-not $id) { $id = ($doc.repos | Where-Object { $folder -like "*$($_.dir)*" } | Select-Object -First 1).id }
        if ($id) { return $id }
    }
    if ($known -contains $folder) { return $folder }
    if ($known.Count -eq 1) { return $known[0] }
    return $folder
}

if ($Mode -eq 'open') {
    if (-not $Repos) {
        if (-not $Key) { throw 'rain-story: pass -Repos or -Key' }
        if (-not $Root) { $Root = (Get-Location).Path }
        $Repos = Get-ChildItem $Root -Directory |
            Where-Object { (Get-Branch $_.FullName) -like "*$Key*" } |
            ForEach-Object { $_.FullName }
        if (-not $Repos) { throw "rain-story: no repo under $Root has a *$Key* branch checked out; pass -Root or -Repos" }
    }
    $Repos = @($Repos | ForEach-Object { $_ -split ',' } | Where-Object { $_ } | ForEach-Object { (Resolve-Path $_.Trim()).Path })
    $doc = $null
    $absOverview = $null
    if ($Overview) {
        $absOverview = (Resolve-Path $Overview).Path
        $doc = Get-Content $absOverview -Raw | ConvertFrom-Json
    }

    $openArgs = @('open')
    $openArgs += if ($Uncommitted) { @('.', '--include-untracked') } else { @($Target, $Base, '--merge-base') }
    foreach ($r in $Repos) { $openArgs += @('--repo', $r) }
    $openArgs += @('--session', $Session, '--background')
    if ($absOverview) { $openArgs += @('--overview', $absOverview) }
    $raw = @(& $rain @openArgs 2>&1 | ForEach-Object { "$_" })
    $info = Get-JsonLine ($raw | Where-Object { $_ -match '"port"' })
    if ($LASTEXITCODE -ne 0 -or -not $info) { throw "rain-story: open failed: $($raw -join ' ')" }
    $state = if ($info.readOnly) { 'read-only' } elseif ($info.reused) { 'reused' } else { 'opened' }
    # Lost an open race: this window refuses changes (423); --session calls reach the holder.
    if ($info.readOnly) { Write-Output "warning: another Rain window already holds session $Session; the one just opened (pid $($info.pid)) is read-only. Calls below use --session and reach the holder; close the extra one with: rain close --port $($info.port)" }

    # A new window takes a moment to load its repos; poll up to ~10 s, tightly at first.
    $known = @()
    for ($i = 0; $i -lt 60 -and -not $known; $i++) {
        $h = Get-JsonLine @(& $rain health --session $Session 2>$null)
        if ($LASTEXITCODE -eq 0 -and $h) { $known = @($h.repos) }
        if (-not $known) { Start-Sleep -Milliseconds ([Math]::Min(50 * ($i + 1), 250)) }
    }
    if (-not $known) { Write-Output "warning: session $Session didn't report its repos within ~10 s; comment repo ids fall back to folder names" }

    if ($CommentsDir) {
        foreach ($r in $Repos) {
            $folder = Split-Path $r -Leaf
            $file = Join-Path $CommentsDir "$folder.json"
            if (-not (Test-Path $file)) { continue }
            $id = Get-RepoId $folder $doc $known
            $out = @(& $rain comment add "@$((Resolve-Path $file).Path)" --repo $id --session $Session 2>&1 | ForEach-Object { "$_" })
            # A warning (e.g. a reply with no matching thread) exits non-zero but still imports.
            $res = Get-JsonLine $out
            if (-not $res.success) { Write-Output "$folder  comments FAILED  $($out -join ' ')"; continue }
            Write-Output "$folder ($id)  comments sent $($res.count) (Rain skips ones it already has)$(if ($res.warnings) { "  warnings: $($res.warnings -join '; ')" })"
        }
    }
    Write-Output "$state  session $Session  pid $($info.pid)  repos: $($known -join ', ')"
}
elseif ($Mode -eq 'get') {
    $out = & $rain comment get --session $Session --format json --all
    if ($LASTEXITCODE -ne 0) { throw "rain-story: no window for session $Session ($out)" }
    if ($OutDir) {
        New-Item -ItemType Directory -Force $OutDir | Out-Null
        $out | Set-Content (Join-Path $OutDir 'rain-comments.json') -Encoding utf8
        Write-Output "wrote $(Join-Path $OutDir 'rain-comments.json')"
    }
    else { Write-Output $out }
}
else {
    & $rain close --session $Session
}
