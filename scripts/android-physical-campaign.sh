#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  scripts/android-physical-campaign.sh prepare <serial_a> <serial_b> <package> [apk] [evidence_dir]
  scripts/android-physical-campaign.sh launch <serial> <package> <gatt|rfcomm|nfc|hotspot> [evidence_dir]
  scripts/android-physical-campaign.sh trace <serial> <package> <audio|accelerometer> [duration_ms] [evidence_dir]
  scripts/android-physical-campaign.sh collect <serial_a> <serial_b> <package> [evidence_dir]
  scripts/android-physical-campaign.sh summarize <evidence_dir>
  scripts/android-physical-campaign.sh readiness <evidence_dir>
  scripts/android-physical-campaign.sh --self-test

Environment:
  SP3_ADB_BIN                       adb executable, default: adb
  SP3_ALLOW_NON_PHYSICAL           1 allows emulator/qemu devices, default: 0
  SP3_GRANT_RUNTIME_PERMISSIONS    1 grants declared runtime permissions, default: 0
  SP3_ENABLE_RADIOS                1 enables Wi-Fi/Bluetooth/location best-effort, default: 0
  SP3_CAMPAIGN_ID                  stable campaign identifier; generated when omitted

This harness prepares and collects physical evidence.
It never upgrades evidence to PHYSICAL_DEVICE by itself.
EOF
}

ADB="${SP3_ADB_BIN:-adb}"
ALLOW_NON_PHYSICAL="${SP3_ALLOW_NON_PHYSICAL:-0}"
GRANT_RUNTIME="${SP3_GRANT_RUNTIME_PERMISSIONS:-0}"
ENABLE_RADIOS="${SP3_ENABLE_RADIOS:-0}"

safe_name() {
  printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_'
}

campaign_id() {
  if [[ -n "${SP3_CAMPAIGN_ID:-}" ]]; then
    printf '%s' "$SP3_CAMPAIGN_ID"
  else
    printf 'sp3-%s' "$(date -u +%Y%m%dT%H%M%SZ)"
  fi
}

default_evidence_dir() {
  local id
  id="$(safe_name "$(campaign_id)")"
  printf 'evidence/android-physical-campaign-%s' "$id"
}

adb_run() {
  local serial="$1"
  shift
  "$ADB" -s "$serial" "$@"
}

require_command() {
  command -v "$1" >/dev/null || {
    echo "required command not found: $1" >&2
    exit 2
  }
}

device_prop() {
  local serial="$1"
  local key="$2"
  adb_run "$serial" shell getprop "$key" | tr -d '\r'
}

api_level() {
  device_prop "$1" ro.build.version.sdk
}

is_qemu_device() {
  local serial="$1"
  local qemu
  qemu="$(device_prop "$serial" ro.kernel.qemu)"
  [[ "$qemu" == "1" ]]
}

require_online_device() {
  local serial="$1"
  local state
  state="$(adb_run "$serial" get-state | tr -d '\r')"
  [[ "$state" == "device" ]] || {
    echo "$serial is not online (state=$state)" >&2
    return 1
  }
}

require_physical_candidate() {
  local serial="$1"
  if is_qemu_device "$serial"; then
    if [[ "$ALLOW_NON_PHYSICAL" != "1" ]]; then
      echo "$serial is an emulator/qemu device; physical campaign refuses it" >&2
      return 1
    fi
    echo "PHYSICAL_PREFLIGHT warning=non_physical_allowed serial=$serial"
  fi
}

package_installed() {
  local serial="$1"
  local package="$2"
  adb_run "$serial" shell pm path "$package" >/dev/null 2>&1
}

grant_if_declared() {
  local serial="$1"
  local package="$2"
  local permission="$3"

  if ! adb_run "$serial" shell dumpsys package "$package"       | tr -d '\r'       | grep -Fq "$permission"; then
    return 0
  fi

  adb_run "$serial" shell pm grant "$package" "$permission"     >/dev/null 2>&1 || true
}

