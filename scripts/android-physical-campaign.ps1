param(
  [Parameter(Position = 0)]
  [string]$Command,
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$Rest
)

$ErrorActionPreference = "Stop"
$Adb = if ($env:SP3_ADB_BIN) { $env:SP3_ADB_BIN } else { "adb" }

function Usage {
  Write-Host "usage:"
  Write-Host "  pwsh scripts/android-physical-campaign.ps1 prepare <serial_a> <serial_b> <package> [apk] [evidence_dir]"
  Write-Host "  pwsh scripts/android-physical-campaign.ps1 launch <serial> <package> <gatt|rfcomm|nfc|hotspot> [evidence_dir]"
  Write-Host "  pwsh scripts/android-physical-campaign.ps1 trace <serial> <package> <audio|accelerometer> [duration_ms] [evidence_dir]"
  Write-Host "  pwsh scripts/android-physical-campaign.ps1 collect <serial_a> <serial_b> <package> [evidence_dir]"
  Write-Host "  pwsh scripts/android-physical-campaign.ps1 summarize <evidence_dir>"
  Write-Host "  pwsh scripts/android-physical-campaign.ps1 readiness <evidence_dir>"
  Write-Host "  pwsh scripts/android-physical-campaign.ps1 --self-test"
}

function SafeName([string]$Value) {
  return ($Value -replace "[^A-Za-z0-9._-]", "_")
}

