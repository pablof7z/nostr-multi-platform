---
title: Real-Relay Validation Tests & Territory
slug: real-relay-validation
summary: "Real-relay validation tests are confined to file-disjoint territory: ONLY crates/nmp-testing/tests/real_relay_*.rs, crates/nmp-testing/tests/soak/, and docs/per"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:9f5b53f7-ae7d-426c-8a51-d7bba9491624
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Real-Relay Validation Tests & Territory

## Scope and Territory

Real-relay validation tests are confined to file-disjoint territory: ONLY crates/nmp-testing/tests/real_relay_*.rs, crates/nmp-testing/tests/soak/, and docs/perf/real-relay/. Territory compliance requires every commit's diff to be exclusively within crates/nmp-testing/tests/real_relay_*, crates/nmp-testing/tests/real_relay_common/, crates/nmp-testing/tests/soak/, or docs/perf/real-relay/. New top-level tests/real_relay_*.rs files build via autodiscovery with no Cargo.toml edit required. [^9f5b5-2]


## Test Scenarios

The real-relay validation suite must implement 6 scenarios: (1) connect+subscribe+receive real kind:1, (2) NIP-65 outbox routing to real authors' write relays, (3) NIP-77 negentropy sync vs REQ-fallback, (4) NIP-42 AUTH challenge/response against an auth-required relay, (5) publish a real signed event+verify OK frame, (6) kind:3 follow-list change→live subscription re-plan. Scenario (2) (F-02 DM cold-start round-trip) must pass against live relays: the gift-wrap is published, the relay ACKs, and the recipient's live subscription receives and decrypts it. Scenario (5) (F-04 zap E2E) requires an NWC connection string and a zap target; the real pay_invoice leg must be confirmed with the user before firing because it irreversibly moves sats. Currently F-02 and F-04 have zero automated verification of the full chain against live relays; only stubbed Rust tests exist. Real parity tests in nmp-testing must call production action builders and assert on actual namespace and JSON shape.

<!-- citations: [^9f5b5-3] [^42908-23] [^f3d8d-15] -->
## Soak Runner

A soak runner must perform sustained multi-relay subscription for a configurable duration, asserting zero leaked subs, bounded memory, and no panics, and write a single-page report to docs/perf/real-relay/. [^9f5b5-4]

## Execution Policy

Real-relay network tests are gated with #[ignore] and run explicitly, not in CI. Every agent is bound to the honest-validation contract: unreachable/absent behavior results in a loud SKIP with a written finding in docs/perf/real-relay/, never a fabricated green result. The test `relay_worker::tests::v58_set_backoff_hint_does_not_break_reconnect` is a known timing-race flake; rerun it rather than chase the failure when it fails on a PR that touches zero Rust. [^9f5b5-5] [^42908-24]

<!-- citations: [^9f5b5-5] [^42908-24] [^6a951-17] -->
## Reporting and Verdicts

Every scenario writes evidence reports in docs/perf/real-relay/ and the suite verdicts are greppable via the pattern '^verdict:' in that directory. [^9f5b5-6]

## Commit Convention

Commit messages for real-relay test work must use the prefix test(real-relay):. [^9f5b5-7]
## See Also