grant_runtime_permissions() {
  local serial="$1"
  local package="$2"
  local api
  api="$(api_level "$serial")"

  grant_if_declared "$serial" "$package" android.permission.RECORD_AUDIO
  grant_if_declared "$serial" "$package" android.permission.CAMERA

  if (( api >= 31 )); then
    grant_if_declared "$serial" "$package" android.permission.BLUETOOTH_SCAN
    grant_if_declared "$serial" "$package" android.permission.BLUETOOTH_CONNECT
    grant_if_declared "$serial" "$package" android.permission.BLUETOOTH_ADVERTISE
  else
    grant_if_declared "$serial" "$package" android.permission.ACCESS_FINE_LOCATION
  fi

  if (( api >= 33 )); then
    grant_if_declared "$serial" "$package" android.permission.NEARBY_WIFI_DEVICES
  fi

  if (( api >= 37 )); then
    grant_if_declared "$serial" "$package" android.permission.ACCESS_LOCAL_NETWORK
  fi
}

enable_radios_best_effort() {
  local serial="$1"

  adb_run "$serial" shell svc wifi enable >/dev/null 2>&1 || true
  adb_run "$serial" shell cmd bluetooth_manager enable >/dev/null 2>&1 || true

  if ! adb_run "$serial" shell cmd location set-location-enabled true       >/dev/null 2>&1; then
    adb_run "$serial" shell settings put secure location_mode 3       >/dev/null 2>&1 || true
  fi
}

snapshot_device() {
  local serial="$1"
  local label="$2"
  local package="$3"
  local out="$4"

  mkdir -p "$out"

  adb_run "$serial" shell getprop >"$out/$label-getprop.txt"
  adb_run "$serial" shell pm list features >"$out/$label-features.txt"
  adb_run "$serial" shell dumpsys package "$package"     >"$out/$label-package.txt" || true
  adb_run "$serial" shell dumpsys battery >"$out/$label-battery.txt" || true
  adb_run "$serial" shell dumpsys bluetooth_manager     >"$out/$label-bluetooth.txt" || true
  adb_run "$serial" shell dumpsys nfc >"$out/$label-nfc.txt" || true
  adb_run "$serial" shell dumpsys wifi >"$out/$label-wifi.txt" || true
  adb_run "$serial" shell dumpsys connectivity     >"$out/$label-connectivity.txt" || true
  adb_run "$serial" shell ip addr >"$out/$label-ip-addr.txt" || true
  adb_run "$serial" shell ip route >"$out/$label-ip-route.txt" || true
  adb_run "$serial" shell settings get secure location_mode     >"$out/$label-location-mode.txt" || true
  adb_run "$serial" logcat -d -v threadtime     >"$out/$label-logcat.txt" || true
}

write_device_meta() {
  local serial="$1"
  local label="$2"
  local package="$3"
  local out="$4"

  {
    echo "campaign_id=$(campaign_id)"
    echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "role_label=$label"
    echo "serial=$serial"
    echo "package=$package"
    echo "api=$(api_level "$serial")"
    echo "qemu=$(device_prop "$serial" ro.kernel.qemu)"
    echo "manufacturer=$(device_prop "$serial" ro.product.manufacturer)"
    echo "model=$(device_prop "$serial" ro.product.model)"
    echo "device=$(device_prop "$serial" ro.product.device)"
    echo "fingerprint=$(device_prop "$serial" ro.build.fingerprint)"
    echo "hardware=$(device_prop "$serial" ro.hardware)"
    echo "boot_serial=$(device_prop "$serial" ro.boot.serialno)"
    echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  } >"$out/$label-metadata.txt"
}

prepare_device() {
  local serial="$1"
  local label="$2"
  local package="$3"
  local apk="$4"
  local out="$5"

  require_online_device "$serial"
  require_physical_candidate "$serial"

  if [[ -n "$apk" ]]; then
    test -f "$apk"
    adb_run "$serial" install -r "$apk"       | tee "$out/$label-install.txt"
  fi

  package_installed "$serial" "$package" || {
    echo "$package is not installed on $serial" >&2
    return 1
  }

  if [[ "$GRANT_RUNTIME" == "1" ]]; then
    grant_runtime_permissions "$serial" "$package"
  fi

  if [[ "$ENABLE_RADIOS" == "1" ]]; then
    enable_radios_best_effort "$serial"
  fi

  write_device_meta "$serial" "$label" "$package" "$out"
  snapshot_device "$serial" "$label" "$package" "$out"

  echo "PHYSICAL_PREFLIGHT_PASS serial=$serial label=$label qemu=$(device_prop "$serial" ro.kernel.qemu)"
}

