# Independent loopback E2E for FlintMessenger (verifier, task-4).
# Real processes: flint-relay.exe + flintmsg.exe. No mocks.
# Writes a line-oriented PASS/FAIL log to verify-raw/05-e2e-demo.log.
$ErrorActionPreference = 'Continue'
$root = 'C:\DHW\FlintMessenger'
$bin  = Join-Path $root 'target-verify\debug'
$evidence = Join-Path $root 'evidence\20261107-extraction\verify-raw'
$work = Join-Path $root ('.tmp-verify\e2e-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Force $work | Out-Null
$logName = if ($env:VERIFY_E2E_LOG) { $env:VERIFY_E2E_LOG } else { '05-e2e-demo.log' }
$log = Join-Path $evidence $logName
'' | Set-Content $log

$script:failed = 0
function Log([string]$line) { $line | Tee-Object -FilePath $log -Append | Out-Null; Write-Host $line }
function Check([string]$name, [bool]$ok, [string]$detail = '') {
    if ($ok) { Log ("RESULT {0} PASS {1}" -f $name, $detail) }
    else { Log ("RESULT {0} FAIL {1}" -f $name, $detail); $script:failed++ }
}
function Run([string]$exe, [string[]]$cmdArgs) {
    $out = & $exe @cmdArgs 2>&1
    $code = $LASTEXITCODE
    [pscustomobject]@{ Code = $code; Out = ($out -join "`n") }
}
function LastJson([string]$text) {
    $line = ($text -split "`n" | Where-Object { $_.Trim().StartsWith('{') } | Select-Object -Last 1)
    if ($line) { return ($line | ConvertFrom-Json) } else { return $null }
}

Log ("=== independent loopback E2E (real processes) {0} ===" -f (Get-Date -Format o))
Log ("work={0}" -f $work)
$relayExe = Join-Path $bin 'flint-relay.exe'
$msgExe   = Join-Path $bin 'flintmsg.exe'
Log ("flint-relay.exe sha256={0}" -f (Get-FileHash $relayExe -Algorithm SHA256).Hash)
Log ("flintmsg.exe    sha256={0}" -f (Get-FileHash $msgExe -Algorithm SHA256).Hash)
Check 'binaries-exist' ((Test-Path $relayExe) -and (Test-Path $msgExe))

# --- free loopback port ---
$listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
$port = ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()
$relayUrl = "http://127.0.0.1:$port"
Log ("relay_url={0}" -f $relayUrl)

$relayStore = Join-Path $work 'relay-store'
$relayOut = Join-Path $work 'relay.stdout.log'
$relayErr = Join-Path $work 'relay.stderr.log'
$relay = Start-Process -FilePath $relayExe -ArgumentList @('--listen', "127.0.0.1:$port", '--store', $relayStore) `
    -PassThru -RedirectStandardOutput $relayOut -RedirectStandardError $relayErr
Log ("relay_pid={0}" -f $relay.Id)

$ready = $false
for ($i = 0; $i -lt 50; $i++) {
    Start-Sleep -Milliseconds 200
    try {
        $r = Invoke-WebRequest -Uri "$relayUrl/flint/v1/health" -TimeoutSec 2 -SkipHttpErrorCheck
        if ($r.StatusCode -eq 200) { $ready = $true; break }
    } catch { }
}
Check 'relay-process-ready' ($ready -and -not $relay.HasExited)
Log ("relay_stdout: " + ((Get-Content $relayOut -Raw) -replace "`r?`n", ' | '))

# --- 1. keygen alice + bob (real processes, random seeds) ---
$alicePath = Join-Path $work 'alice.json'
$bobPath   = Join-Path $work 'bob.json'
$r1 = Run $msgExe @('keygen', '--out', $alicePath)
$r2 = Run $msgExe @('keygen', '--out', $bobPath)
$aliceJson = LastJson $r1.Out
$bobJson = LastJson $r2.Out
Log ("keygen_alice exit={0} out={1}" -f $r1.Code, $r1.Out)
Log ("keygen_bob   exit={0} out={1}" -f $r2.Code, $r2.Out)
Check 'keygen-exit' (($r1.Code -eq 0) -and ($r2.Code -eq 0))
Check 'keygen-alice-address' ([bool]($aliceJson.address -like 'tgrin1*'))
Check 'keygen-bob-address' ([bool]($bobJson.address -like 'tgrin1*'))
$aliceAddr = $aliceJson.address
$bobAddr = $bobJson.address
$bobOnion = $bobJson.onion
Log ("alice={0} onion={1}" -f $aliceAddr, $aliceJson.onion)
Log ("bob={0} onion={1}" -f $bobAddr, $bobOnion)

# --- 2. address command re-derives the same bob address/onion ---
$r3 = Run $msgExe @('address', '--identity', $bobPath)
$addrJson = LastJson $r3.Out
Log ("address_bob exit={0} out={1}" -f $r3.Code, $r3.Out)
Check 'address-exit' ($r3.Code -eq 0)
Check 'address-roundtrip' (($addrJson.address -eq $bobAddr) -and ($addrJson.onion -eq $bobOnion))
Check 'address-has-onion-url' ($addrJson.onion_url -eq "http://$bobOnion.onion")

# --- 3. alice sends through the relay (plain TCP, loopback) ---
$text = 'verify-e2e-marker: hello over standalone relay'
$r4 = Run $msgExe @('send', '--identity', $alicePath, '--to', $bobAddr, '--text', $text, '--relay', $relayUrl, '--plain')
$sendJson = LastJson $r4.Out
Log ("send exit={0} out={1}" -f $r4.Code, $r4.Out)
Check 'send-exit' ($r4.Code -eq 0)
Check 'send-event' ($sendJson.event -eq 'sent')
$messageId = $sendJson.id
Check 'send-id-uuid' ([bool]($messageId -match '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'))

# --- 4. relay stored exactly one opaque envelope; no plaintext/sender leak ---
Start-Sleep -Milliseconds 300
$stored = @(Get-ChildItem $relayStore -File -Filter '*.json' -ErrorAction SilentlyContinue)
Check 'relay-store-one-file' ($stored.Count -eq 1) ("count=" + $stored.Count)
if ($stored.Count -ge 1) {
    $storedName = $stored[0].Name
    $storedText = Get-Content $stored[0].FullName -Raw
    Check 'relay-store-file-named-by-id' ($storedName -eq "$messageId.json") ("file=" + $storedName)
    Check 'relay-store-no-plaintext' (-not ($storedText -match 'verify-e2e-marker'))
    Check 'relay-store-no-sender-address' (-not ($storedText -match [regex]::Escape($aliceAddr)))
    $storedJson = $storedText | ConvertFrom-Json
    Check 'relay-store-id-matches' ($storedJson.id -eq $messageId)
    Check 'relay-store-recipient-is-known-metadata' ($storedJson.to -eq $bobAddr)
    Check 'relay-store-payload-opaque' ([bool]($storedJson.payload -and ($storedJson.payload.Length -gt 100)))
}
$bytesOnDisk = (Get-ChildItem $relayStore -File | Measure-Object -Property Length -Sum).Sum
Log ("relay_store_bytes={0}" -f $bytesOnDisk)

# --- 5. unsigned ACK must be rejected by the relay (require_signed_ack) ---
$ackUrl = "$relayUrl/flint/v1/msg/$messageId/ack"
try {
    $unsigned = Invoke-WebRequest -Uri $ackUrl -Method Post -ContentType 'application/json' `
        -Body (@{ to = $bobAddr } | ConvertTo-Json -Compress) -SkipHttpErrorCheck -TimeoutSec 5
    $unsignedStatus = [int]$unsigned.StatusCode
} catch {
    $unsignedStatus = -1
}
Log ("unsigned_ack_http_status={0}" -f $unsignedStatus)
Check 'unsigned-ack-rejected' ($unsignedStatus -eq 401)
$stillThere = @(Get-ChildItem $relayStore -File -Filter '*.json').Count
Check 'unsigned-ack-does-not-delete' ($stillThere -eq 1) ("count=" + $stillThere)

