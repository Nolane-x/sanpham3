param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet("egress", "shaper", "client")]
    [string]$Role,

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$RoleArgs
)

$ErrorActionPreference = "Stop"

function Usage {
    @"
usage:
  g7-physical-role.ps1 egress <bind_addr> <node_id> <expected_client_node> [evidence_dir]
  g7-physical-role.ps1 shaper <listen_addr> <upstream_ip:port> <aggregate_bps> <chunk_bytes> <outage_period_ms> <outage_down_ms> [evidence_dir]
  g7-physical-role.ps1 client <shaper_ip:port> <node_id> <expected_egress_node> <hostname> [evidence_dir]
"@ | Write-Host
}

function Require-Psk {
    if (-not $env:SP3_PEER_PSK_HEX) {
        throw "SP3_PEER_PSK_HEX must contain the shared 64-hex PSK"
    }
    if ($env:SP3_PEER_PSK_HEX -notmatch "^[0-9a-fA-F]{64}$") {
        throw "SP3_PEER_PSK_HEX must be exactly 64 hexadecimal characters"
    }
}

function Build-Binaries {
    cargo build --release -p g7-shaper-proxy -p peer-egress-cli
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed"
    }

    $script:ShaperBin = Join-Path $PWD "target\release\g7-shaper-proxy.exe"
    $script:EgressBin = Join-Path $PWD "target\release\peer-egress-cli.exe"

    if (-not (Test-Path $script:ShaperBin)) {
        throw "g7-shaper-proxy.exe not found"
    }
    if (-not (Test-Path $script:EgressBin)) {
        throw "peer-egress-cli.exe not found"
    }
}

function Prepare(
    [string]$Name,
    [string]$Requested
) {
    if ($Requested) {
        $script:EvidenceDir = $Requested
    } else {
        $stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
        $script:EvidenceDir = Join-Path "evidence" "g7-$Name-$stamp"
    }

    New-Item -ItemType Directory -Force -Path $script:EvidenceDir | Out-Null
    $script:LogFile = Join-Path $script:EvidenceDir "$Name.log"
    $script:MetaFile = Join-Path $script:EvidenceDir "metadata.txt"
}

function Write-Meta(
    [string]$Name,
    [string[]]$Items
) {
    $commit = (git rev-parse HEAD).Trim()
    $shaperHash = (Get-FileHash -Algorithm SHA256 $script:ShaperBin).Hash.ToLowerInvariant()
    $egressHash = (Get-FileHash -Algorithm SHA256 $script:EgressBin).Hash.ToLowerInvariant()

    $lines = @(
        "timestamp_utc=$((Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ"))",
        "git_commit=$commit",
        "role=$Name",
        "computer_name=$env:COMPUTERNAME",
        "os=$([System.Environment]::OSVersion.VersionString)",
        "shaper_sha256=$shaperHash",
        "peer_egress_sha256=$egressHash"
    )
    $lines += $Items
    $lines | Set-Content -Encoding UTF8 $script:MetaFile
}

function Run-Logged(
    [string]$Binary,
    [string[]]$Arguments
) {
    & $Binary @Arguments 2>&1 | Tee-Object -FilePath $script:LogFile
    if ($LASTEXITCODE -ne 0) {
        throw "$Binary exited with code $LASTEXITCODE"
    }
}

function Require-Contains(
    [string]$Pattern,
    [string]$ErrorMessage
) {
    $text = Get-Content -Raw $script:LogFile
    if ($text -notmatch [Regex]::Escape($Pattern)) {
        throw $ErrorMessage
    }
}

function Finalize {
    $files = Get-ChildItem -File $script:EvidenceDir |
        Where-Object { $_.Name -ne "SHA256SUMS" } |
        Sort-Object Name

    $lines = foreach ($file in $files) {
        $hash = (Get-FileHash -Algorithm SHA256 $file.FullName).Hash.ToLowerInvariant()
        "$hash  $($file.Name)"
    }
    $lines | Set-Content -Encoding ASCII (Join-Path $script:EvidenceDir "SHA256SUMS")
}

