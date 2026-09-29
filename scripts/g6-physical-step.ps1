param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet("enqueue", "receive", "send", "egress", "dispatch", "inspect", "show-result")]
    [string]$Step,

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$StepArgs
)

$ErrorActionPreference = "Stop"

function Show-Usage {
    @"
usage:
  g6-physical-step.ps1 enqueue <spool> <bundle_id> <request_id> <hostname> <bulk|normal|urgent> <ttl_secs> [evidence_dir]
  g6-physical-step.ps1 receive <bind_addr> <node_id> <expected_peer_node> <spool> <expected_bundle_id> [evidence_dir]
  g6-physical-step.ps1 send <peer_addr> <node_id> <expected_peer_node> <spool> <expected_bundle_id> [evidence_dir]
  g6-physical-step.ps1 egress <bind_addr> <node_id> <expected_peer_node> [evidence_dir]
  g6-physical-step.ps1 dispatch <egress_addr> <node_id> <expected_peer_node> <request_spool> <request_bundle_id> <return_spool> <return_bundle_id> <return_ttl_secs> [evidence_dir]
  g6-physical-step.ps1 inspect <spool> <expected_bundle_id|empty> [evidence_dir]
  g6-physical-step.ps1 show-result <spool> <expected_bundle_id> <expected_request_id> [evidence_dir]

SP3_PEER_PSK_HEX must contain the shared 64-hex laboratory PSK.
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
    cargo build --release -p recovery-lab-cli -p peer-egress-cli
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed"
    }

    $script:LabBin = Join-Path $PWD "target\release\recovery-lab-cli.exe"
    $script:EgressBin = Join-Path $PWD "target\release\peer-egress-cli.exe"

    if (-not (Test-Path $script:LabBin)) {
        throw "recovery-lab-cli.exe not found"
    }
    if (-not (Test-Path $script:EgressBin)) {
        throw "peer-egress-cli.exe not found"
    }
}

function Prepare-Evidence(
    [string]$Name,
    [string]$Requested
) {
    if ($Requested) {
        $script:EvidenceDir = $Requested
    } else {
        $stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
        $script:EvidenceDir = Join-Path "evidence" "g6-$Name-$stamp"
    }

    New-Item -ItemType Directory -Force -Path $script:EvidenceDir | Out-Null
    $script:LogFile = Join-Path $script:EvidenceDir "$Name.log"
    $script:MetaFile = Join-Path $script:EvidenceDir "metadata.txt"
}

function Write-Metadata(
    [string]$Name,
    [string[]]$Items
) {
    $labHash = (Get-FileHash -Algorithm SHA256 $script:LabBin).Hash.ToLowerInvariant()
    $egressHash = (Get-FileHash -Algorithm SHA256 $script:EgressBin).Hash.ToLowerInvariant()
    $commit = (git rev-parse HEAD).Trim()

    $lines = @(
        "timestamp_utc=$((Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ"))",
        "git_commit=$commit",
        "step=$Name",
        "computer_name=$env:COMPUTERNAME",
        "os=$([System.Environment]::OSVersion.VersionString)",
        "recovery_lab_sha256=$labHash",
        "peer_egress_sha256=$egressHash"
    )
    $lines += $Items
    $lines | Set-Content -Encoding UTF8 $script:MetaFile
}

function Snapshot-Spool(
    [string]$Label,
    [string]$Spool
) {
    $out = Join-Path $script:EvidenceDir "$Label.txt"
    $lines = @("path=$Spool")

    if (Test-Path $Spool) {
        $item = Get-Item $Spool
        $hash = (Get-FileHash -Algorithm SHA256 $Spool).Hash.ToLowerInvariant()
        $lines += "exists=yes"
        $lines += "size_bytes=$($item.Length)"
        $lines += "sha256=$hash"
    } else {
        $lines += "exists=no"
    }

    $inspect = & $script:LabBin inspect-spool $Spool 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "inspect-spool failed for $Spool"
    }

    $lines += $inspect
    $lines | Set-Content -Encoding UTF8 $out
}

function Invoke-Logged(
    [string]$Binary,
    [string[]]$Arguments
) {
    & $Binary @Arguments 2>&1 | Tee-Object -FilePath $script:LogFile
    if ($LASTEXITCODE -ne 0) {
        throw "$Binary exited with code $LASTEXITCODE"
    }
}

function Require-LogContains(
    [string]$Path,
    [string]$Pattern,
    [string]$ErrorMessage
) {
    $text = Get-Content -Raw $Path
    if ($text -notmatch [Regex]::Escape($Pattern)) {
        throw $ErrorMessage
    }
}