# --- 6. bob fetch opens the message and sends a signed ACK (deletes it) ---
$r5 = Run $msgExe @('fetch', '--identity', $bobPath, '--source', $relayUrl, '--plain')
$fetchJson = LastJson $r5.Out
$fetchMessages = @($r5.Out -split "`n" | Where-Object { $_ -match '"event":"message"' })
Log ("fetch exit={0} out={1}" -f $r5.Code, $r5.Out)
Check 'fetch-exit' ($r5.Code -eq 0)
Check 'fetch-opened-one' ($fetchJson.opened -eq 1) ("opened=" + $fetchJson.opened)
Check 'fetch-message-event' ($fetchMessages.Count -eq 1)
$opened= $null
if ($fetchMessages.Count -eq 1) { $opened = $fetchMessages[0] | ConvertFrom-Json }
Check 'fetch-text-matches' ([bool]($opened -and $opened.text -eq $text))
Check 'fetch-from-alice' ([bool]($opened -and $opened.from -eq $aliceAddr))
Check 'fetch-signed-ack-count' ([bool](($r5.Out -match '"event":"acked","count":1') -or ($r5.Out -match '"count":1,"event":"acked"')))
$afterAck = @(Get-ChildItem $relayStore -File -Filter '*.json' -ErrorAction SilentlyContinue).Count
Check 'signed-ack-deleted-from-relay' ($afterAck -eq 0) ("remaining=" + $afterAck)

