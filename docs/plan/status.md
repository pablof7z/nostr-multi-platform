# Status — Where we are right now

> Part of the [Build & Validation Plan](../plan.md). Kept fresh by the milestone heartbeat.
>
> **Reconciled 2026-05-21.** A strategic review found this file badly out of sync with
> the codebase (it claimed "~3,800 LOC, in-memory only"). The numbers and subsystem
> inventory below are a fresh survey of `crates/` on master. Honest accounting before
> forecasting forward. See `docs/perf/execution-assessment-2026-05-21.md` for the
> latest risk and debt assessment.

## Codebase size — actual

`find crates -name "*.rs" | xargs wc -l` totals **136,364 LOC of Rust across 28 crate directories.**
Rough per-crate LOC (from `wc -l`, includes tests):

| Crate | LOC | Crate | LOC |
|---|---:|---|---:|
| `nmp-core` | 63,239 | `nmp-nip01` | 2,300 |
| `nmp-testing` | 20,800 | `nmp-nip23` | 2,300 |
| `nmp-repl` | 6,000 | `nmp-content-fixtures` | 2,300 |
| `nmp-signers` | 4,500 | `nmp-nwc` | 1,600 |
| `nmp-marmot` | 4,200 | `nmp-nip22` | 1,500 |
| `nmp-nostr-lmdb` | 3,900 | `nmp-nip57` | 1,400 |
| `nmp-content` | 3,800 | `nmp-codegen` | 1,300 |
| `nmp-nip29` | 3,100 | `nmp-threading` | 1,100 |
| `nmp-reactions` (pkg `nmp-relations`) | 2,800 | `nmp-nip59` | 850 |
| `nmp-signer-broker` | 2,700 | `nmp-nip42` | 820 |
| `nmp-nip77` | 2,600 | `nmp-desktop` | 580 |
| | | `nmp-cli` | 400 |

Plus smaller crates: `nmp-nip42-types`, `fixture-todo-core`, `nmp-signer-iface`,
and `nmp-android-ffi`.

`nmp-core` alone (63k LOC) is the kernel substrate — far past the "~3,800 LOC"
this file previously claimed.

## Implemented and running

The framework has grown well beyond the in-memory kernel slice. Built and on master:

- **Kernel substrate** in `crates/nmp-core`: actor on a dedicated OS thread,
  dual-channel mailbox loop (commands fully drained before relay events;
  `Kernel: !Send` type-enforces the single-writer invariant), substrate trait
  families, ingest pipeline, claim/release refcounting with `ClaimGuard` RAII,
  composite reverse-index dependency tracking, injected `Clock` trait for
  deterministic replay.
- **Persistent storage (LMDB).** `crates/nmp-nostr-lmdb` (~3,900 LOC) plus the
  `lmdb-backend` feature in `nmp-core`. Namespace-isolated key-space
  (`namespace || 0x00 || user_key`), migration staging. **No longer in-memory only.**
- **NIP-65 outbox routing.** Full subscription planner/compiler under
  `nmp-core/src/planner/` — author-set / mailbox / p-tag partition cases,
  interest lattice, recompilation. Reads route to authors' write relays; reaction
  inbox routing regression-guarded (commits `e7c3d3bf`, `50e7e069`).
- **Signers + write path.** `crates/nmp-signers` (~4,500 LOC): local-key signer
  (encrypted, NIP-49), NIP-07, NIP-46 bunker. `crates/nmp-signer-broker` (~2,700
  LOC) for NIP-46 rendezvous. Write path (publish / react / follow) with
  non-blocking signing via `PendingSign`.
- **Multi-account.** Account model, `switch_active` action, identity FFI surface;
  account-switch atomicity reviewed and closed (single-actor invariant).
- **NIP-77 negentropy sync.** `crates/nmp-nip77` (~2,600 LOC): reconciler, planner
  gate, coverage gate, wire layer — wired into `nmp-core/src/subs/`.
- **NIP-42 relay auth.** `crates/nmp-nip42` + `nmp-nip42-types`; AUTH flow,
  CLOSED ingest handlers, tests in `nmp-core`.
- **Marmot / MLS encrypted groups.** `crates/nmp-marmot` (~4,200 LOC, OpenMLS):
  domain, projection, view, action, interest, service. Plus `crates/nmp-nip29`
  (~3,100 LOC) for NIP-29 groups and `crates/nmp-nip59` (~850 LOC) gift-wrap.
  Auto-registration after `createAccount` (commit `28cf348d`).
- **NWC wallet (NIP-47).** `crates/nmp-nwc` (~1,600 LOC): URI parsing, request
  signing (kind 23194), response decode (kind 23195), feature-gated
  `wallet = ["dep:nmp-nwc"]` in the kernel.
- **NIP-57 zaps.** `crates/nmp-nip57` (~1,400 LOC): zap receipt parsing.
- **Protocol module crates.** `nmp-nip01` (events/profiles), `nmp-nip22`
  (comments), `nmp-nip23` (long-form), `nmp-reactions` (pkg `nmp-relations`),
  `nmp-threading`, `nmp-content` (~3,800 LOC content rendering).
- **Codegen tool** `crates/nmp-codegen` (~1,300 LOC): reads `nmp.toml`, produces
  per-app crates, determinism tests.
- **Desktop shell.** `crates/nmp-desktop` (~580 LOC) links the kernel rlib directly.
- **CLI** `crates/nmp-cli`: `gen` + `init` scaffolding commands.
- **Android FFI** `crates/nmp-android-ffi` (CI-checked via `cargo check`).
- **Active app shells.** `ios/Chirp` and the Android Chirp/gallery shells are the
  active product proofs. Podcast and Highlighter app shells have been removed
  from active scope until Chirp is complete.
