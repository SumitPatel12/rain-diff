<#
Wait on a Rain session as the attached agent until something needs the agent, then print it and exit.
Run it in the background while the user reviews; one wake per actionable event.
  exit 0 : events printed as JSON ({"seq","gap","events":[…]}); handle them, then rerun with -Since <seq>.
           "gap":true means events were lost off the ring: run `rain next --session <s>` first.
  exit 3 : the window closed (or the session has no window)
Kinds: ask (answer now), submitted (the user pressed Finish review), brief-missing (set a brief), plus any -Kinds given.
Rain binary: $env:RAIN_EXE, else the per-OS install path of install-rain.ps1, else Rain on PATH.
#>
param(
    [Parameter(Mandatory)] [string]$Session,
    [string]$Since,
    [string]$Kinds = 'ask,submitted,brief-missing'
)

function Resolve-RainExe {
    if ($env:RAIN_EXE) { return $env:RAIN_EXE }
    $default = if ($IsWindows) { Join-Path $env:LOCALAPPDATA 'Programs\Rain\Rain.exe' } else { Join-Path $HOME '.local/bin/Rain' }
    if (Test-Path $default) { return $default }
    $onPath = Get-Command Rain -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($onPath) { return $onPath.Source }
    return $default
}
$rain = Resolve-RainExe
if (-not (Test-Path $rain)) { Write-Output "Rain not found at $rain (set RAIN_EXE)"; exit 3 }

# Runs one watch call; stdout's last JSON line is the result, stderr only explains a failure.
function Invoke-Watch([string[]]$watchArgs) {
    $lines = @(& $rain watch @watchArgs --session $Session 2>&1 | ForEach-Object { "$_" })
    $json = $lines | Where-Object { $_ -match '^\s*\{' } | Select-Object -Last 1
    [pscustomobject]@{ Code = $LASTEXITCODE; Json = $json; Doc = if ($json) { $json | ConvertFrom-Json }; Text = $lines -join ' ' }
}

if (-not $Since) {
    $first = Invoke-Watch @('--kinds', $Kinds, '--timeout', '1')
    if (-not $first.Doc) { Write-Output "window gone: $($first.Text)"; exit 3 }
    $Since = $first.Doc.seq
}
while ($true) {
    $w = Invoke-Watch @('--for', 'agent', '--kinds', $Kinds, '--since', $Since, '--timeout', '60')
    if ($w.Code -notin 0, 2 -or -not $w.Doc) { Write-Output "window gone: $($w.Text)"; exit 3 }
    if ($w.Doc.events -or $w.Doc.gap) { Write-Output $w.Json; exit 0 }
    $Since = $w.Doc.seq
}