# --- 7. second fetch is empty (idempotent after ACK) ---
$r6 = Run $msgExe @('fetch', '--identity', $bobPath, '--source', $relayUrl, '--plain')
$fetch2 = LastJson $r6.Out
Log ("fetch2 exit={0} out={1}" -f $r6.Code, $r6.Out)
Check 'fetch2-exit' ($r6.Code -eq 0)
Check 'fetch2-empty' ($fetch2.opened -eq 0) ("opened=" + $fetch2.opened)

# --- 8. offline case: relay stopped -> send/fetch fail, no silent success ---
Stop-Process -Id $relay.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
Log ("relay_stopped pid={0}" -f $relay.Id)
$r7 = Run $msgExe @('send', '--identity', $alicePath, '--to', $bobAddr, '--text', 'offline probe', '--relay', $relayUrl, '--plain')
Log ("offline_send exit={0} out={1}" -f $r7.Code, $r7.Out)
Check 'offline-send-nonzero' ($r7.Code -ne 0)
Check 'offline-send-no-sent-event' (-not ($r7.Out -match '"event":"sent"'))
$r8 = Run $msgExe @('fetch', '--identity', $bobPath, '--source', $relayUrl, '--plain')
Log ("offline_fetch exit={0} out={1}" -f $r8.Code, $r8.Out)
Check 'offline-fetch-nonzero' ($r8.Code -ne 0)

# --- 9. CLI misuse is rejected (no fake success) ---
$r9 = Run $msgExe @('send', '--identity', $alicePath, '--to', $bobAddr, '--text', 'x', '--plain')
Log ("direct_send_with_plain exit={0} out={1}" -f $r9.Code, $r9.Out)
Check 'direct-plain-refused' ($r9.Code -ne 0)
$r10 = Run $msgExe @('fetch', '--identity', $bobPath, '--source', "http://$bobOnion.onion", '--plain')
Log ("plain-onion-refused exit={0} out={1}" -f $r10.Code, $r10.Out)
Check 'plain-onion-refused' ($r10.Code -ne 0)

# cleanup identity seed files (never leave key material around)
Remove-Item -Force -ErrorAction SilentlyContinue $alicePath, $bobPath
Log ("identity seed files deleted: {0}" -f (-not (Test-Path $alicePath)))
Log ("=== SUMMARY failures={0} ===" -f $script:failed)
if ($script:failed -ne 0) { exit 1 } else { exit 0 }
