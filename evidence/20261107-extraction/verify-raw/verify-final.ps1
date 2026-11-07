# Final frozen-state verification run (verifier, task-4).
# Runs fmt/build/test/ignored relay_live with the verifier's own TMP and target dir,
# logs each command with its real exit code, and fails loudly if a step fails.
$ErrorActionPreference = 'Continue'
$root = 'C:\DHW\FlintMessenger'
$env:TMP = "$root\.tmp-verify"
$env:TEMP = $env:TMP
$env:CARGO_TARGET_DIR = "$root\target-verify"
Set-Location $root
$evidence = "$root\evidence\20261107-extraction\verify-raw"
$script:failures = 0

function RunCargo([string]$name, [string[]]$cargoArgs) {
    $log = Join-Path $evidence $name
    "TIME=$(Get-Date -Format o)" | Set-Content $log
    "CMD: cargo $($cargoArgs -join ' ')" | Add-Content $log
    "TMP=$env:TMP CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR" | Add-Content $log
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & cargo @cargoArgs *>> $log
    $code = $LASTEXITCODE
    $sw.Stop()
    "EXIT_CODE=$code ELAPSED_SEC=$([math]::Round($sw.Elapsed.TotalSeconds,1))" | Add-Content $log
    Write-Host ("{0}: EXIT_CODE={1} ({2}s)" -f $name, $code, [math]::Round($sw.Elapsed.TotalSeconds,1))
    if ($code -ne 0) { $script:failures++ }
}

RunCargo '08-fmt-final.log' @('fmt', '--all', '--', '--check')
RunCargo '09-build-final.log' @('build', '--workspace', '--offline', '--locked')
RunCargo '10-tests-final.log' @('test', '--workspace', '--offline', '--locked')
RunCargo '11-relay-live-final.log' @('test', '-p', 'flint-gateway', '--test', 'relay_live', '--offline', '--', '--ignored', '--test-threads=1')

# Sanity: the logs must contain the real test output, not cargo's help text.
$tests = Get-Content (Join-Path $evidence '10-tests-final.log') -Raw
if ($tests -notmatch 'test result: ok') { Write-Host 'SANITY FAIL: 10-tests-final.log has no test result'; $script:failures++ }
$relay = Get-Content (Join-Path $evidence '11-relay-live-final.log') -Raw
if ($relay -notmatch '1 passed') { Write-Host 'SANITY FAIL: 11-relay-live-final.log has no 1 passed'; $script:failures++ }
if ((Get-Content (Join-Path $evidence '09-build-final.log') -Raw) -match 'Usage: cargo') { Write-Host 'SANITY FAIL: build log looks like cargo help'; $script:failures++ }

"OVERALL_EXIT=$($script:failures)" | Add-Content (Join-Path $evidence '12-final-summary.log')
Write-Host "FINAL_FAILURES=$($script:failures)"
if ($script:failures -ne 0) { exit 1 } else { exit 0 }