- **Test + bench harness** `crates/nmp-testing` (~20,800 LOC): mock relays,
  reactivity-bench, firehose-bench. `nmp-repl` (~6,000 LOC) interactive harness.
- **CI.** Rust CI runs workspace tests, a `--features lmdb-backend` step, and
  an `android-ffi` host `cargo check`. Native iOS builds/tests, Gradle builds,
  and Android instrumentation are not currently PR-gated.
- **Architecture decisions** locked in ADRs `docs/decisions/0001`–`0022`.

## Designed but not implemented

- **Blossom uploads / downloads** — no `nmp-blossom` crate exists; no Blossom code
  in `nmp-core`.
- **Web-of-Trust subsystem** — no WoT crate, no trust-score source in `crates/`.
- **NIP-17 messaging** as a dedicated conversation layer + NSE companion crate
  (gift-wrap primitives exist in `nmp-nip59`; the NIP-17 conversation layer does not).
- **Cashu (NIP-60) / nutzaps (NIP-61)** — NWC (NIP-47) and zap-receipt parsing
  (NIP-57) are built; the rest of the wallet stack is not.
- **UniFFI bindings** — the iOS bridge is still raw C FFI; no crate depends on
  `uniffi`. UniFFI migration (M14) is unstarted.
- **Web (wasm) shell** — desktop and Android shells exist; no wasm shell.
- **Non-Nostr-shaped product app** demonstrating the kernel boundary in
  production beyond `fixture-todo-core`. Podcast and Highlighter are deferred
  until Chirp is complete.

## Milestone ladder — implemented vs. ladder claim

The M0–M17 ladder predates the current codebase. Reality, milestone by milestone:

| Milestone | Ladder claim | Actual state |
|---|---|---|
| M0 Kernel + fixture | DONE | ✅ Built |
| M1 Chirp social baseline on iOS | hardening | ✅ Built (Chirp + iOS shells) |
| M2 Subscription compilation + outbox + kind:3 | design + impl | ✅ Planner/compiler built |
| M3 Persistence (LMDB) | design + impl | ✅ `nmp-nostr-lmdb` + `lmdb-backend` feature |
| M4 NIP-77 negentropy | pending | ✅ `nmp-nip77` built + wired |
| M5 NIP-42 relay auth | pending | ✅ `nmp-nip42` built |
| M6 Sessions + signers + write | pending | ✅ `nmp-signers` + write path built |
| M7 Reactions + thread + reply | pending | ✅ `nmp-reactions` + `nmp-threading` built |
| M8 Multi-session | pending | ✅ Multi-account + `switch_active` built |
| ~~M9~~ DMs | deferred post-v1 | 🟡 gift-wrap built; conversation layer not |
| M10 Blossom + media | pending | ❌ Not built |
| M10.5 FFI hardening | design done | ✅ S2/S3/S4/S5 gates closed; native CI coverage still a gap |
| ~~M11~~ Podcast rebuild | deferred | Deferred until Chirp is complete |
| ~~M11.5~~ Highlighter app proof | deferred | `nmp-nip29` built as generic infrastructure; app shell removed |
| ~~M12~~ Wallet | deferred post-v1 | 🟡 NWC + NIP-57 built; Cashu/nutzaps not |
| M13 Web-of-Trust | pending | ❌ Not built |
| M14 UniFFI migration | pending | ❌ Not started (still raw C FFI) |
| M15 Cross-platform | pending | 🟡 Desktop + Android shells; no wasm |
| M16 CLI + starter | pending | 🟡 `nmp-cli` exists; starter recipes not |
| M17 v1 release | pending | ❌ Pending |

The ladder is no longer a forward roadmap for M2–M9 — that work is largely on
master. M10 (Blossom), M13 (WoT), M14 (UniFFI), and M17 (release) remain the
genuine open frontier.

## Divergence from scope-adjustments-2026-05-18.md — needs a decision

`scope-adjustments-2026-05-18.md` deferred two areas to post-v1:

- **M9 — NIP-17 DMs / NSE** (the messaging / encrypted-group lane), and
- **M12 — Wallet (NWC + zaps + Cashu + nutzaps)**.

Both were subsequently built on master anyway: **Marmot/MLS encrypted groups**
(`nmp-marmot`, `nmp-nip29`, `nmp-nip59`) and **NWC + NIP-57 zaps** (`nmp-nwc`,
`nmp-nip57`) are present, tested, and kernel-wired. The plan and the codebase have
diverged.

This is a reality check, not a critique — but it needs a formal scope decision:
either accept Marmot/NWC into v1 scope and update the deferral document, or
remove them from master. Tracked in
[`docs/arch-review-queue.md`](../arch-review-queue.md). The deferral document is
kept in place; a note has been added there pointing at this divergence.

## Gaps in the prior plan that the M0–M17 rewrite addressed

(Retained from the prior plan revision — still accurate as design history.)

- The earlier plan was phase-numbered without explicit *demoable products* per phase.
- NIP-42 wasn't covered as its own milestone.
- Subscription compilation (the load-bearing NDK/Applesauce lesson) wasn't elevated
  to its own milestone.
- Blossom and media-capability lifecycle were one bullet under a larger phase.
- No milestone proved the kernel boundary for a fundamentally non-social product.
- No dedicated FFI hardening + iOS empirical proof gate (now M10.5).
- M11 was generic; the rewrite tied it to a concrete app rebuild target.
