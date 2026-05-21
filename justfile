set shell := ["zsh", "-cu"]

rust-test:
    cargo test --workspace

gen-modules:
    cargo run -p nmp-codegen -- gen modules --manifest apps/fixture/nmp.toml --out apps/fixture/nmp-app-fixture

gen-modules-check:
    cargo run -p nmp-codegen -- gen modules --manifest apps/fixture/nmp.toml --out apps/fixture/nmp-app-fixture --check

rust-ios-sim:
    # Keep the standalone core archive fresh for shells that link nmp-core
    # directly.
    cargo build -p nmp-core --features lmdb-backend --target aarch64-apple-ios-sim
    # Chirp links one aggregate archive so nmp-core static state is not
    # duplicated across app, projection, and NIP-46 broker crates.
    cargo build -p nmp-app-chirp --target aarch64-apple-ios-sim

gen-ios:
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
