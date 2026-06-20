set shell := ["zsh", "-cu"]

rust-test:
    cargo test --workspace

rust-ios-sim:
    # Keep the standalone core archive fresh for shells that link nmp-core
    # directly.
    cargo build -p nmp-core --features lmdb-backend --target aarch64-apple-ios-sim
    # Chirp links one aggregate archive so nmp-core static state is not
    # duplicated across app, projection, and NIP-46 broker crates.
    cargo build -p nmp-app-chirp --features marmot --target aarch64-apple-ios-sim

rust-ios-device:
    # Release build required — pbxproj LIBRARY_SEARCH_PATHS points at the
    # release archive. IPHONEOS_DEPLOYMENT_TARGET=17.0 avoids the
    # ___chkstk_darwin linker error introduced by Xcode 26.
    IPHONEOS_DEPLOYMENT_TARGET=17.0 cargo build -p nmp-core --features lmdb-backend --target aarch64-apple-ios --release
    IPHONEOS_DEPLOYMENT_TARGET=17.0 cargo build -p nmp-app-chirp --features marmot --target aarch64-apple-ios --release

# Seed the gitignored BuildInfo.generated.swift BEFORE xcodegen runs.
#
# project.yml's `Generate BuildInfo` preBuildScript writes this file at every
# Xcode build, but xcodegen's source globs only pick up files that already
# exist when `xcodegen generate` runs. On a clean checkout (e.g. CI) the file
# is absent, so it would be excluded from the Chirp target's Sources and the app
# would fail to compile with "cannot find 'BuildInfo' in scope" even though the
# preBuildScript later writes it. Seeding it here makes the generated project
# self-consistent on a fresh tree. The preBuildScript still overwrites it with
# live branch/commit/time on each Xcode build.
gen-buildinfo:
    #!/usr/bin/env zsh
    set -eu
    OUT="ios/Chirp/Chirp/App/BuildInfo.generated.swift"
    BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
    COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
    BUILD_TIME=$(date -u +"%Y-%m-%d %H:%M UTC")
    {
      echo "// Auto-generated at build time — do not edit"
      echo "enum BuildInfo {"
      echo "    static let branch = \"$BRANCH\""
      echo "    static let commit = \"$COMMIT\""
      echo "    static let buildTime = \"$BUILD_TIME\""
      echo "}"
    } > "$OUT"

gen-ios: gen-buildinfo
    xcodegen generate --spec ios/Chirp/project.yml

build-ios: rust-ios-sim gen-ios
    xcodebuild -project ios/Chirp/Chirp.xcodeproj -scheme Chirp -destination 'platform=iOS Simulator,name=iPhone 17,OS=26.5' -derivedDataPath ios/DerivedData build

run-ios: build-ios
    xcrun simctl install booted ios/DerivedData/Build/Products/Debug-iphonesimulator/Chirp.app
    xcrun simctl launch booted com.example.Chirp

# === FFI hardening (M10.5 phase 1) ===
# Runs S1..S5 Rust harness scenarios against nmp_app_* C symbols.
# Per-scenario output: docs/perf/m10.5/<SCENARIO>/{metrics.json,report.md}

# Individual scenario shortcuts.
stress-s1:
    cargo run --release -p nmp-testing --bin ffi-stress -- mount-unmount --write-report --fail-on-gate

stress-s2:
    cargo run --release -p nmp-testing --bin ffi-stress -- dispatch-flood --write-report --fail-on-gate

stress-s3:
    cargo run --release -p nmp-testing --bin ffi-stress -- snapshot-pressure --write-report --fail-on-gate

stress-s4:
    cargo run --release -p nmp-testing --bin ffi-stress -- reconciler-backpressure --write-report --fail-on-gate

stress-s5:
    cargo run --release -p nmp-testing --bin ffi-stress -- reentrancy --write-report --fail-on-gate

# Generic dispatcher: `just stress s1` .. `just stress s5`
stress S:
    cargo run --release -p nmp-testing --bin ffi-stress -- {{S}} --write-report --fail-on-gate

# Pre-merge fast gate: S1..S5 at fast durations.  Target: < 7 min wall-time.
# Per docs/design/ffi-hardening/ci.md §9.
stress-gate-fast:
    cargo run --release -p nmp-testing --bin ffi-stress -- mount-unmount --duration 60s --write-report --fail-on-gate
    cargo run --release -p nmp-testing --bin ffi-stress -- dispatch-flood --duration 30s --threads 4 --write-report --fail-on-gate
    cargo run --release -p nmp-testing --bin ffi-stress -- snapshot-pressure --duration 30s --write-report --fail-on-gate
    cargo run --release -p nmp-testing --bin ffi-stress -- reconciler-backpressure --duration 60s --write-report --fail-on-gate
    cargo run --release -p nmp-testing --bin ffi-stress -- reentrancy --duration 30s --write-report --fail-on-gate
