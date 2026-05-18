# Pulse e2e smoke scenarios (T66c)

Smoke scenarios that drive the **hard parts** of the NMP kernel through the
real FFI surface — the same code path the SwiftUI screens use — and the
matching simulator walkthrough.

- **Authoritative artifact:** `ios/NmpPulse/NmpPulseTests/SmokeScenariosTests.swift`
  (XCTest, `@testable import NmpPulse`, drives the real `KernelModel` →
  `nmp_app_*` FFI → real relays). This is the load-bearing pass/fail.
- **Visual evidence:** the screenshots in this directory, captured driving
  the live app in the iPhone 17 simulator (iOS 26.5, sim
  `C380BA01-AD22-4F4A-8524-A309DA15144C`).

## How to run the authoritative suite

```bash
# 1. Build the sim staticlib (consumes the kernel as-built; no Rust changes).
cargo build -p nmp-core --target aarch64-apple-ios-sim

# 2. Regenerate the Xcode project + run the gated smoke suite.
cd ios/NmpPulse && xcodegen generate
xcodebuild test \
  -project NmpPulse.xcodeproj -scheme NmpPulse \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  -derivedDataPath ./build \
  -only-testing:NmpPulseCapabilityTests/SmokeScenariosTests \
  TEST_RUNNER_NMP_SMOKE=1
```

Without `NMP_SMOKE=1` every scenario `XCTSkip`s (they are network-bound and
slow, mirroring the Rust `#[ignore]` smoke suite) — a skip is honest
non-execution, never a fake pass.

## Result — last run 2026-05-18, iPhone 17 sim (iOS 26.5)

`Test Counts: Total: 4 · Passed: 4 · Failed: 0 · Skipped: 0`

| # | Scenario | Result | What it asserts | Evidence |
|---|----------|--------|-----------------|----------|
| 2 | kind:3 follow-list change → timeline re-targets/re-plans | **PASS** | `nmp_app_follow` enqueues a kind:3 in `publishQueue`; `rev` advances past the pre-follow value (timeline re-planned). | `03-timeline-backfill.png` (live feed + `rev` counter), `02-thread-detail.png` |
| 3 | NIP-77 negentropy path on connect (REQ-fallback asserted) | **PASS (reachable subset)** | A relay reaches `connected` and the timeline backfills over a real socket — the REQ-fallback path proven end-to-end. The negentropy verdict itself is **not observable** (gap G1). | `04-diagnostics-auth-relay.png` (`stored events 688`, `events RX 1014→1020`, `relay.primal.net connected`) |
| 4 | NIP-42 AUTH challenge handled | **PASS (reachable subset)** | The AUTH state machine is wired and every default relay settles to the `not_required` key with no spurious challenge. A real handshake is **not drivable in-sim** from the app surface (gap G2). | `04-diagnostics-auth-relay.png` (`auth: not_required`) |
| 6 | multi-session: add 2nd account, switch active, feed + compose identity switch synchronously | **PASS** | Add account B via the same sign-in path the "+ Add" sheet uses; `switchActive` flips `activeAccount` **and** the `accounts` projection's `isActive` synchronously, both directions. Per D5/D8 there is no Swift-side session state, so this single snapshot fact *is* the feed + compose identity. | `06-accounts-multisession.png` (Accounts screen, active checkmark, Add-account entry point) |

## Simulator walkthrough (xcode-MCP)

The XCTest suite is authoritative; this walkthrough captures the same paths
through the real UI for visual evidence.

```
build_sim  → install_app_sim → launch_app_sim com.example.NmpPulse
screenshot                                  → 00-onboarding.png
tap "Paste nsec" → (kernel signs in, Timeline populates)
screenshot                                  → 03-timeline-backfill.png
tap a note (Thread/NoteDetail opens)
screenshot                                  → 02-thread-detail.png
tap Diagnostics tab
screenshot                                  → 04-diagnostics-auth-relay.png
tap Accounts tab
screenshot                                  → 06-accounts-multisession.png
```

## Honest gaps (REPORT findings — Rust NOT modified; `crates/**` off-limits this task)

