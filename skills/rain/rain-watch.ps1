<#
Wait on a Rain session as the attached agent until something needs the agent, then print it and exit.
Run it in the background while the user reviews; one wake per actionable event.
  exit 0 : events printed as JSON ({"seq","events":[…]}); handle them, then rerun with -Since <seq>
  exit 3 : the window closed (or the session has no window)
Kinds: ask (answer now), submitted (the user pressed Finish review), plus any -Kinds given.
Rain binary: $env:RAIN_EXE, else the per-OS install path of install-rain.ps1, else Rain on PATH.
#>
param(
    [Parameter(Mandatory)] [string]$Session,
    [string]$Since,
    [string]$Kinds = 'ask,submitted'
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

if (-not $Since) {
    $first = & $rain watch --kinds $Kinds --timeout 1 --session $Session 2>&1
    if ($LASTEXITCODE -eq 1) { Write-Output "window gone: $first"; exit 3 }
    $Since = ($first | ConvertFrom-Json).seq
}
while ($true) {
    $out = & $rain watch --for agent --kinds $Kinds --since $Since --timeout 60 --session $Session 2>&1
    if ($LASTEXITCODE -eq 2) { $Since = ($out | ConvertFrom-Json).seq; continue }
    if ($LASTEXITCODE -ne 0) { Write-Output "window gone: $out"; exit 3 }
    if (($out | ConvertFrom-Json).events) { Write-Output "$out"; exit 0 }
    $Since = ($out | ConvertFrom-Json).seq
}