component_for_carrier() {
  case "$1" in
    gatt) printf '.GattCourtActivity' ;;
    rfcomm) printf '.RfcommCourtActivity' ;;
    nfc) printf '.NfcCourtActivity' ;;
    hotspot) printf '.HotspotCourtActivity' ;;
    *) return 1 ;;
  esac
}

launch_court() {
  local serial="$1"
  local package="$2"
  local carrier="$3"
  local out="$4"
  local activity
  activity="$(component_for_carrier "$carrier")" || {
    echo "unsupported carrier: $carrier" >&2
    exit 2
  }

  require_online_device "$serial"
  package_installed "$serial" "$package" || {
    echo "$package is not installed on $serial" >&2
    exit 1
  }

  mkdir -p "$out"
  adb_run "$serial" shell am start -W     -n "$package/$activity"     | tee "$out/launch-$(safe_name "$serial")-$carrier.txt"

  echo "PHYSICAL_COURT_LAUNCHED serial=$serial carrier=$carrier component=$activity"
  echo "Operator must complete the real physical interaction in the app."
}

pull_evidence_dir() {
  local serial="$1"
  local label="$2"
  local package="$3"
  local out="$4"
  local remote="/sdcard/Android/data/$package/files/evidence"
  local dest="$out/$label-app-evidence"

  mkdir -p "$dest"

  if adb_run "$serial" shell test -d "$remote"; then
    if adb_run "$serial" pull "$remote/." "$dest"         >"$out/$label-pull.txt" 2>&1; then
      return 0
    fi
  fi

  # Debug Recovery Lab fallback: app UID can read its own external files dir.
  if adb_run "$serial" shell run-as "$package" sh -c       "test -d '$remote'"; then
    local list_file="$out/$label-evidence-list.txt"
    adb_run "$serial" shell run-as "$package" sh -c       "find '$remote' -maxdepth 1 -type f -name '*.txt' -print"       | tr -d '\r' >"$list_file"

    while IFS= read -r remote_file; do
      [[ -n "$remote_file" ]] || continue
      local base
      base="$(basename "$remote_file")"
      adb_run "$serial" exec-out run-as "$package" cat "$remote_file"         >"$dest/$base"
    done <"$list_file"
  fi
}

collect_device() {
  local serial="$1"
  local label="$2"
  local package="$3"
  local out="$4"

  require_online_device "$serial"
  write_device_meta "$serial" "$label" "$package" "$out"
  snapshot_device "$serial" "$label" "$package" "$out"
  pull_evidence_dir "$serial" "$label" "$package" "$out"
}

hash_evidence() {
  local out="$1"
  (
    cd "$out"
    find . -type f ! -name SHA256SUMS -print0       | sort -z       | xargs -0 sha256sum >SHA256SUMS
  )
}

summarize_evidence() {
  local out="$1"
  python scripts/summarize-physical-evidence.py "$out"     | tee "$out/physical-summary.txt"
  python scripts/summarize-physical-evidence.py --json "$out"     >"$out/physical-summary.json"
}
readiness_evidence() {
  local out="$1"
  python scripts/physical-gate-readiness.py "$out" \
    | tee "$out/gate-readiness.txt"
  python scripts/physical-gate-readiness.py --json "$out" \
    >"$out/gate-readiness.json"
}


