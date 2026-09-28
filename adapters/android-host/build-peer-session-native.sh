#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT_DIR="$REPO_ROOT/adapters/android-host/src/main/jniLibs"

: "${ANDROID_NDK_HOME:?ANDROID_NDK_HOME must point to an Android NDK}"

cd "$REPO_ROOT"

cargo ndk   -t arm64-v8a   -t x86_64   -o "$OUT_DIR"   build -p android-peer-session-jni --release

test -f "$OUT_DIR/arm64-v8a/libsp3_android_peer_session.so"
test -f "$OUT_DIR/x86_64/libsp3_android_peer_session.so"

echo "Built Android peer-session JNI libraries:"
find "$OUT_DIR" -type f -name 'libsp3_android_peer_session.so' -print
