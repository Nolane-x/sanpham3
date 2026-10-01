#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  scripts/android-recorded-signal-court.sh <emitter_serial> <capture_serial> <package> <audio|vibration> <payload_hex> [evidence_dir]

Environment:
  SP3_ADB_BIN                 adb executable, default: adb
  SP3_ALLOW_NON_PHYSICAL      1 allows qemu/emulator devices, default: 0
  SP3_EMIT_DELAY_MS           emitter pre-delay after launch, default: 1000
  SP3_CAPTURE_POSTROLL_MS     capture tail after signal, default: 1500
  SP3_SIGNAL_TIMEOUT          overall poll timeout seconds, default: derived
  SP3_SET_MEDIA_VOLUME        1 sets media stream to maximum for audio, default: 0

The court produces candidate physical recorded-signal evidence.
It does not close the physical gate automatically.
EOF
}

[[ $# -ge 5 && $# -le 6 ]] || { usage; exit 2; }

EMITTER="$1"
CAPTURE="$2"
PACKAGE="$3"
MODE="$4"
PAYLOAD_HEX="$5"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVIDENCE="${6:-evidence/f3-recorded-signal-$MODE-$STAMP}"
ADB="${SP3_ADB_BIN:-adb}"
ALLOW_NON_PHYSICAL="${SP3_ALLOW_NON_PHYSICAL:-0}"
EMIT_DELAY_MS="${SP3_EMIT_DELAY_MS:-1000}"
POSTROLL_MS="${SP3_CAPTURE_POSTROLL_MS:-1500}"

[[ "$EMITTER" != "$CAPTURE" ]] || {
  echo "emitter and capture devices must be distinct" >&2
  exit 2
}
[[ "$MODE" == "audio" || "$MODE" == "vibration" ]] || {
  echo "mode must be audio or vibration" >&2
  exit 2
}
[[ "$PAYLOAD_HEX" =~ ^[0-9a-fA-F]+$ ]] || {
  echo "payload_hex must be hexadecimal" >&2
  exit 2
}
(( ${#PAYLOAD_HEX} % 2 == 0 )) || {
  echo "payload_hex must contain an even number of digits" >&2
  exit 2
}
[[ "$EMIT_DELAY_MS" =~ ^[0-9]+$ ]] || exit 2
[[ "$POSTROLL_MS" =~ ^[0-9]+$ ]] || exit 2
(( EMIT_DELAY_MS <= 10000 )) || exit 2

mkdir -p "$EVIDENCE"
command -v "$ADB" >/dev/null
command -v cargo >/dev/null
command -v sha256sum >/dev/null

adb_run() {
  local serial="$1"
  shift
  "$ADB" -s "$serial" "$@"
}

device_prop() {
  adb_run "$1" shell getprop "$2" | tr -d '\r'
}

require_device() {
  local serial="$1"
  local state
  state="$(adb_run "$serial" get-state | tr -d '\r')"
  [[ "$state" == "device" ]] || {
    echo "$serial is not online" >&2
    return 1
  }

  adb_run "$serial" shell pm path "$PACKAGE" >/dev/null

  local qemu
  qemu="$(device_prop "$serial" ro.kernel.qemu)"
  if [[ "$qemu" == "1" && "$ALLOW_NON_PHYSICAL" != "1" ]]; then
    echo "$serial is qemu/emulator; physical court refuses it" >&2
    return 1
  fi
}

require_device "$EMITTER"
require_device "$CAPTURE"

PAYLOAD_BYTES=$(( ${#PAYLOAD_HEX} / 2 ))
BITS=$(( PAYLOAD_BYTES * 8 ))

if [[ "$MODE" == "audio" ]]; then
  (( PAYLOAD_BYTES <= 64 )) || {
    echo "audio emitter supports at most 64 payload bytes" >&2
    exit 2
  }
  SIGNAL_MS=$(( BITS * 20 ))
  CAPTURE_MODE="audio"
  CAPTURE_FILE="$EVIDENCE/capture.wav"
  SOURCE_FILE="files/recorded-traces/latest-audio.wav"
  adb_run "$CAPTURE" shell pm grant     "$PACKAGE" android.permission.RECORD_AUDIO >/dev/null

  if [[ "${SP3_SET_MEDIA_VOLUME:-0}" == "1" ]]; then
    adb_run "$EMITTER" shell media volume       --stream 3 --set 15 >/dev/null 2>&1 || true
  fi
else
  (( PAYLOAD_BYTES <= 8 )) || {
    echo "vibration emitter supports at most 8 payload bytes" >&2
    exit 2
  }
  SIGNAL_MS=$(( BITS * 400 ))
  CAPTURE_MODE="accelerometer"
  CAPTURE_FILE="$EVIDENCE/capture.csv"
  SOURCE_FILE="files/recorded-traces/latest-accelerometer.csv"
fi

CAPTURE_MS=$(( EMIT_DELAY_MS + SIGNAL_MS + POSTROLL_MS ))
(( CAPTURE_MS >= 250 && CAPTURE_MS <= 30000 )) || {
  echo "derived capture duration $CAPTURE_MS ms is outside 250..30000" >&2
  exit 2
}

TIMEOUT_SECONDS="${SP3_SIGNAL_TIMEOUT:-$(( CAPTURE_MS / 1000 + 35 ))}"

adb_run "$CAPTURE" logcat -c
adb_run "$EMITTER" logcat -c

CAPTURE_COMPONENT="$PACKAGE/.RecordedTraceCaptureActivity"
EMITTER_COMPONENT="$PACKAGE/.PhysicalSignalEmitterActivity"

adb_run "$CAPTURE" shell am start -W   -n "$CAPTURE_COMPONENT"   --es dev.nolane.sanpham3.recoverylab.TRACE_CAPTURE_MODE "$CAPTURE_MODE"   --ei dev.nolane.sanpham3.recoverylab.TRACE_CAPTURE_DURATION_MS "$CAPTURE_MS"   | tee "$EVIDENCE/capture-am-start.txt"

adb_run "$EMITTER" shell am start -W   -n "$EMITTER_COMPONENT"   --es dev.nolane.sanpham3.recoverylab.SIGNAL_EMIT_MODE "$MODE"   --es dev.nolane.sanpham3.recoverylab.SIGNAL_EMIT_HEX "$PAYLOAD_HEX"   --ei dev.nolane.sanpham3.recoverylab.SIGNAL_EMIT_DELAY_MS "$EMIT_DELAY_MS"   | tee "$EVIDENCE/emitter-am-start.txt"

EMIT_PASS=""
CAPTURE_PASS=""
for _ in $(seq 1 "$TIMEOUT_SECONDS"); do
  EMIT_PASS="$(
    adb_run "$EMITTER" logcat -d -v brief -s SP3SignalEmit:I '*:S'       | tr -d '\r'       | grep 'PHYSICAL_SIGNAL_EMIT_PASS'       | grep "mode=$MODE"       | tail -n 1       || true
  )"

  CAPTURE_PASS="$(
    adb_run "$CAPTURE" logcat -d -v brief -s SP3TraceCapture:I '*:S'       | tr -d '\r'       | grep 'RECORDED_TRACE_PASS'       | grep "mode=$CAPTURE_MODE"       | tail -n 1       || true
  )"

  if [[ -n "$EMIT_PASS" && -n "$CAPTURE_PASS" ]]; then
    break
  fi

  if adb_run "$EMITTER" logcat -d -v brief -s SP3SignalEmit:I '*:S'       | grep -q 'PHYSICAL_SIGNAL_EMIT_FAIL'; then
    break
  fi
  if adb_run "$CAPTURE" logcat -d -v brief -s SP3TraceCapture:I '*:S'       | grep -q 'RECORDED_TRACE_FAIL'; then
    break
  fi
  sleep 1
done

adb_run "$EMITTER" logcat -d -v threadtime -s SP3SignalEmit:I '*:S'   >"$EVIDENCE/emitter-logcat.txt" || true
adb_run "$CAPTURE" logcat -d -v threadtime -s SP3TraceCapture:I '*:S'   >"$EVIDENCE/capture-logcat.txt" || true

if [[ -z "$EMIT_PASS" || -z "$CAPTURE_PASS" ]]; then
  echo "emitter/capture court did not complete" >&2
  cat "$EVIDENCE/emitter-logcat.txt" >&2 || true
  cat "$EVIDENCE/capture-logcat.txt" >&2 || true
  exit 1
fi

grep -q 'evidence_level=ANDROID_RUNTIME_EMIT' <<<"$EMIT_PASS"
grep -q 'evidence_level=ANDROID_RUNTIME_CAPTURE' <<<"$CAPTURE_PASS"

adb_run "$CAPTURE" exec-out run-as "$PACKAGE" cat "$SOURCE_FILE"   >"$CAPTURE_FILE"
test -s "$CAPTURE_FILE"

if [[ "$MODE" == "audio" ]]; then
  cargo run -p signal-trace-replay-cli --     acoustic-wav-search     "$CAPTURE_FILE"     "$PAYLOAD_HEX"     0     | tee "$EVIDENCE/replay.txt"
  grep -q 'F3_ACOUSTIC_REPLAY_SEARCH' "$EVIDENCE/replay.txt"
else
  cargo run -p signal-trace-replay-cli --     vibration-csv-search     "$CAPTURE_FILE"     "$PAYLOAD_HEX"     5     | tee "$EVIDENCE/replay.txt"
  grep -q 'F3_VIBRATION_REPLAY_SEARCH' "$EVIDENCE/replay.txt"
fi

grep -q 'bit_errors=0' "$EVIDENCE/replay.txt"

for entry in   "emitter:$EMITTER"   "capture:$CAPTURE"
do
  label="${entry%%:*}"
  serial="${entry#*:}"
  adb_run "$serial" shell getprop >"$EVIDENCE/$label-getprop.txt"
  adb_run "$serial" shell dumpsys battery >"$EVIDENCE/$label-battery.txt" || true
  adb_run "$serial" shell dumpsys audio >"$EVIDENCE/$label-audio.txt" || true
  adb_run "$serial" shell dumpsys sensorservice >"$EVIDENCE/$label-sensors.txt" || true
done

{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "emitter_serial=$EMITTER"
  echo "capture_serial=$CAPTURE"
  echo "emitter_qemu=$(device_prop "$EMITTER" ro.kernel.qemu)"
  echo "capture_qemu=$(device_prop "$CAPTURE" ro.kernel.qemu)"
  echo "emitter_model=$(device_prop "$EMITTER" ro.product.model)"
  echo "capture_model=$(device_prop "$CAPTURE" ro.product.model)"
  echo "mode=$MODE"
  echo "payload_hex=$PAYLOAD_HEX"
  echo "bits=$BITS"
  echo "emit_delay_ms=$EMIT_DELAY_MS"
  echo "signal_duration_ms=$SIGNAL_MS"
  echo "capture_duration_ms=$CAPTURE_MS"
  echo "emitter_pass=$EMIT_PASS"
  echo "capture_pass=$CAPTURE_PASS"
  echo "evidence_level=CANDIDATE_PHYSICAL_RECORDED_SIGNAL"
  echo "note=Review physical topology, distance/surface and device provenance before closing F3 recorded-trace gates."
} >"$EVIDENCE/metadata.txt"

(
  cd "$EVIDENCE"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -print0     | sort -z     | xargs -0 sha256sum >SHA256SUMS
)

echo "F3_RECORDED_SIGNAL_COURT_PASS mode=$MODE payload=$PAYLOAD_HEX evidence=$EVIDENCE"
echo "Candidate physical evidence only; review topology/provenance before gate promotion."