**G1 — NIP-77 negentropy diagnostic is dead.**
`crates/nmp-core/src/kernel/status.rs:22` hardcodes
`nip77_negentropy: "unknown"`. The snapshot never carries a real negentropy
verdict, so scenario 3 cannot observe NEG-OPEN/NEG-MSG vs. REQ from the app
surface. Reachable subset asserted instead: events arrive + a relay
connects (REQ-fallback proven over a real socket). To make scenario 3 prove
negentropy-vs-REQ, the kernel must surface the `Nip77CapabilityProbe`
verdict and per-relay neg byte counters into `RelayStatus`. *Kernel-team
action; not fixable from `ios/NmpPulse/**`.*

**G2 — `nmp_app_add_relay` does not open a wire socket; default relays are
not AUTH-required.**
`crates/nmp-core/src/actor/commands/relays.rs::add_relay` only mutates the
`relay_edit_rows` projection. The wire layer
(`crates/nmp-core/src/actor/relay_mgmt.rs`) iterates the fixed
`RelayRole::all()` pair — `wss://relay.primal.net` + `wss://purplepag.es` —
neither AUTH-required. So adding `wss://nostr.wine` from the Accounts screen
cannot drive a real NIP-42 handshake: no third socket is opened. The NIP-42
*ingest + driver FSM* is wired and correct
(`crates/nmp-core/src/kernel/ingest/auth_handlers.rs`,
`crates/nmp-core/src/kernel/auth.rs`); full handshake transition coverage
(`not_required → challenge_received → authenticating → authenticated`)
lives in the Rust `crates/nmp-core/src/kernel/auth_tests.rs` suite. Scenario
4 asserts the reachable in-sim subset (state machine wired + steady-state
`not_required`, no false challenge). *Kernel-team action to make
`add_relay` open a real socket; not fixable from `ios/NmpPulse/**`.*

**G3 — FFI does not survive instantiate→free→reinstantiate in one process.**
`nmp_app_new()` → `nmp_app_free()` → `nmp_app_new()` in the same process
SEGVs (`SIGSEGV`, `KERN_INVALID_ADDRESS`) when relay sockets were live
during the free. Suspect: relay-worker thread-join in `nmp_app_free`
(`crates/nmp-core/src/ffi/mod.rs:62-104`) vs. sockets blocked in
`connect()` while the C callback context is dropped
(`crates/nmp-core/src/relay_worker.rs`). Mitigation in the smoke suite: a
single process-shared `KernelModel`; scenarios run additively in XCTest's
default alphabetical order (2 < 3 < 4 < 6). The FFI supports **N=1**
`NmpApp` instances per process in practice. *Kernel-team action; not
fixable from `ios/NmpPulse/**`.*

**G4 — Combine sinks against the shared kernel crash XCTest.**
Attaching `model.$x.sink` subscribers to the long-lived shared kernel and
calling `XCTestExpectation.fulfill()` from them crashed in
`swift_task_localValuePopImpl()` (`KERN_INVALID_ADDRESS`): the shared kernel
keeps emitting after an async test's task-local context is torn down, so the
sink fires into a freed XCTest error-observation scope. This is a
test-harness interaction, not a kernel defect — resolved entirely within the
test file by using a synchronous polling primitive (`waitUntil`) instead of
Combine subscriptions. No external action required; recorded for the next
test author.

**G5 — SwiftUI accessibility tree not exported to the sim AX bridge.**
`describe_ui` returns an empty hierarchy for this app across launches, so
MCP-driven element-precise tapping is unreliable; the walkthrough above uses
coordinate taps. The XCTest suite (driving `KernelModel` directly) is
unaffected and remains authoritative. A future UI-automation effort would
need `.accessibilityIdentifier` annotations on the Pulse views (those files
are owned by held agents this task; not touched here). Recorded for
visibility.

## Scope notes

- Touched only `ios/NmpPulse/NmpPulseTests/**` + `docs/perf/pulse/smoke/**`
  (disjoint from the held pulse-feed / d1-avatar agents per the task brief).
- No Rust modified; the kernel is consumed as-built. Every gap above that
  requires a kernel change is filed here as a REPORT finding rather than
  papered over with a fake pass.
- `SmokeScenariosTests.swift` is 274 LOC (≤300 soft cap); no
  TODO/unimplemented in non-test code (this is test code regardless).
