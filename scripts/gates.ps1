<#
.SYNOPSIS
  Rain's merge gates: fmt, clippy -D warnings, tests, release build. Stops at the first failure
  with its exit code. Run from any worktree root; honours $env:CARGO_TARGET_DIR.
.PARAMETER Perf
  Also runs the #[ignore] release perf tests and appends their output to docs/perf/<yyyy-MM-dd-HHmm>.txt.
#>
param([switch]$Perf)

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $steps = @(
        @{ Name = 'fmt'; Args = @('fmt', '--all', '--check') },
        @{ Name = 'clippy'; Args = @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings') },
        @{ Name = 'test'; Args = @('test', '--workspace') },
        @{ Name = 'build --release'; Args = @('build', '--release') }
    )
    if ($env:CARGO_TARGET_DIR) { Write-Host "target dir: $env:CARGO_TARGET_DIR" }
    $total = [Diagnostics.Stopwatch]::StartNew()
    foreach ($step in $steps) {
        Write-Host "==> cargo $($step.Args -join ' ')" -ForegroundColor Cyan
        $sw = [Diagnostics.Stopwatch]::StartNew()
        & cargo @($step.Args)
        $code = $LASTEXITCODE
        $sw.Stop()
        Write-Host ("<== {0}: exit {1} in {2:N1} s" -f $step.Name, $code, $sw.Elapsed.TotalSeconds)
        if ($code -ne 0) {
            Write-Host "GATES FAILED at $($step.Name)" -ForegroundColor Red
            exit $code
        }
    }
    if ($Perf) {
        $dir = Join-Path $root 'docs\perf'
        New-Item -ItemType Directory -Force $dir | Out-Null
        $file = Join-Path $dir ("{0}.txt" -f (Get-Date -Format 'yyyy-MM-dd-HHmm'))
        Write-Host "==> perf tests -> $file" -ForegroundColor Cyan
        $sw = [Diagnostics.Stopwatch]::StartNew()
        "# $(Get-Date -Format o) $(git -C $root rev-parse --short HEAD)" | Out-File -Append -Encoding utf8 $file
        & cargo test --workspace --release -- --ignored --nocapture 2>&1 | Tee-Object -Variable perfOut | Out-Host
        $code = $LASTEXITCODE
        $perfOut | Out-File -Append -Encoding utf8 $file
        $sw.Stop()
        Write-Host ("<== perf: exit {0} in {1:N1} s" -f $code, $sw.Elapsed.TotalSeconds)
        if ($code -ne 0) {
            Write-Host 'GATES FAILED at perf' -ForegroundColor Red
            exit $code
        }
    }
    Write-Host ("GATES PASSED in {0:N1} s" -f $total.Elapsed.TotalSeconds) -ForegroundColor Green
    exit 0
}
finally {
    Pop-Location
}