function CampaignId {
  if ($env:SP3_CAMPAIGN_ID) { return $env:SP3_CAMPAIGN_ID }
  return "sp3-" + (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
}

function DefaultEvidence {
  return "evidence/android-physical-campaign-" + (SafeName (CampaignId))
}

function Adb {
  param([string]$Serial, [string[]]$Args)
  $output = & $Adb -s $Serial @Args 2>&1
  if ($LASTEXITCODE -ne 0) {
    throw "adb failed serial=$Serial args=$($Args -join ' ') output=$($output -join ' ')"
  }
  return $output
}

function Prop([string]$Serial, [string]$Key) {
  return ((Adb $Serial @("shell","getprop",$Key)) -join [Environment]::NewLine).Trim()
}

function Online([string]$Serial) {
  $state = ((Adb $Serial @("get-state")) -join [Environment]::NewLine).Trim()
  if ($state -ne "device") { throw "$Serial is not online" }
}

function PhysicalCandidate([string]$Serial) {
  $qemu = Prop $Serial "ro.kernel.qemu"
  if ($qemu -eq "1" -and $env:SP3_ALLOW_NON_PHYSICAL -ne "1") {
    throw "$Serial is an emulator/qemu device; physical campaign refuses it"
  }
}

function PackageInstalled([string]$Serial, [string]$Package) {
  & $Adb -s $Serial shell pm path $Package *> $null
  return $LASTEXITCODE -eq 0
}

function Snapshot([string]$Serial, [string]$Label, [string]$Package, [string]$Out) {
  New-Item -ItemType Directory -Force -Path $Out | Out-Null

  $captures = @(
    @("getprop.txt", @("shell","getprop")),
    @("features.txt", @("shell","pm","list","features")),
    @("package.txt", @("shell","dumpsys","package",$Package)),
    @("battery.txt", @("shell","dumpsys","battery")),
    @("bluetooth.txt", @("shell","dumpsys","bluetooth_manager")),
    @("nfc.txt", @("shell","dumpsys","nfc")),
    @("wifi.txt", @("shell","dumpsys","wifi")),
    @("connectivity.txt", @("shell","dumpsys","connectivity")),
    @("ip-addr.txt", @("shell","ip","addr")),
    @("ip-route.txt", @("shell","ip","route"))
  )

  foreach ($capture in $captures) {
    $path = Join-Path $Out ("$Label-" + $capture[0])
    try {
      (Adb $Serial $capture[1]) | Set-Content -Path $path -Encoding utf8
    } catch {
      ("capture_failed=" + $_.Exception.Message) | Set-Content -Path $path -Encoding utf8
    }
  }

  $git = "unknown"
  try { $git = (git rev-parse HEAD 2>$null).Trim() } catch {}

  @(
    "campaign_id=$(CampaignId)",
    "timestamp_utc=$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))",
    "serial=$Serial",
    "label=$Label",
    "package=$Package",
    "api=$(Prop $Serial 'ro.build.version.sdk')",
    "qemu=$(Prop $Serial 'ro.kernel.qemu')",
    "manufacturer=$(Prop $Serial 'ro.product.manufacturer')",
    "model=$(Prop $Serial 'ro.product.model')",
    "fingerprint=$(Prop $Serial 'ro.build.fingerprint')",
    "git_commit=$git"
  ) | Set-Content -Path (Join-Path $Out "$Label-metadata.txt") -Encoding utf8
}

function GrantDeclared([string]$Serial, [string]$Package, [string]$Permission) {
  $dump = (& $Adb -s $Serial shell dumpsys package $Package 2>$null) -join [Environment]::NewLine
  if ($dump.Contains($Permission)) {
    & $Adb -s $Serial shell pm grant $Package $Permission *> $null
  }
}

function GrantRuntime([string]$Serial, [string]$Package) {
  $api = [int](Prop $Serial "ro.build.version.sdk")
  GrantDeclared $Serial $Package "android.permission.RECORD_AUDIO"
  GrantDeclared $Serial $Package "android.permission.CAMERA"

  if ($api -ge 31) {
    GrantDeclared $Serial $Package "android.permission.BLUETOOTH_SCAN"
    GrantDeclared $Serial $Package "android.permission.BLUETOOTH_CONNECT"
    GrantDeclared $Serial $Package "android.permission.BLUETOOTH_ADVERTISE"
  } else {
    GrantDeclared $Serial $Package "android.permission.ACCESS_FINE_LOCATION"
  }
  if ($api -ge 33) {
    GrantDeclared $Serial $Package "android.permission.NEARBY_WIFI_DEVICES"
  }
  if ($api -ge 37) {
    GrantDeclared $Serial $Package "android.permission.ACCESS_LOCAL_NETWORK"
  }
}

function PrepareOne([string]$Serial,[string]$Label,[string]$Package,[string]$Apk,[string]$Out) {
  Online $Serial
  PhysicalCandidate $Serial

  if ($Apk) {
    if (-not (Test-Path $Apk)) { throw "APK not found: $Apk" }
    & $Adb -s $Serial install -r $Apk | Set-Content (Join-Path $Out "$Label-install.txt")
    if ($LASTEXITCODE -ne 0) { throw "APK install failed on $Serial" }
  }

  if (-not (PackageInstalled $Serial $Package)) {
    throw "$Package is not installed on $Serial"
  }

  if ($env:SP3_GRANT_RUNTIME_PERMISSIONS -eq "1") {
    GrantRuntime $Serial $Package
  }

  if ($env:SP3_ENABLE_RADIOS -eq "1") {
    & $Adb -s $Serial shell svc wifi enable *> $null
    & $Adb -s $Serial shell cmd bluetooth_manager enable *> $null
    & $Adb -s $Serial shell cmd location set-location-enabled true *> $null
  }

  Snapshot $Serial $Label $Package $Out
  Write-Host "PHYSICAL_PREFLIGHT_PASS serial=$Serial label=$Label qemu=$(Prop $Serial 'ro.kernel.qemu')"
}

function Activity([string]$Carrier) {
  switch ($Carrier) {
    "gatt" { return ".GattCourtActivity" }
    "rfcomm" { return ".RfcommCourtActivity" }
    "nfc" { return ".NfcCourtActivity" }
    "hotspot" { return ".HotspotCourtActivity" }
    default { throw "unsupported carrier: $Carrier" }
  }
}

function PullEvidence([string]$Serial,[string]$Label,[string]$Package,[string]$Out) {
  $remote = "/sdcard/Android/data/$Package/files/evidence"
  $dest = Join-Path $Out "$Label-app-evidence"
  New-Item -ItemType Directory -Force -Path $dest | Out-Null

  & $Adb -s $Serial pull ("$remote/.") $dest *> (Join-Path $Out "$Label-pull.txt")
  if ($LASTEXITCODE -ne 0) {
    Write-Host "warning: direct evidence pull failed for $Serial; use app Downloads copy if device policy blocks Android/data"
  }
}

function HashEvidence([string]$Out) {
  $root = (Resolve-Path $Out).Path
  $hashPath = Join-Path $root "SHA256SUMS"
  $lines = @()
  foreach ($file in (Get-ChildItem $root -Recurse -File | Sort-Object FullName)) {
    if ($file.FullName -eq $hashPath) { continue }
    $hash = (Get-FileHash -Algorithm SHA256 $file.FullName).Hash.ToLowerInvariant()
    $relative = $file.FullName.Substring($root.Length).TrimStart([char]92,[char]47).Replace([char]92,[char]47)
    $lines += "$hash  ./$relative"
  }
  $lines | Set-Content -Path $hashPath -Encoding ascii
}

function Summarize([string]$Out) {
  python scripts/summarize-physical-evidence.py $Out |
    Set-Content -Path (Join-Path $Out "physical-summary.txt") -Encoding utf8
  if ($LASTEXITCODE -ne 0) { throw "text summary failed" }

  python scripts/summarize-physical-evidence.py --json $Out |
    Set-Content -Path (Join-Path $Out "physical-summary.json") -Encoding utf8
  if ($LASTEXITCODE -ne 0) { throw "JSON summary failed" }
}

function Readiness([string]$Out) {
  python scripts/physical-gate-readiness.py $Out |
    Set-Content -Path (Join-Path $Out "gate-readiness.txt") -Encoding utf8
  if ($LASTEXITCODE -ne 0) { throw "gate readiness text report failed" }

  python scripts/physical-gate-readiness.py --json $Out |
    Set-Content -Path (Join-Path $Out "gate-readiness.json") -Encoding utf8
  if ($LASTEXITCODE -ne 0) { throw "gate readiness JSON report failed" }
}

function AdbExecOutToFile([string]$Serial,[string[]]$Args,[string]$Path) {
  $psi = [System.Diagnostics.ProcessStartInfo]::new()
  $psi.FileName = $Adb
  $psi.UseShellExecute = $false
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.ArgumentList.Add("-s")
  $psi.ArgumentList.Add($Serial)
  foreach ($arg in $Args) { $psi.ArgumentList.Add($arg) }

  $process = [System.Diagnostics.Process]::new()
  $process.StartInfo = $psi
  [void]$process.Start()

  $stream = [System.IO.File]::Open(
    $Path,
    [System.IO.FileMode]::Create,
    [System.IO.FileAccess]::Write,
    [System.IO.FileShare]::None
  )
  try {
    $process.StandardOutput.BaseStream.CopyTo($stream)
  } finally {
    $stream.Dispose()
  }

  $stderr = $process.StandardError.ReadToEnd()
  $process.WaitForExit()
  if ($process.ExitCode -ne 0) {
    throw "adb exec-out failed serial=$Serial stderr=$stderr"
  }
}

function TraceCapture([string]$Serial,[string]$Package,[string]$Mode,[int]$DurationMs,[string]$Out) {
  if ($Mode -ne "audio" -and $Mode -ne "accelerometer") {
    throw "trace mode must be audio or accelerometer"
  }
  if ($DurationMs -lt 250 -or $DurationMs -gt 30000) {
    throw "duration_ms must be in 250..30000"
  }

  Online $Serial
  PhysicalCandidate $Serial
  if (-not (PackageInstalled $Serial $Package)) {
    throw "$Package is not installed on $Serial"
  }

  New-Item -ItemType Directory -Force -Path $Out | Out-Null
  $traceOut = Join-Path $Out ("recorded-trace-" + (SafeName $Serial) + "-" + $Mode)
  New-Item -ItemType Directory -Force -Path $traceOut | Out-Null

  $expected = if ($env:SP3_TRACE_EXPECTED_HEX) { $env:SP3_TRACE_EXPECTED_HEX } else { "" }
  $startSample = if ($env:SP3_TRACE_START_SAMPLE) { [int]$env:SP3_TRACE_START_SAMPLE } else { 0 }
  if ($expected -and $expected -notmatch '^[0-9a-fA-F]+$') {
    throw "SP3_TRACE_EXPECTED_HEX must be hexadecimal"
  }
  if ($expected -and ($expected.Length % 2 -ne 0)) {
    throw "SP3_TRACE_EXPECTED_HEX must contain an even number of digits"
  }
  if ($expected -and -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo executable is required for exact trace replay"
  }

  if ($Mode -eq "audio") {
    GrantDeclared $Serial $Package "android.permission.RECORD_AUDIO"
    $sourceFile = "files/recorded-traces/latest-audio.wav"
    $captureFile = Join-Path $traceOut "capture.wav"
  } else {
    $sourceFile = "files/recorded-traces/latest-accelerometer.csv"
    $captureFile = Join-Path $traceOut "capture.csv"
  }

  & $Adb -s $Serial logcat -c *> $null
  & $Adb -s $Serial shell am start -W -n "$Package/.RecordedTraceCaptureActivity" --es dev.nolane.sanpham3.recoverylab.TRACE_CAPTURE_MODE $Mode --ei dev.nolane.sanpham3.recoverylab.TRACE_CAPTURE_DURATION_MS $DurationMs |
    Set-Content -Path (Join-Path $traceOut "am-start.txt") -Encoding utf8
  if ($LASTEXITCODE -ne 0) { throw "failed to start recorded trace activity" }

  $timeout = if ($env:SP3_TRACE_TIMEOUT) { [int]$env:SP3_TRACE_TIMEOUT } else { [int]($DurationMs / 1000) + 30 }
  $passLine = ""
  for ($i = 0; $i -lt $timeout; $i++) {
    $lines = & $Adb -s $Serial logcat -d -v brief -s "SP3TraceCapture:I" "*:S"
    $passLine = $lines | Where-Object { $_ -like "*RECORDED_TRACE_PASS*" -and $_ -like "*mode=$Mode*" } | Select-Object -Last 1
    if ($passLine) { break }
    $failed = $lines | Where-Object { $_ -like "*RECORDED_TRACE_FAIL*" }
    if ($failed) { break }
    Start-Sleep -Seconds 1
  }

  (& $Adb -s $Serial logcat -d -v threadtime -s "SP3TraceCapture:I" "*:S") |
    Set-Content -Path (Join-Path $traceOut "trace-logcat.txt") -Encoding utf8
  (& $Adb -s $Serial shell getprop) |
    Set-Content -Path (Join-Path $traceOut "getprop.txt") -Encoding utf8
  (& $Adb -s $Serial shell dumpsys sensorservice) |
    Set-Content -Path (Join-Path $traceOut "sensorservice.txt") -Encoding utf8
  (& $Adb -s $Serial shell dumpsys media.audio_flinger) |
    Set-Content -Path (Join-Path $traceOut "audio-flinger.txt") -Encoding utf8

  if (-not $passLine) { throw "recorded trace capture did not PASS" }
  if (("$passLine") -notlike "*evidence_level=ANDROID_RUNTIME_CAPTURE*") {
    throw "recorded trace PASS has unexpected evidence level"
  }

  AdbExecOutToFile $Serial @("exec-out","run-as",$Package,"cat",$sourceFile) $captureFile
  if ((Get-Item $captureFile).Length -le 0) {
    throw "recorded trace capture file is empty"
  }

  if ($expected) {
    $replayPath = Join-Path $traceOut "replay.txt"
    if ($Mode -eq "audio") {
      $replay = & cargo run -p signal-trace-replay-cli -- acoustic-wav $captureFile $expected 0 $startSample 2>&1
    } else {
      $replay = & cargo run -p signal-trace-replay-cli -- vibration-csv $captureFile $expected 5 $startSample 2>&1
    }
    $replay | Set-Content -Path $replayPath -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw "trace replay failed" }
    if (($replay -join [Environment]::NewLine) -notmatch 'bit_errors=0') {
      throw "trace replay did not achieve zero BER"
    }
  }

  $expectedMetadata = if ($expected) { $expected } else { "none" }
  $git = "unknown"
  try { $git = (git rev-parse HEAD 2>$null).Trim() } catch {}
  $timestamp = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  $qemu = Prop $Serial "ro.kernel.qemu"
  $api = Prop $Serial "ro.build.version.sdk"

  @(
    "timestamp_utc=$timestamp",
    "git_commit=$git",
    "serial=$Serial",
    "qemu=$qemu",
    "api=$api",
    "package=$Package",
    "mode=$Mode",
    "duration_ms=$DurationMs",
    "expected_hex=$expectedMetadata",
    "start_sample=$startSample",
    "capture_pass=$passLine",
    "evidence_level=ANDROID_RUNTIME_CAPTURE"
  ) | Set-Content -Path (Join-Path $traceOut "metadata.txt") -Encoding utf8

  @(
    "campaign_id=$(CampaignId)",
    "timestamp_utc=$timestamp",
    "serial=$Serial",
    "qemu=$qemu",
    "mode=$Mode",
    "trace_dir=$traceOut",
    "result=PASS",
    "evidence_level=CANDIDATE_PHYSICAL_TRACE",
    "note=Physical-candidate wrapper only; replay metadata remains authoritative."
  ) | Set-Content -Path (Join-Path $Out ("trace-" + (SafeName $Serial) + "-" + $Mode + "-physical.txt")) -Encoding utf8

  Readiness $Out
  HashEvidence $Out
  Write-Host "PHYSICAL_TRACE_CAMPAIGN_PASS serial=$Serial mode=$Mode evidence=$traceOut"
}


