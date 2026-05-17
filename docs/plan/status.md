# Status — Where we are right now

> Part of the [Build & Validation Plan](../plan.md). Kept fresh by the milestone heartbeat.

Honest accounting before forecasting forward.

## Implemented and running

- **Kernel substrate** in `crates/nmp-core` (~3,800 LOC): actor on a dedicated OS thread, mailbox-driven (ADR feedback adopted — relay reads happen in tokio reader tasks, the actor blocks on its own channel with deadline timeouts), substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`) in `nmp-core/src/substrate/`, ingest pipeline (`kernel/ingest.rs`), claim/release refcounting for profile interest (commit `23ae829`), composite reverse-index dependency tracking.
- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
- **Codegen tool** in `crates/nmp-codegen` (~423 LOC): reads `nmp.toml`, produces a per-app crate, has determinism tests.
- **Benches** in `crates/nmp-testing`: `reactivity-bench` (composite-key reverse index + coalescer + working set; run 002 passed all ADR-0001..0004 gates) and `firehose-bench` (replay + capture + live modes; replay scenarios pass the modeled budget contract).
- **Perf reports** in `docs/perf/` documenting reactivity-bench run 002, firehose-bench replay runs, and three iOS measurement reports (relay lifecycle, profile/thread subscriptions, the primal slice baseline).
- **Architecture decisions** locked in 10 ADRs (`docs/decisions/0001`–`0010`).

## Designed but not implemented

- LMDB / IndexedDB persistent storage (in-memory only today).
- NIP-65 outbox routing (hardcoded content + indexer relays today).
- NIP-77 negentropy sync.
- NIP-42 relay auth.
- Multi-account / multi-session model and account switching.
- Signer trait + local-key signer + NIP-46 bunker signer.
- Action ledger + write path (compose / react / repost / quote).
- NIP-17 messaging and the NSE companion crate.
- Blossom uploads / downloads with resumable progress.
- Wallet stack (NWC, NIP-57 zaps, Cashu, nutzaps).
- Web-of-Trust subsystem.
- UniFFI bindings (current iOS bridge is raw C FFI).
- Android shell, Desktop shell, Web shell.
- The `nmp` CLI scaffolding tool.
- A non-Nostr-shaped product (podcast app) demonstrating the kernel boundary in production.

## Gaps in the prior plan that this rewrite addresses

- The prior plan was phase-numbered (Phase 1, 2, …) without explicit *demoable products* per phase.
- NIP-42 wasn't covered.
- Subscription compilation (the load-bearing NDK/Applesauce lesson) wasn't elevated as its own milestone.
- Blossom and media-capability lifecycle (long-running, resumable, background) were one bullet under Phase 6.
- No milestone proved the kernel boundary for a fundamentally non-social product.
- The plan didn't reflect that M0 and M1 are largely done (M1 is now ✅ DONE — two exit-gate items deferred to M10.5 and M14 per T22; see m1-twitter-slice.md §Deferred).
- **No dedicated FFI hardening + iOS empirical proof gate before the kernel-boundary proof.** The prior M11 implicitly assumed the FFI surface was ready; this rewrite makes it a separate milestone (M10.5).
- **M11 was generic.** This rewrite ties it concretely to `/Users/pablofernandez/src/podcast` (the fully-functional Swift app) as the rebuild target, with copy-first UI fidelity and an explicit view-by-view module mapping.

The plan below is a single ladder of eighteen milestones (M0–M17, with M10.5 inserted as the FFI gate), each producing a runnable artifact, ordered so that each milestone strictly adds capabilities to the prior demoable product.
