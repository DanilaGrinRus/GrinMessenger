# Independent bounded live Tor attempt (verifier, task-4).
# Rebuilds and runs the ignored live_onion test with a hard 600 s wall cap.
$ErrorActionPreference = 'Continue'
$root = 'C:\DHW\FlintMessenger'
$evidence = Join-Path $root 'evidence\20261107-extraction\verify-raw'
$log = Join-Path $evidence '06-live-tor-independent.log'
$out = Join-Path $evidence '06-live-tor-independent.stdout.tmp'
$err = Join-Path $evidence '06-live-tor-independent.stderr.tmp'
$env:TMP = Join-Path $root '.tmp-verify'
$env:TEMP = $env:TMP
$env:CARGO_TARGET_DIR = Join-Path $root 'target-verify'

"=== independent live Tor attempt (bounded) $(Get-Date -Format o) ===" | Set-Content $log
"TMP=$env:TMP CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR" | Add-Content $log
"command: cargo test -p flint-tor-arti --test live_onion --offline -- --ignored --nocapture --test-threads=1" | Add-Content $log
"=== source fingerprint: $(Get-FileHash (Join-Path $root 'crates\flint-tor-arti\tests\live_onion.rs') -Algorithm SHA256).Hash" | Add-Content $log

$cargo = (Get-Command cargo).Source
Set-Location $root
$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $cargo `
    -ArgumentList @('test', '-p', 'flint-tor-arti', '--test', 'live_onion', '--offline', '--', '--ignored', '--nocapture', '--test-threads=1') `
    -PassThru -NoNewWindow -RedirectStandardOutput $out -RedirectStandardError $err
$finished = $p.WaitForExit(600000)
$sw.Stop()
if (-not $finished) {
    & taskkill /F /T /PID $p.Id 2>&1 | Out-Null
    "TIMEOUT: killed process tree after 600s" | Add-Content $log
    $code = 'TIMEOUT'
} else {
    $code = $p.ExitCode
}
if (Test-Path $out) { Get-Content $out | Add-Content $log }
if (Test-Path $err) { Get-Content $err | Add-Content $log }
"EXIT_CODE=$code ELAPSED_SEC=$([math]::Round($sw.Elapsed.TotalSeconds,1)) FINISHED=$finished" | Add-Content $log
Remove-Item -Force -ErrorAction SilentlyContinue $out, $err
exit 0
