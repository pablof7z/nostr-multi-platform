# CI / pre-merge hygiene

> Part of the [Build & Validation Plan](../plan.md).

Required CI gates (apply from the milestone they become possible):

- `cargo fmt --all -- --check` (always).
- `cargo test --workspace` (always).
- `cargo run -p nmp-codegen -- gen modules --check` (codegen determinism, from [M0](m0-fixture.md)).
- `cargo run -p nmp-testing --bin reactivity-bench --release -- --standard --fail-on-gate` (from [M0](m0-fixture.md)).
- `cargo run -p nmp-testing --bin firehose-bench --release -- replay --standard --fail-on-gate` (from [M0](m0-fixture.md)).
- iOS build (`just build-ios`) from [M1](m1-twitter-slice.md).
- iOS UI test (`xcrun simctl test`) from [M1](m1-twitter-slice.md).
- Android build from [M15](m15-cross-platform.md).
- Desktop build from [M15](m15-cross-platform.md).
- Web build from [M15](m15-cross-platform.md).
- Cross-platform consistency test from [M15](m15-cross-platform.md).

Live firehose runs are not in pre-merge CI (would block on relay flakes); they run nightly on a dedicated runner and produce reports tagged `live` in `docs/perf/m<N>/`.
