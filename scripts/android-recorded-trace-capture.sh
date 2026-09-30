#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  scripts/android-recorded-trace-capture.sh <serial> <package> <audio|accelerometer> [duration_ms] [evidence_dir]

Environment:
  SP3_ADB_BIN                adb executable, default: adb
  SP3_TRACE_TIMEOUT          poll timeout seconds, default: duration/1000 + 30
  SP3_TRACE_EXPECTED_HEX     optional expected payload; enables replay/BER gate
  SP3_TRACE_START_SAMPLE     optional replay start sample, default: 0

Evidence remains ANDROID_RUNTIME_CAPTURE. The script cannot infer whether the
target is an emulator or a physical device.
EOF
}

[[ $# -ge 3 && $# -le 5 ]] || { usage; exit 2; }

SERIAL="$1"
PACKAGE="$2"
MODE="$3"
DURATION_MS="${4:-4000}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVIDENCE="${5:-evidence/android-recorded-trace-$MODE-$STAMP}"
ADB="${SP3_ADB_BIN:-adb}"
EXPECTED_HEX="${SP3_TRACE_EXPECTED_HEX:-}"
START_SAMPLE="${SP3_TRACE_START_SAMPLE:-0}"

[[ "$MODE" == "audio" || "$MODE" == "accelerometer" ]] || {
  echo "mode must be audio or accelerometer" >&2
  exit 2
}
[[ "$DURATION_MS" =~ ^[0-9]+$ ]] || exit 2
(( DURATION_MS >= 250 && DURATION_MS <= 30000 )) || exit 2
[[ "$START_SAMPLE" =~ ^[0-9]+$ ]] || exit 2
if [[ -n "$EXPECTED_HEX" ]]; then
  [[ "$EXPECTED_HEX" =~ ^[0-9a-fA-F]+$ ]] || exit 2
  (( ${#EXPECTED_HEX} % 2 == 0 )) || exit 2
fi

mkdir -p "$EVIDENCE"
command -v "$ADB" >/dev/null
command -v sha256sum >/dev/null

adb_run() {
  "$ADB" -s "$SERIAL" "$@"
}

state="$(adb_run get-state | tr -d '\r')"
[[ "$state" == "device" ]] || {
  echo "$SERIAL is not online" >&2
  exit 1
}
adb_run shell pm path "$PACKAGE" >/dev/null

if [[ "$MODE" == "audio" ]]; then
  adb_run shell pm grant "$PACKAGE" android.permission.RECORD_AUDIO
  SOURCE_FILE="files/recorded-traces/latest-audio.wav"
  CAPTURE_FILE="$EVIDENCE/capture.wav"
else
  SOURCE_FILE="files/recorded-traces/latest-accelerometer.csv"
  CAPTURE_FILE="$EVIDENCE/capture.csv"
fi

adb_run logcat -c

COMPONENT="$PACKAGE/.RecordedTraceCaptureActivity"
adb_run shell am start -W   -n "$COMPONENT"   --es dev.nolane.sanpham3.recoverylab.TRACE_CAPTURE_MODE "$MODE"   --ei dev.nolane.sanpham3.recoverylab.TRACE_CAPTURE_DURATION_MS "$DURATION_MS"   | tee "$EVIDENCE/am-start.txt"

TIMEOUT_SECONDS="${SP3_TRACE_TIMEOUT:-$(( DURATION_MS / 1000 + 30 ))}"
PASS_LINE=""
for _ in $(seq 1 "$TIMEOUT_SECONDS"); do
  PASS_LINE="$(
    adb_run logcat -d -v brief -s SP3TraceCapture:I '*:S'       | tr -d '\r'       | grep 'RECORDED_TRACE_PASS'       | grep "mode=$MODE"       | tail -n 1       || true
  )"
  if [[ -n "$PASS_LINE" ]]; then
    break
  fi

  if adb_run logcat -d -v brief -s SP3TraceCapture:I '*:S'       | grep -q 'RECORDED_TRACE_FAIL'; then
    break
  fi
  sleep 1
done

adb_run logcat -d -v threadtime -s SP3TraceCapture:I '*:S'   >"$EVIDENCE/trace-logcat.txt" || true
adb_run shell getprop >"$EVIDENCE/getprop.txt"
adb_run shell dumpsys sensorservice >"$EVIDENCE/sensorservice.txt" || true
adb_run shell dumpsys media.audio_flinger >"$EVIDENCE/audio-flinger.txt" || true

if [[ -z "$PASS_LINE" ]]; then
  echo "recorded trace capture did not PASS" >&2
  cat "$EVIDENCE/trace-logcat.txt" >&2 || true
  exit 1
fi

grep -q 'evidence_level=ANDROID_RUNTIME_CAPTURE' <<<"$PASS_LINE"

adb_run exec-out run-as "$PACKAGE" cat "$SOURCE_FILE" >"$CAPTURE_FILE"
test -s "$CAPTURE_FILE"

if [[ -n "$EXPECTED_HEX" ]]; then
  if [[ "$MODE" == "audio" ]]; then
    cargo run -p signal-trace-replay-cli --       acoustic-wav       "$CAPTURE_FILE"       "$EXPECTED_HEX"       0       "$START_SAMPLE"       | tee "$EVIDENCE/replay.txt"
    grep -q 'F3_ACOUSTIC_REPLAY' "$EVIDENCE/replay.txt"
    grep -q 'bit_errors=0' "$EVIDENCE/replay.txt"
  else
    cargo run -p signal-trace-replay-cli --       vibration-csv       "$CAPTURE_FILE"       "$EXPECTED_HEX"       5       "$START_SAMPLE"       | tee "$EVIDENCE/replay.txt"
    grep -q 'F3_VIBRATION_REPLAY' "$EVIDENCE/replay.txt"
    grep -q 'bit_errors=0' "$EVIDENCE/replay.txt"
  fi
fi

{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "serial=$SERIAL"
  echo "api=$(adb_run shell getprop ro.build.version.sdk | tr -d '\r')"
  echo "package=$PACKAGE"
  echo "mode=$MODE"
  echo "duration_ms=$DURATION_MS"
  echo "expected_hex=${EXPECTED_HEX:-none}"
  echo "start_sample=$START_SAMPLE"
  echo "capture_pass=$PASS_LINE"
  echo "evidence_level=ANDROID_RUNTIME_CAPTURE"
} >"$EVIDENCE/metadata.txt"

(
  cd "$EVIDENCE"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -print0     | sort -z     | xargs -0 sha256sum >SHA256SUMS
)

echo "F3_ANDROID_TRACE_CAPTURE_PASS mode=$MODE serial=$SERIAL capture=$CAPTURE_FILE evidence=$EVIDENCE"