Build-Binaries

switch ($Role) {
    "egress" {
        if ($RoleArgs.Count -lt 3 -or $RoleArgs.Count -gt 4) {
            Usage
            exit 2
        }
        Require-Psk

        $bind = $RoleArgs[0]
        $node = $RoleArgs[1]
        $expectedClient = $RoleArgs[2]
        $evidence = if ($RoleArgs.Count -eq 4) { $RoleArgs[3] } else { $null }

        Prepare "egress" $evidence
        Write-Meta "egress" @(
            "bind_addr=$bind",
            "node_id=$node",
            "expected_client_node=$expectedClient"
        )
        Run-Logged $script:EgressBin @("server", $bind, $node, "-")
        Require-Contains "authenticated peer node_id=$expectedClient" "unexpected client identity"
        Require-Contains "request served" "egress did not serve request"
        Write-Host "G7_ROLE_PASS role=egress node_id=$node client_node=$expectedClient"
    }

    "shaper" {
        if ($RoleArgs.Count -lt 6 -or $RoleArgs.Count -gt 7) {
            Usage
            exit 2
        }

        $listen = $RoleArgs[0]
        $upstream = $RoleArgs[1]
        [UInt64]$bps = $RoleArgs[2]
        $chunk = $RoleArgs[3]
        $period = $RoleArgs[4]
        $down = $RoleArgs[5]
        $evidence = if ($RoleArgs.Count -eq 7) { $RoleArgs[6] } else { $null }

        Prepare "shaper" $evidence
        Write-Meta "shaper" @(
            "listen_addr=$listen",
            "upstream_addr=$upstream",
            "aggregate_bps=$bps",
            "chunk_bytes=$chunk",
            "outage_period_ms=$period",
            "outage_down_ms=$down"
        )
        Run-Logged $script:ShaperBin @($listen, $upstream, "$bps", $chunk, $period, $down)
        Require-Contains "G7_SHAPER_PASS aggregate_target_bps=$bps" "shaper did not complete"

        $text = Get-Content -Raw $script:LogFile
        $observedMatch = [Regex]::Match($text, "wall_observed_bps=([0-9]+)")
        $bytesMatch = [Regex]::Match($text, "total_bytes=([0-9]+)")
        if (-not $observedMatch.Success -or -not $bytesMatch.Success) {
            throw "shaper metrics missing"
        }

        [UInt64]$observed = $observedMatch.Groups[1].Value
        [UInt64]$total = $bytesMatch.Groups[1].Value
        if ($total -eq 0) {
            throw "shaper relayed zero bytes"
        }
        if ($observed -gt $bps) {
            throw "observed aggregate rate $observed exceeded configured cap $bps"
        }

        Write-Host "G7_ROLE_PASS role=shaper target_bps=$bps observed_bps=$observed total_bytes=$total"
    }

    "client" {
        if ($RoleArgs.Count -lt 4 -or $RoleArgs.Count -gt 5) {
            Usage
            exit 2
        }
        Require-Psk

        $peer = $RoleArgs[0]
        $node = $RoleArgs[1]
        $expectedEgress = $RoleArgs[2]
        $hostName = $RoleArgs[3]
        $evidence = if ($RoleArgs.Count -eq 5) { $RoleArgs[4] } else { $null }

        Prepare "client" $evidence
        Write-Meta "client" @(
            "shaper_addr=$peer",
            "node_id=$node",
            "expected_egress_node=$expectedEgress",
            "hostname=$hostName"
        )
        Run-Logged $script:EgressBin @("client", $peer, $node, "-", $hostName)
        Require-Contains "authenticated egress peer node_id=$expectedEgress" "unexpected egress identity"
        Require-Contains "resolved $hostName through peer:" "client did not receive public result"
        Write-Host "G7_ROLE_PASS role=client node_id=$node egress_node=$expectedEgress hostname=$hostName"
    }
}

Finalize
Write-Host "evidence=$script:EvidenceDir"
