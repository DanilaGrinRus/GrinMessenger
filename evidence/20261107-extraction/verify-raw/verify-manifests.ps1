# Independent re-check of the two provenance manifests (verifier, task-4).
# 1) freeze manifest of the deliverable sources; 2) upstream source hash manifest.
$ErrorActionPreference = 'Continue'
$root = 'C:\DHW\FlintMessenger'
$ev = "$root\evidence\20261107-extraction"
$outFreeze = "$ev\verify-raw\13-freeze-manifest-check.log"
$outUpstream = "$ev\verify-raw\14-upstream-provenance-check.log"
$failures = 0

# --- freeze manifest ---
$lines = Get-Content "$ev\source-sha256.sha256"
$manifestHash = (Get-FileHash "$ev\source-sha256.sha256" -Algorithm SHA256).Hash.ToLower()
$hashFile = (Get-Content "$ev\source-sha256.sha256.hash" -Raw).Trim()
$ok = 0; $bad = @(); $missing = @()
foreach ($line in $lines) {
    if ($line -match '^([0-9a-fA-F]{64})\s+\*?(.+)$') {
        $expected = $Matches[1].ToLower(); $rel = $Matches[2].Trim()
        $full = Join-Path $root $rel
        if (-not (Test-Path $full)) { $missing += $rel; continue }
        $actual = (Get-FileHash $full -Algorithm SHA256).Hash.ToLower()
        if ($actual -eq $expected) { $ok++ } else { $bad += "$rel expected=$expected actual=$actual" }
    }
}
$outLines = @(
    "TIME=$(Get-Date -Format o)",
    "manifest=$ev\source-sha256.sha256",
    "manifest_sha256=$manifestHash",
    "manifest_hash_file=$hashFile",
    "entries=$($lines.Count) ok=$ok bad=$($bad.Count) missing=$($missing.Count)",
    "MATCH_HASH_FILE=$([bool]($hashFile -match [regex]::Escape($manifestHash)))"
) + @($bad | ForEach-Object { "BAD $_" }) + @($missing | ForEach-Object { "MISSING $_" })
($outLines -join "`n") | Set-Content $outFreeze
$failures += @($bad).Count + @($missing).Count + $(if ($hashFile -match [regex]::Escape($manifestHash)) { 0 } else { 1 })
Write-Host "freeze: entries=$($lines.Count) ok=$ok bad=$($bad.Count) missing=$($missing.Count)"

# --- upstream provenance manifest ---
$upRoot = 'C:\DHW\tmp\flint-upstream'
$upLines = Get-Content "$ev\upstream-sources.sha256"
$upHash = (Get-FileHash "$ev\upstream-sources.sha256" -Algorithm SHA256).Hash.ToLower()
$upOk = 0; $upBad = @(); $upMissing = @()
foreach ($line in $upLines) {
    if ($line -match '^([0-9a-fA-F]{64})\s+\*?(.+)$') {
        $expected = $Matches[1].ToLower(); $rel = $Matches[2].Trim()
        $full = Join-Path $upRoot $rel
        if (-not (Test-Path $full)) { $upMissing += $rel; continue }
        $actual = (Get-FileHash $full -Algorithm SHA256).Hash.ToLower()
        if ($actual -eq $expected) { $upOk++ } else { $upBad += "$rel expected=$expected actual=$actual" }
    }
}
$head = (& git -C $upRoot rev-parse HEAD) 2>$null
$status = (& git -C $upRoot status --porcelain) 2>$null
$outLines = @(
    "TIME=$(Get-Date -Format o)",
    "manifest=$ev\upstream-sources.sha256 sha256=$upHash",
    "upstream_root=$upRoot",
    "upstream_head=$head",
    "upstream_dirty=$([bool]($status -and $status.Trim()))",
    "entries=$($upLines.Count) ok=$upOk bad=$($upBad.Count) missing=$($upMissing.Count)"
) + @($upBad | ForEach-Object { "BAD $_" }) + @($upMissing | ForEach-Object { "MISSING $_" })
($outLines -join "`n") | Set-Content $outUpstream
$failures += @($upBad).Count + @($upMissing).Count
Write-Host "upstream: entries=$($upLines.Count) ok=$upOk bad=$($upBad.Count) missing=$($upMissing.Count) head=$head dirty=$([bool]($status -and $status.Trim()))"

$lines | ForEach-Object { ($_ -split '\s+', 2)[1] } | Sort-Object | Set-Content "$ev\verify-raw\13-freeze-manifest-files.txt"
Write-Host "FINAL_FAILURES=$failures"
"OVERALL_FAILURES=$failures" | Add-Content "$ev\verify-raw\12-final-summary.log"
if ($failures -ne 0) { exit 1 } else { exit 0 }