cmd_prepare() {
  [[ $# -ge 4 && $# -le 6 ]] || { usage; exit 2; }

  local serial_a="$2"
  local serial_b="$3"
  local package="$4"
  local apk="${5:-}"
  local out="${6:-$(default_evidence_dir)}"

  [[ "$serial_a" != "$serial_b" ]] || {
    echo "physical campaign requires two distinct serials" >&2
    exit 2
  }

  mkdir -p "$out"
  prepare_device "$serial_a" device-a "$package" "$apk" "$out"
  prepare_device "$serial_b" device-b "$package" "$apk" "$out"

  {
    echo "campaign_id=$(campaign_id)"
    echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "serial_a=$serial_a"
    echo "serial_b=$serial_b"
    echo "package=$package"
    echo "allow_non_physical=$ALLOW_NON_PHYSICAL"
    echo "grant_runtime_permissions=$GRANT_RUNTIME"
    echo "enable_radios=$ENABLE_RADIOS"
    echo "evidence_level=CANDIDATE_PHYSICAL_CAMPAIGN"
    echo "note=Preflight only; physical PASS requires carrier court evidence."
  } >"$out/campaign-metadata.txt"

  hash_evidence "$out"
  echo "PHYSICAL_CAMPAIGN_PREPARED campaign=$(campaign_id) evidence=$out"
}

cmd_launch() {
  [[ $# -ge 4 && $# -le 5 ]] || { usage; exit 2; }
  local serial="$2"
  local package="$3"
  local carrier="$4"
  local out="${5:-$(default_evidence_dir)}"

  launch_court "$serial" "$package" "$carrier" "$out"
}

cmd_trace() {
  [[ $# -ge 4 && $# -le 6 ]] || { usage; exit 2; }

  local serial="$2"
  local package="$3"
  local mode="$4"
  local duration_ms="${5:-4000}"
  local out="${6:-$(default_evidence_dir)}"
  local trace_out="$out/recorded-trace-$(safe_name "$serial")-$mode"

  [[ "$mode" == "audio" || "$mode" == "accelerometer" ]] || {
    echo "trace mode must be audio or accelerometer" >&2
    exit 2
  }

  require_online_device "$serial"
  require_physical_candidate "$serial"
  package_installed "$serial" "$package" || {
    echo "$package is not installed on $serial" >&2
    exit 1
  }

  mkdir -p "$out"
  bash scripts/android-recorded-trace-capture.sh \
    "$serial" \
    "$package" \
    "$mode" \
    "$duration_ms" \
    "$trace_out"

  {
    echo "campaign_id=$(campaign_id)"
    echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "serial=$serial"
    echo "qemu=$(device_prop "$serial" ro.kernel.qemu)"
    echo "mode=$mode"
    echo "trace_dir=$trace_out"
    echo "result=PASS"
    echo "evidence_level=CANDIDATE_PHYSICAL_TRACE"
    echo "note=Physical-candidate wrapper only; replay metadata remains authoritative."
  } >"$out/trace-$(safe_name "$serial")-$mode-physical.txt"

  readiness_evidence "$out"
  hash_evidence "$out"

  echo "PHYSICAL_TRACE_CAMPAIGN_PASS serial=$serial mode=$mode evidence=$trace_out"
}

cmd_collect() {
  [[ $# -ge 4 && $# -le 5 ]] || { usage; exit 2; }
  local serial_a="$2"
  local serial_b="$3"
  local package="$4"
  local out="${5:-$(default_evidence_dir)}"

  mkdir -p "$out"
  collect_device "$serial_a" device-a "$package" "$out"
  collect_device "$serial_b" device-b "$package" "$out"
  summarize_evidence "$out"
  readiness_evidence "$out"
  hash_evidence "$out"

  echo "PHYSICAL_CAMPAIGN_COLLECTED campaign=$(campaign_id) evidence=$out"
  echo "Review original evidence + topology/range/energy observations before closing any physical gate."
}

self_test() {
  [[ "$(safe_name 'a b/c')" == "a_b_c" ]]
  [[ "$(component_for_carrier gatt)" == ".GattCourtActivity" ]]
  [[ "$(component_for_carrier rfcomm)" == ".RfcommCourtActivity" ]]
  [[ "$(component_for_carrier nfc)" == ".NfcCourtActivity" ]]
  [[ "$(component_for_carrier hotspot)" == ".HotspotCourtActivity" ]]
  [[ "$(safe_name 'audio trace')" == "audio_trace" ]]
  if component_for_carrier invalid >/dev/null 2>&1; then
    echo "invalid carrier unexpectedly accepted" >&2
    exit 1
  fi

  echo "ANDROID_PHYSICAL_CAMPAIGN_SELF_TEST_PASS"
}

main() {
  require_command bash

  if [[ "${1:-}" == "--self-test" ]]; then
    self_test
    exit 0
  fi

  [[ $# -ge 1 ]] || { usage; exit 2; }

  require_command "$ADB"
  require_command sha256sum
  require_command python

  case "$1" in
    prepare) cmd_prepare "$@" ;;
    launch) cmd_launch "$@" ;;
    trace) cmd_trace "$@" ;;
    collect) cmd_collect "$@" ;;
    summarize)
      [[ $# -eq 2 ]] || { usage; exit 2; }
      summarize_evidence "$2"
      ;;
    readiness)
      [[ $# -eq 2 ]] || { usage; exit 2; }
      readiness_evidence "$2"
      ;;
    *) usage; exit 2 ;;
  esac
}

main "$@"
