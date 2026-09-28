param(
    [Parameter(Mandatory = $true)][string]$PeerAddr,
    [Parameter(Mandatory = $true)][UInt64]$NodeId,
    [Parameter(Mandatory = $true)][string]$Psk,
    [Parameter(Mandatory = $true)][string]$Hostname,
    [Parameter(Mandatory = $true)][string]$HttpsAddr,
    [Parameter(Mandatory = $true)][string]$HttpsServerName,
    [string]$EvidenceDir = ""
)

$ErrorActionPreference = "Stop"

if ($Psk.Length -ne 64 -or $Psk -notmatch '^[0-9a-fA-F]{64}$') {
    throw "Psk must contain exactly 64 hexadecimal characters"
}

if ([string]::IsNullOrWhiteSpace($EvidenceDir)) {
    $stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
    $EvidenceDir = "g9-evidence-$stamp"
}

New-Item -ItemType Directory -Force -Path $EvidenceDir | Out-Null
$EvidenceDir = (Resolve-Path $EvidenceDir).Path

cargo build --release -p host-probe-cli -p peer-egress-cli
if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed"
}

$env:SP3_HTTPS_PROBE_ADDR = $HttpsAddr
$env:SP3_HTTPS_PROBE_NAME = $HttpsServerName
$env:SP3_HTTPS_PROBE_PATH = "/"
$env:SP3_HTTPS_PROBE_MAX_BYTES = "1024"

$directLog = Join-Path $EvidenceDir "direct-path.log"
$peerLog = Join-Path $EvidenceDir "peer-rescue.log"

Write-Host "=== exact-path host probe ==="
& .\target\release\host-probe-cli.exe 2>&1 | Tee-Object -FilePath $directLog
if ($LASTEXITCODE -ne 0) {
    throw "host-probe-cli failed"
}

if (-not (Select-String -Path $directLog -SimpleMatch "MEASURED_PATHS none_verified_by_tiny_https" -Quiet)) {
    throw "Direct tiny-HTTPS path was verified; refusing false G9 PASS"
}

Write-Host "=== authenticated peer rescue ==="
$env:SP3_PEER_PSK_HEX = $Psk
& .\target\release\peer-egress-cli.exe client $PeerAddr $NodeId - $Hostname 2>&1 |
    Tee-Object -FilePath $peerLog
Remove-Item Env:SP3_PEER_PSK_HEX -ErrorAction SilentlyContinue
if ($LASTEXITCODE -ne 0) {
    throw "peer-egress-cli failed"
}

if (-not (Select-String -Path $peerLog -SimpleMatch "authenticated egress peer node_id=" -Quiet)) {
    throw "Peer authentication evidence missing"
}
if (-not (Select-String -Path $peerLog -SimpleMatch "resolved $Hostname through peer:" -Quiet)) {
    throw "Peer rescue result evidence missing"
}

$commit = (git rev-parse HEAD).Trim()
$utc = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
$directSha = (Get-FileHash -Algorithm SHA256 $directLog).Hash.ToLowerInvariant()
$peerSha = (Get-FileHash -Algorithm SHA256 $peerLog).Hash.ToLowerInvariant()
$os = [System.Environment]::OSVersion.VersionString
$machine = [System.Environment]::MachineName

$evidence = @"
gate=G9
result=PASS_CANDIDATE
timestamp_utc=$utc
git_commit=$commit
platform=windows
os=$os
machine=$machine
default_path_tiny_https=NO_VERIFIED_PATH
peer_addr=$PeerAddr
local_node_id=$NodeId
remote_operation=resolve:$Hostname
direct_log_sha256=$directSha
peer_log_sha256=$peerSha
note=Candidate physical evidence; review topology and freshness before marking G9 closed.
"@

$evidencePath = Join-Path $EvidenceDir "evidence.txt"
Set-Content -Path $evidencePath -Value $evidence -Encoding UTF8

Write-Host "G9_CAPTURE_PASS evidence_dir=$EvidenceDir"
Write-Host "Review evidence.txt plus both raw logs before checking the physical G9 gate."