function Finalize-Evidence {
    $files = Get-ChildItem -File $script:EvidenceDir |
        Where-Object { $_.Name -ne "SHA256SUMS" } |
        Sort-Object Name

    $lines = foreach ($file in $files) {
        $hash = (Get-FileHash -Algorithm SHA256 $file.FullName).Hash.ToLowerInvariant()
        "$hash  $($file.Name)"
    }

    $lines | Set-Content -Encoding ASCII (Join-Path $script:EvidenceDir "SHA256SUMS")
}

Require-Psk
Build-Binaries

switch ($Step) {
    "enqueue" {
        if ($StepArgs.Count -lt 6 -or $StepArgs.Count -gt 7) {
            Show-Usage
            exit 2
        }

        $spool = $StepArgs[0]
        $bundle = $StepArgs[1]
        $request = $StepArgs[2]
        $hostName = $StepArgs[3]
        $priority = $StepArgs[4]
        $ttl = $StepArgs[5]
        $evidence = if ($StepArgs.Count -eq 7) { $StepArgs[6] } else { $null }

        Prepare-Evidence "enqueue" $evidence
        Write-Metadata "enqueue" @(
            "spool=$spool",
            "bundle_id=$bundle",
            "request_id=$request",
            "hostname=$hostName",
            "priority=$priority",
            "ttl_secs=$ttl"
        )
        Snapshot-Spool "before" $spool
        Invoke-Logged $script:LabBin @("enqueue", $spool, $bundle, $request, $hostName, $priority, $ttl)
        Snapshot-Spool "after" $spool
        Require-LogContains (Join-Path $script:EvidenceDir "after.txt") "bundle_id=$bundle" "bundle not persisted after enqueue"
        Write-Host "G6_STEP_PASS step=enqueue bundle_id=$bundle"
    }

    "receive" {
        if ($StepArgs.Count -lt 5 -or $StepArgs.Count -gt 6) {
            Show-Usage
            exit 2
        }

        $bind = $StepArgs[0]
        $node = $StepArgs[1]
        $expectedPeer = $StepArgs[2]
        $spool = $StepArgs[3]
        $bundle = $StepArgs[4]
        $evidence = if ($StepArgs.Count -eq 6) { $StepArgs[5] } else { $null }

        Prepare-Evidence "receive" $evidence
        Write-Metadata "receive" @(
            "bind_addr=$bind",
            "node_id=$node",
            "expected_peer_node=$expectedPeer",
            "spool=$spool",
            "expected_bundle_id=$bundle"
        )
        Snapshot-Spool "before" $spool
        Invoke-Logged $script:LabBin @("custody-receive", $bind, $node, "-", $spool)
        Snapshot-Spool "after" $spool
        Require-LogContains $script:LogFile "peer_id=$expectedPeer" "unexpected custody sender identity"
        Require-LogContains $script:LogFile "bundle_id=$bundle" "expected custody bundle not received"
        Require-LogContains (Join-Path $script:EvidenceDir "after.txt") "bundle_id=$bundle" "received bundle not durable in spool"
        Write-Host "G6_STEP_PASS step=receive node_id=$node peer_id=$expectedPeer bundle_id=$bundle"
    }

    "send" {
        if ($StepArgs.Count -lt 5 -or $StepArgs.Count -gt 6) {
            Show-Usage
            exit 2
        }

        $peer = $StepArgs[0]
        $node = $StepArgs[1]
        $expectedPeer = $StepArgs[2]
        $spool = $StepArgs[3]
        $bundle = $StepArgs[4]
        $evidence = if ($StepArgs.Count -eq 6) { $StepArgs[5] } else { $null }

        Prepare-Evidence "send" $evidence
        Write-Metadata "send" @(
            "peer_addr=$peer",
            "node_id=$node",
            "expected_peer_node=$expectedPeer",
            "spool=$spool",
            "expected_bundle_id=$bundle"
        )
        Snapshot-Spool "before" $spool
        Require-LogContains (Join-Path $script:EvidenceDir "before.txt") "bundle_id=$bundle" "expected bundle missing before custody send"
        Invoke-Logged $script:LabBin @("custody-send", $peer, $node, "-", $spool)
        Snapshot-Spool "after" $spool
        Require-LogContains $script:LogFile "authenticated peer_id=$expectedPeer" "unexpected custody receiver identity"

        $after = Get-Content -Raw (Join-Path $script:EvidenceDir "after.txt")
        if ($after -match [Regex]::Escape("bundle_id=$bundle")) {
            throw "bundle still present after custody ACK"
        }

        Write-Host "G6_STEP_PASS step=send node_id=$node peer_id=$expectedPeer bundle_id=$bundle"
    }

    "egress" {
        if ($StepArgs.Count -lt 3 -or $StepArgs.Count -gt 4) {
            Show-Usage
            exit 2
        }

        $bind = $StepArgs[0]
        $node = $StepArgs[1]
        $expectedPeer = $StepArgs[2]
        $evidence = if ($StepArgs.Count -eq 4) { $StepArgs[3] } else { $null }

        Prepare-Evidence "egress" $evidence
        Write-Metadata "egress" @(
            "bind_addr=$bind",
            "node_id=$node",
            "expected_peer_node=$expectedPeer"
        )
        Invoke-Logged $script:EgressBin @("server", $bind, $node, "-")
        Require-LogContains $script:LogFile "authenticated peer node_id=$expectedPeer" "unexpected dispatch peer identity"
        Require-LogContains $script:LogFile "request served" "egress did not serve request"
        Write-Host "G6_STEP_PASS step=egress node_id=$node peer_id=$expectedPeer"
    }

    "dispatch" {
        if ($StepArgs.Count -lt 8 -or $StepArgs.Count -gt 9) {
            Show-Usage
            exit 2
        }

        $peer = $StepArgs[0]
        $node = $StepArgs[1]
        $expectedPeer = $StepArgs[2]
        $requestSpool = $StepArgs[3]
        $requestBundle = $StepArgs[4]
        $returnSpool = $StepArgs[5]
        $returnBundle = $StepArgs[6]
        $returnTtl = $StepArgs[7]
        $evidence = if ($StepArgs.Count -eq 9) { $StepArgs[8] } else { $null }

        Prepare-Evidence "dispatch" $evidence
        Write-Metadata "dispatch" @(
            "egress_addr=$peer",
            "node_id=$node",
            "expected_peer_node=$expectedPeer",
            "request_spool=$requestSpool",
            "request_bundle_id=$requestBundle",
            "return_spool=$returnSpool",
            "return_bundle_id=$returnBundle",
            "return_ttl_secs=$returnTtl"
        )
        Snapshot-Spool "request_before" $requestSpool
        Snapshot-Spool "return_before" $returnSpool
        Require-LogContains (Join-Path $script:EvidenceDir "request_before.txt") "bundle_id=$requestBundle" "request bundle missing before dispatch"
        Invoke-Logged $script:LabBin @("dispatch", $peer, $node, "-", $requestSpool, $returnSpool, $returnBundle, $returnTtl)
        Snapshot-Spool "request_after" $requestSpool
        Snapshot-Spool "return_after" $returnSpool
        Require-LogContains $script:LogFile "egress peer_id=$expectedPeer" "unexpected egress identity"
        Require-LogContains (Join-Path $script:EvidenceDir "return_after.txt") "bundle_id=$returnBundle" "return bundle not persisted"

        $requestAfter = Get-Content -Raw (Join-Path $script:EvidenceDir "request_after.txt")
        if ($requestAfter -match [Regex]::Escape("bundle_id=$requestBundle")) {
            throw "request bundle still present after terminal dispatch"
        }

        Write-Host "G6_STEP_PASS step=dispatch node_id=$node egress_peer=$expectedPeer return_bundle_id=$returnBundle"
    }

    "inspect" {
        if ($StepArgs.Count -lt 2 -or $StepArgs.Count -gt 3) {
            Show-Usage
            exit 2
        }

        $spool = $StepArgs[0]
        $expected = $StepArgs[1]
        $evidence = if ($StepArgs.Count -eq 3) { $StepArgs[2] } else { $null }

        Prepare-Evidence "inspect" $evidence
        Write-Metadata "inspect" @("spool=$spool", "expected=$expected")
        Snapshot-Spool "current" $spool
        Copy-Item (Join-Path $script:EvidenceDir "current.txt") $script:LogFile

        if ($expected -eq "empty") {
            Require-LogContains $script:LogFile "bundles=0" "spool is not empty"
        } else {
            Require-LogContains $script:LogFile "bundle_id=$expected" "expected bundle missing from spool"
        }

        Write-Host "G6_STEP_PASS step=inspect expected=$expected"
    }

    "show-result" {
        if ($StepArgs.Count -lt 3 -or $StepArgs.Count -gt 4) {
            Show-Usage
            exit 2
        }

        $spool = $StepArgs[0]
        $bundle = $StepArgs[1]
        $request = $StepArgs[2]
        $evidence = if ($StepArgs.Count -eq 4) { $StepArgs[3] } else { $null }

        Prepare-Evidence "show-result" $evidence
        Write-Metadata "show-result" @(
            "spool=$spool",
            "expected_bundle_id=$bundle",
            "expected_request_id=$request"
        )
        Snapshot-Spool "before" $spool
        Invoke-Logged $script:LabBin @("show-result", $spool)
        Require-LogContains $script:LogFile "bundle_id=$bundle request_id=$request status=Ok" "expected return result missing"
        Write-Host "G6_STEP_PASS step=show-result bundle_id=$bundle request_id=$request"
    }
}

Finalize-Evidence
Write-Host "evidence=$script:EvidenceDir"
