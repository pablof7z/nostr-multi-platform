# Execution Assessment — 2026-05-21

This assessment is grounded in the live repository, not the historical heartbeat
logs. Commands run from branch `agent-5233e312-exec-assessment`.

## Current Execution State

- `master` is three local commits ahead of `origin/master` at `ca9f6d02`.
  Those commits are docs/comment-only polish and review artifacts; the last
  remote commit is `71c558a9 feat(nip29): wire 14 dormant ActionModule impls
  through dispatch_action (#99)`.
- Cargo metadata reports 29 workspace members. There are 28 crate directories
  under `crates/` plus the Chirp and fixture app crates.
- `find crates -name "*.rs" | xargs wc -l` reports 136,364 Rust LOC, including
  tests and benches. The framework is well beyond the earlier in-memory kernel
  slice.
- The active product proof is Chirp. Podcast and Highlighter remain deferred as
  app proofs; generic protocol work that came from those explorations remains in
  reusable NMP crates.
- Raw C FFI is still the iOS boundary. `NmpCore.h` now declares capability,
  unsigned-event, and URI-opening symbols that `docs/ffi-surface.md` still
  described as absent.

## Verified Strengths

- The kernel has real storage, routing, auth, publish, wallet, Marmot, NIP-29,
  NIP-57, NIP-77, and signer substrates in the workspace.
- The actor loop prioritizes command drain before relay events and blocks on
  `recv_timeout` rather than spinning.
- The iOS shell registers a native keychain capability before startup, then
  mirrors pushed snapshots instead of polling for kernel state.
- `nmp-app-chirp` has moved social verbs and several NIP-module actions behind
  `nmp_app_dispatch_action`, reducing app-noun C symbols.

## Main Risks

1. Swift still owns some business policy. `KernelModel.swift` hardcodes default
   relays and injects them before `start()`, and `createAccount` passes Swift's
   chosen relays into Rust. That should become Rust-owned policy with Swift only
   reporting capability facts or rendering choices.
2. Snapshot application still triggers effects. `KernelModel.apply` calls
   Marmot registration and synchronous snapshot pulls from the main actor. That
   weakens the "snapshot as mirror" discipline and creates a performance risk
   around every kernel revision.
3. Marmot group creation/invite is still a Swift-orchestrated retry flow. Swift
   kicks off key-package fetches, then asks the user to tap again if packages are
   missing. Rust should own the pending operation and retry/complete when the
   needed packages arrive.
4. Several core and bridge files remain far over the 500-line ceiling. The
   biggest current offenders include `crates/nmp-core/src/actor/commands/tests.rs`
   (1,862), `crates/nmp-core/src/kernel/mod.rs` (1,078),
   `crates/nmp-core/src/ffi/mod.rs` (1,063), and
   `ios/Chirp/Chirp/Bridge/KernelBridge.swift` (826).
5. Core progress still depends on periodic idle wakes. The actor uses
   `recv_timeout(wait)` to drive lifecycle ticks, publish retries, planner
   triggers, and pending remote-sign checks. That is a pragmatic current
   implementation, but it means "no polling" should not be read as "no periodic
   wakeups anywhere."
6. Some identifiers that comments describe as stable use `DefaultHasher`
   (`contacts.rs`, `sub_key.rs`, profile/thread request ids). Replace these with
   an explicit stable hash before relying on them for persisted or cross-process
   identity.
7. CI does not currently prove the native app surfaces. Rust workspace tests and
   Android FFI host checks exist, but there is no PR-gated `xcodebuild`, iOS UI
   test, Gradle build, or Android instrumentation job.
8. File-size enforcement prevents new changed-file violations but does not make
   the repository clean. The current tree still has dozens of hard-cap
   offenders.
9. Documentation has stale exact claims. The largest confirmed drift was the FFI
   surface reference saying symbols were missing from `NmpCore.h` after the
   header had already been updated.

## Technical Debt And Improvement Opportunities

- Split `nmp-core` boundary files by cohesive owner: actor command tests,
  kernel module root, FFI root/action, action registry, identity commands, relay
  management, planner compiler, and publish engine.
- Move Chirp default-relay policy and Marmot registration/retry orchestration
  into Rust. Swift should only send user intent and execute OS capabilities.
- Avoid extra JSON work on the iOS hot path: `KernelBridge` currently parses the
  outer update, reserializes the inner payload, then decodes again; `KernelModel`
  may also call `chirpSnapshot()` as a second FFI/JSON round-trip when timeline
  items change.
- Bound command-drain fairness explicitly. The current dual-channel actor design
  prevents relay floods from starving commands, but an unbounded command burst
  can delay relay ingestion and idle work.
- Reduce per-event timeline churn. Timeline ingest still pushes and sorts the
  bounded visible list on each inserted event; batch sorting or binary insertion
  would reduce burst costs.
- Keep generated/review dumps out of source-size accounting. `docs/perf/codex-reviews`
  contains very large review transcripts; they are useful artifacts, but should
  remain clearly exempt from hand-authored documentation limits.
- Add a native verification lane for Chirp. At minimum, PRs touching iOS bridge,
  headers, or app views should run an `xcodebuild` build/test path; PRs touching
  Android bindings should run Gradle, not just host `cargo check`.
- Add deterministic LMDB test seams where tests currently sleep to force async
  commit ordering.

## Validation

- `cargo metadata --no-deps --format-version 1`
- `find crates -name "*.rs" | xargs wc -l`
- `find crates apps ios android docs -type f ... | xargs wc -l | sort -nr`
- `rg` scans for stale FFI/header claims, polling/sleep patterns, and Swift
  policy/effect seams
- `cargo test -p nmp-testing --test framework_magic_contract` passed:
  14 passed, 0 failed, 0 ignored
- `cargo test --workspace` built and ran many suites, then failed in
  `nmp-nostr-lmdb` because LMDB test stores could not be created:
  `Os { code: 28, kind: StorageFull, message: "No space left on device" }`.
  This is an environment/disk result, not a verified behavioral regression.
