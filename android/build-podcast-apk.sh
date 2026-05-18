#!/usr/bin/env bash
# build-podcast-apk.sh — single-version always-built APK for sideload (T157).
#
# Pablo's sideload contract:
#   - The artifact lives at a stable, predictable path
#   - Only ONE APK is on disk at any time — older ones are deleted before
#     each rebuild (no version-suffix sprawl)
#   - The build prints the version + path + size so the orchestrator can
#     surface them in the commit body / heartbeat log
#
# Pipeline:
#   1. `cargo ndk` builds `libnmp_android_ffi.so` for `arm64-v8a` + `x86_64`
#      into `android/app/src/main/jniLibs/` (shared between :app and :podcast)
#   2. Delete any old APKs in podcast/build/outputs/apk/debug/
#   3. `./gradlew :podcast:assembleDebug` builds `podcast-debug.apk`
#      (Gradle's applicationVariants rename does the file naming —
#       see android/podcast/build.gradle.kts)
#   4. Mirror the APK to android/app/build/outputs/apk/debug/podcast-debug.apk
#      so the T157 spec's literal path is satisfied alongside the natural one.
#   5. Print version + path + size
#
# Usage:
#   ./android/build-podcast-apk.sh [--clean]

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ANDROID_DIR="$REPO_ROOT/android"
PODCAST_OUT="$ANDROID_DIR/podcast/build/outputs/apk/debug"
APP_MIRROR="$ANDROID_DIR/app/build/outputs/apk/debug"
APK_NAME="podcast-debug.apk"

if [[ "${1:-}" == "--clean" ]]; then
    echo "[1/5] Clean Gradle outputs"
    (cd "$ANDROID_DIR" && ./gradlew :podcast:clean) || true
fi

# Step 1 — cargo-ndk builds the .so. The :podcast:preBuild task already
# depends on :app:cargoNdk, but invoking it here gives the script a clear
# stage with surface-able output if NDK is misconfigured.
echo "[1/5] cargo ndk → libnmp_android_ffi.so (arm64-v8a, x86_64)"
"$HOME/.cargo/bin/cargo" ndk \
    --manifest-path "$REPO_ROOT/crates/nmp-android-ffi/Cargo.toml" \
    -t arm64-v8a -t x86_64 \
    -o "$ANDROID_DIR/app/src/main/jniLibs" \
    build --release

# Step 2 — single-version constraint: delete any stale APKs.
echo "[2/5] Clear old APKs under $PODCAST_OUT"
rm -f "$PODCAST_OUT"/*.apk 2>/dev/null || true
mkdir -p "$PODCAST_OUT"

# Step 3 — Gradle assemble. The applicationVariants rename in
# android/podcast/build.gradle.kts produces $APK_NAME directly.
echo "[3/5] Gradle :podcast:assembleDebug"
(cd "$ANDROID_DIR" && ./gradlew :podcast:assembleDebug)

APK="$PODCAST_OUT/$APK_NAME"
if [[ ! -f "$APK" ]]; then
    echo "ERROR: expected APK at $APK — Gradle output may have changed name."
    ls -la "$PODCAST_OUT" || true
    exit 1
fi

# Step 4 — also mirror under app/build/outputs/apk/debug so the T157
# spec's literal path resolves to the same APK. Single-version applies to
# both paths.
echo "[4/5] Mirror APK to $APP_MIRROR/$APK_NAME"
mkdir -p "$APP_MIRROR"
rm -f "$APP_MIRROR"/podcast-debug.apk
cp "$APK" "$APP_MIRROR/$APK_NAME"

# Step 5 — print version + path + size.
echo "[5/5] Done"
VERSION="$(awk -F'"' '/versionName =/ { print $2 }' "$ANDROID_DIR/podcast/build.gradle.kts" | head -1)"
SIZE_BYTES="$(stat -f%z "$APK" 2>/dev/null || stat -c%s "$APK")"
SIZE_MIB="$(awk -v b="$SIZE_BYTES" 'BEGIN { printf "%.2f", b/1024/1024 }')"

cat <<EOF

────────────────────────────────────────────────────────────
  podcast-debug.apk  built
  versionName : $VERSION
  paths       : $APK
                $APP_MIRROR/$APK_NAME
  size        : $SIZE_BYTES bytes (${SIZE_MIB} MiB)
────────────────────────────────────────────────────────────
EOF