function SelfTest {
  if ((SafeName "a b/c") -ne "a_b_c") { throw "SafeName self-test failed" }
  if ((Activity "gatt") -ne ".GattCourtActivity") { throw "gatt route failed" }
  if ((Activity "rfcomm") -ne ".RfcommCourtActivity") { throw "rfcomm route failed" }
  if ((Activity "nfc") -ne ".NfcCourtActivity") { throw "nfc route failed" }
  if ((Activity "hotspot") -ne ".HotspotCourtActivity") { throw "hotspot route failed" }

  $caught = $false
  try { Activity "invalid" | Out-Null } catch { $caught = $true }
  if (-not $caught) { throw "invalid carrier was accepted" }

  Write-Host "ANDROID_PHYSICAL_CAMPAIGN_POWERSHELL_SELF_TEST_PASS"
}

if ($Command -eq "--self-test") {
  SelfTest
  exit 0
}

if (-not $Command) {
  Usage
  exit 2
}

if (-not (Get-Command $Adb -ErrorAction SilentlyContinue)) {
  throw "adb executable not found: $Adb"
}

switch ($Command) {
  "prepare" {
    if ($Rest.Count -lt 3 -or $Rest.Count -gt 5) { Usage; exit 2 }
    $a=$Rest[0]; $b=$Rest[1]; $pkg=$Rest[2]
    $apk=if ($Rest.Count -ge 4) { $Rest[3] } else { "" }
    $out=if ($Rest.Count -ge 5) { $Rest[4] } else { DefaultEvidence }
    if ($a -eq $b) { throw "physical campaign requires two distinct serials" }
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    PrepareOne $a "device-a" $pkg $apk $out
    PrepareOne $b "device-b" $pkg $apk $out
    @(
      "campaign_id=$(CampaignId)",
      "timestamp_utc=$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))",
      "serial_a=$a",
      "serial_b=$b",
      "package=$pkg",
      "evidence_level=CANDIDATE_PHYSICAL_CAMPAIGN",
      "note=Preflight only; physical PASS requires carrier court evidence."
    ) | Set-Content -Path (Join-Path $out "campaign-metadata.txt") -Encoding utf8
    HashEvidence $out
    Write-Host "PHYSICAL_CAMPAIGN_PREPARED campaign=$(CampaignId) evidence=$out"
  }
  "launch" {
    if ($Rest.Count -lt 3 -or $Rest.Count -gt 4) { Usage; exit 2 }
    $serial=$Rest[0]; $pkg=$Rest[1]; $carrier=$Rest[2]
    $out=if ($Rest.Count -eq 4) { $Rest[3] } else { DefaultEvidence }
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    $activity=Activity $carrier
    & $Adb -s $serial shell am start -W -n "$pkg/$activity" |
      Set-Content -Path (Join-Path $out "launch-$(SafeName $serial)-$carrier.txt")
    if ($LASTEXITCODE -ne 0) { throw "failed to launch court" }
    Write-Host "PHYSICAL_COURT_LAUNCHED serial=$serial carrier=$carrier component=$activity"
    Write-Host "Operator must complete the real physical interaction in the app."
  }
  "trace" {
    if ($Rest.Count -lt 3 -or $Rest.Count -gt 5) { Usage; exit 2 }
    $serial=$Rest[0]; $pkg=$Rest[1]; $mode=$Rest[2]
    $duration=if ($Rest.Count -ge 4) { [int]$Rest[3] } else { 4000 }
    $out=if ($Rest.Count -ge 5) { $Rest[4] } else { DefaultEvidence }
    TraceCapture $serial $pkg $mode $duration $out
  }
  "collect" {
    if ($Rest.Count -lt 3 -or $Rest.Count -gt 4) { Usage; exit 2 }
    $a=$Rest[0]; $b=$Rest[1]; $pkg=$Rest[2]
    $out=if ($Rest.Count -eq 4) { $Rest[3] } else { DefaultEvidence }
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    Snapshot $a "device-a" $pkg $out
    Snapshot $b "device-b" $pkg $out
    PullEvidence $a "device-a" $pkg $out
    PullEvidence $b "device-b" $pkg $out
    Summarize $out
    Readiness $out
    HashEvidence $out
    Write-Host "PHYSICAL_CAMPAIGN_COLLECTED campaign=$(CampaignId) evidence=$out"
  }
  "summarize" {
    if ($Rest.Count -ne 1) { Usage; exit 2 }
    Summarize $Rest[0]
  }
  "readiness" {
    if ($Rest.Count -ne 1) { Usage; exit 2 }
    Readiness $Rest[0]
  }
  default {
    Usage
    exit 2
  }
}
