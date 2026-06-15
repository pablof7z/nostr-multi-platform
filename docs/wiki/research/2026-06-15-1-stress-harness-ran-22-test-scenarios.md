---
type: research-record
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
source_lines: 4059-4102
agent_attribution: a0eaa206b95d5b4f5
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Stress harness ran 22 test scenarios against real NMP kernel with fixture relay; 17 PASS, 0 FAIL, 5 SKIP — landed event-flow architecture empirically verified sound"
captured_at: 2026-06-15T17:37:19Z
---

Stress harness ran 22 test scenarios against real NMP kernel with fixture relay; 17 PASS, 0 FAIL, 5 SKIP — landed event-flow architecture empirically verified sound

---

User: [Agent task result: Agent "Build runnable NMP stress harness" completed]
Done. Built, ran, all green, branch pushed (NOT to master).

## Report

**Branch:** `arch/stress-harness` (pushed to `origin`, not master). Run with `cargo run -p nmp-stress-harness`.

**What it is:** A real, runnable throwaway binary (`crates/nmp-stress-harness/`) — a `[[bin]]`, not a `#[test]` suite, so `cargo test`/CI never runs it (`cargo test -p nmp-stress-harness` runs 0 tests). It drives the actual NMP public/FFI driver surface against real infrastructure: real `nostr::Keys::generate()` + `add_signer(LocalNsec)`, real Schnorr signing through the publish engine (`nmp_app_dispatch_action("nmp.publish")`), an embedded in-process `ws://` fixture relay so events flow through the REAL relay worker → `handle_event` → `verify_and_persist` chokepoint, real `nmp_nip59::gift_wrap_local`, an injected `MonotonicSecondClock`, and persistent LMDB `storage_path` for cold-restart.

**Only existing-file edit:** the workspace `members` line (+ cargo's `Cargo.lock`). Everything else is new files. No nmp-core/nmp-nip01 changes — zero conflict with in-flight PR3/B.

**Result: 17 PASS, 0 FAIL, 5 SKIP (22 total), deterministic across repeated runs.** All landed-behavior scenarios pass — the landed architecture is sound under this harness.

| Scenario | Driver | Result |
|---|---|---|
| A1.1/1.2/1.3 RYW note/repost/reaction | local-publish | PASS |
| A2.1 persistence≠relevance (non-followed persists) | fixture-relay | PASS |
| A3.1 relay-echo dedup, observer fires once (D4) | fixture-relay | PASS |
| A3.2 foreign-author ingest | fixture-relay | PASS |
| A4.1 ephemeral reaches observer, never persisted (ADR-0057 fix) | fixture-relay | PASS |
| A5.1 D9 clamp in observer, store keeps raw ts | fixture-relay | PASS |
| A7.1 cold-restart rebuild from LMDB | persistent+relay | PASS |
| A10.1 local kind:0 profile RYW (PR2) | local-publish | PASS |
| A11.1/11.2 publish-policy fail-closed (C) | local-publish | PASS |
| A13.1 replaceable kind:0 supersession | fixture-relay | PASS |
| CX3 kind:5 delete tombstones target | fixture-relay | PASS |
| CX5 NIP-40 expired-on-arrival silent | fixture-relay | PASS |
| CX7 bad-sig rejected, no poison | kernel-inject | PASS |
| CX8 gift-wrap kind:1059 ingest | fixture-relay | PASS |
| A9.1/9.2 contacts backfill | — | SKIP (pending PR3) |
| B.1 acquisition one-door | — | SKIP (pending Workstream B) |
| F.1/F.2 doctrine gates | — | SKIP (pending Workstream F — runtime via `doctrine_lint_smoke`, not this harness) |

**Fixture-relay vs kernel-injection:** All foreign-author / future-dated / delete / replaceable-sibling / ephemeral / gift-wrap / expiry scenarios use the **fixture relay** (real chokepoint). Local RYW + publish-policy use **dispatch_action** through the real publish engine. Only CX7 uses the **kernel-injection** seam (`nmp_app_inject_signed_event_json`) — deliberately, because its target is the signature-verify gate, not the relay transport.

**No landed-behavior FAILs** — so no master bugs surfaced. Two things worth flagging from building it (configuration findings, not master bugs):

1. **LMDB is off under the plain `native` FFI build.** `kernel/store_init.rs` opens the persistent LMDB store only when *nmp-core's own* `lmdb-backend` cfg flag is set. The FFI `native` feature pulls `nmp-store/lmdb-backend` (so the code compiles) but does **not** set nmp-core's `lmdb-backend` flag — so a `storage_path` silently falls back to `MemEventStore` and cold-restart loses data. The harness's first A7.1 run FAILed for exactly this reason; the fix was adding `nmp-core = { features = ["lmdb-backend"] }` to the harness. Production iOS/Android builds must be enabling it explicitly; worth a one-line audit that they do.

2. **The kernel/FFI APIs the catalog assumed are mostly `pub(crate)`** (`handle_event`, `verify_and_persist`, `push_interest` on `Kernel`, `derive_store_pin_set`). An external crate cannot drive the kernel directly; the only real external driver is the `NmpApp` actor + a real relay. Also, `IngestPreVerifiedEvents` (the kernel-inject test seam) is a **legacy path that bypasses the ADR-0057 chokepoint**, so chokepoint scenarios had to use the fixture relay — which is the correct, more faithful approach anyway.

The harness is D8-clean (no sleep/poll loops — waits are observer-condvar + an in-order single-socket drain barrier); `cargo test -p nmp-testing --test doctrine_lint_smoke` passes 60/60.

The GC/pin scenarios (Area 6, codex #12 pin-release) and a few codex edges (#11 provenance transition, #13 widened-window memoization, #16 multi-account) are **not** implemented this pass — `derive_store_pin_set`/`run_gc_step` are `pub(crate)` on `Kernel` and not reachable through `NmpApp`, so they'd need either an FFI accessor or in-crate test placement; I left them out rather than fake them. Everything implemented drives only real public APIs.
