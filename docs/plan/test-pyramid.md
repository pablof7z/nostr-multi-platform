# Test pyramid

> Part of the [Build & Validation Plan](../plan.md).

| Level | Tooling | What it covers | Where it lives |
|---|---|---|---|
| Unit | `cargo test` per crate | Pure-function correctness, substrate trait invariants, codegen determinism | Each crate's `tests/` |
| Subsystem integration | `cargo test --test '*'` in `nmp-testing` | EventStore + planner + sync against MockRelay | `crates/nmp-testing/tests/` |
| Cross-FFI | UniFFI binding round-trip tests | Bindings stability, rev ordering, callback delivery | `apps/<name>/nmp-app-<name>/tests/` (post-M14) |
| Cross-platform consistency | Script harness | Same scenario on iOS sim + Android emu + desktop + headless web; assert `AppState` JSON byte-equal | `nmp-testing/scenarios/` |
| Offline publish-intent contract | `cargo test` + `nmp-testing` scenarios | Intent persistence before signing, stored relay resolution, restart/foreground/reconnect drains, no polling loops | `docs/design/offline-first-publish-intents.md` |
| Reactivity bench | `reactivity-bench --standard --fail-on-gate` | Composite reverse index, delta coalescing, working-set memory, allocation gates | `crates/nmp-testing/bin/reactivity-bench/` |
| Firehose bench (modeled) | `firehose-bench replay --standard --fail-on-gate` | Budget contract for the runtime | `crates/nmp-testing/bin/firehose-bench/` |
| Firehose bench (live) | `firehose-bench live` against the real iOS app | Runtime evidence end-to-end | reports in `docs/perf/m<N>/` |
| Per-app UI smoke | XCUITest + Espresso + iced UI test + Playwright | End-to-end flows render without error | `ios/<app>/UITests/` etc. |
| Manual exploratory | Humans on reference devices | What metrics can't catch | per-milestone manual checklist |

The cross-platform consistency tests are the highest-value tier post-[M15](m15-cross-platform.md).
