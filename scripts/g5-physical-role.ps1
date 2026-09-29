param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet("egress", "relay", "client")]
    [string]$Role,

    [Parameter(Mandatory = $true, Position = 1)]
    [string]$Address,

    [Parameter(Mandatory = $true, Position = 2)]
    [UInt64]$NodeId,

    [Parameter(Mandatory = $true, Position = 3)]
    [UInt64]$ExpectedPeerNode,

    [Parameter(Position = 4)]
    [string]$Extra,

    [Parameter(Position = 5)]
    [UInt64]$ExpectedDownstreamNode = 0,

    [Parameter(Position = 6)]
    [string]$EvidenceDir
)

$ErrorActionPreference = "Stop"

if (-not $env:SP3_PEER_PSK_HEX) {
    throw "SP3_PEER_PSK_HEX must contain the shared 64-hex PSK"
}
if ($env:SP3_PEER_PSK_HEX -notmatch "^[0-9a-fA-F]{64}$") {
    throw "SP3_PEER_PSK_HEX must be exactly 64 hexadecimal characters"
}

cargo build --release -p peer-egress-cli
if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed"
}

$Binary = Join-Path $PWD "target\release\peer-egress-cli.exe"
if (-not (Test-Path $Binary)) {
    throw "peer-egress-cli.exe not found at $Binary"
}

$Stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
if (-not $EvidenceDir) {
    $EvidenceDir = Join-Path "evidence" "g5-$Role-$Stamp"
}
New-Item -ItemType Directory -Force -Path $EvidenceDir | Out-Null

$LogFile = Join-Path $EvidenceDir "$Role.log"
$MetaFile = Join-Path $EvidenceDir "metadata.txt"
$BinaryHash = (Get-FileHash -Algorithm SHA256 $Binary).Hash.ToLowerInvariant()
$GitCommit = (git rev-parse HEAD).Trim()

$Metadata = @(
    "timestamp_utc=$((Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ"))",
    "git_commit=$GitCommit",
    "role=$Role",
    "computer_name=$env:COMPUTERNAME",
    "os=$([System.Environment]::OSVersion.VersionString)",
    "binary=$Binary",
    "binary_sha256=$BinaryHash",
    "address=$Address",
    "node_id=$NodeId",
    "expected_peer_node=$ExpectedPeerNode"
)

$Arguments = @()
switch ($Role) {
    "egress" {
        $Metadata += "expected_relay_node=$ExpectedPeerNode"
        $Arguments = @("server", $Address, "$NodeId", "-")
    }
    "relay" {
        if (-not $Extra) {
            throw "relay requires Extra=<upstream_ip:port>"
        }
        if ($ExpectedDownstreamNode -eq 0) {
            throw "relay requires ExpectedDownstreamNode"
        }
        $Metadata += "upstream_addr=$Extra"
        $Metadata += "expected_upstream_node=$ExpectedPeerNode"
        $Metadata += "expected_downstream_node=$ExpectedDownstreamNode"
        $Arguments = @("relay-server", $Address, "$NodeId", "-", $Extra)
    }
    "client" {
        if (-not $Extra) {
            throw "client requires Extra=<public_hostname>"
        }
        $Metadata += "relay_addr=$Address"
        $Metadata += "expected_relay_node=$ExpectedPeerNode"
        $Metadata += "target_hostname=$Extra"
        $Arguments = @("client", $Address, "$NodeId", "-", $Extra)
    }
}

$Metadata | Set-Content -Encoding UTF8 $MetaFile

& $Binary @Arguments 2>&1 | Tee-Object -FilePath $LogFile
$ExitCode = $LASTEXITCODE
if ($ExitCode -ne 0) {
    throw "peer-egress-cli exited with code $ExitCode"
}

$Log = Get-Content -Raw $LogFile
switch ($Role) {
    "egress" {
        if ($Log -notmatch "authenticated peer node_id=$ExpectedPeerNode") {
            throw "egress did not authenticate expected relay node"
        }
        if ($Log -notmatch "request served") {
            throw "egress did not serve request"
        }
    }
    "relay" {
        if ($Log -notmatch "authenticated upstream node_id=$ExpectedPeerNode") {
            throw "relay did not authenticate expected upstream node"
        }
        if ($Log -notmatch "authenticated downstream node_id=$ExpectedDownstreamNode") {
            throw "relay did not authenticate expected downstream node"
        }
        if ($Log -notmatch "relayed one request") {
            throw "relay did not relay request"
        }
    }
    "client" {
        if ($Log -notmatch "authenticated egress peer node_id=$ExpectedPeerNode") {
            throw "client did not authenticate expected relay node"
        }
        if ($Log -notmatch "resolved $([Regex]::Escape($Extra)) through peer:") {
            throw "client did not receive the expected peer resolution result"
        }
    }
}

$HashLines = @()
foreach ($File in @($LogFile, $MetaFile)) {
    $Hash = (Get-FileHash -Algorithm SHA256 $File).Hash.ToLowerInvariant()
    $HashLines += "$Hash  $([IO.Path]::GetFileName($File))"
}
$HashLines | Set-Content -Encoding ASCII (Join-Path $EvidenceDir "SHA256SUMS")

Write-Host "G5_ROLE_PASS role=$Role node_id=$NodeId evidence=$EvidenceDir"
