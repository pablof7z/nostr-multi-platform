# Scope Adjustments — 2026-05-18

> **Divergence note (2026-05-20):** The two deferrals recorded below — M9 (DMs/messaging)
> and M12 (Wallet) — did not hold. Marmot/MLS encrypted groups (`nmp-marmot`, `nmp-nip29`,
> `nmp-nip59`) and NWC + NIP-57 zaps (`nmp-nwc`, `nmp-nip57`) were subsequently built,
> tested, and wired into the kernel on master. This document is kept for the historical
> record; the plan and the codebase have diverged. A formal scope decision is needed —
> tracked in `docs/arch-review-queue.md` and summarized in `docs/plan/status.md`.

User direction landed mid-session-1. Captures: what shifts in the milestone ladder, what gets folded into existing milestones, and what new research must precede the next plan revision.

## What changes in the ladder

| Was | Becomes |
|---|---|
| **M9 — NIP-17 DMs + NSE** | **Deferred to post-v1.** DMs add NSE, gift-wrap, NIP-44, App Groups, and a whole capability lane that we don't need for v1 demos. Re-add as a post-v1 milestone. |
| **M12 — Wallet (NWC + zaps + Cashu + nutzaps)** | **Deferred to post-v1.** Wallet is huge surface area (NWC, NIP-57, NIP-60, NIP-61) and not load-bearing for v1 doctrine proofs. Re-add as a post-v1 milestone. |
| *(none)* | **New M11.5 — Highlighter rebuild + `nmp-nip29` crate.** Builds the second non-social-domain extension app on top of the kernel, after the podcast app proves the boundary. Source: `/Users/pablofernandez/Work/hl/app` (Highlighter — already has a Rust core + native UIs; the rebuild port it onto NMP's substrate + codegen + view-module/action-module pattern). Adds NIP-29 as a first-class protocol module crate (groups, moderation, joining flow, kind 39000–39003 metadata events) done in NMP-idiomatic shape. |

## What must be folded INTO existing milestones

The user named several capabilities a "proper" Nostr app must have that the framework must guarantee, not the app:

### Folded into M2 (subscription compilation + outbox routing)

- **Kind:3 auto-tracking** for the active account's follow list. When a kind:3 lands for the active account, the planner automatically refreshes the author-set of every subscription that depends on "the current user's follows". The app never re-issues the REQ — the framework recompiles transparently.
- **NDK reference:** how NDK auto-follows kind:3 changes and re-routes its open subs. (Captured in M2 research wave; agents fan out.)
- **Applesauce reference:** the "event store query builder" magic that makes subscriptions auto-update without the app touching them. Highest-priority NDK/Applesauce lesson per user.

### Folded into M6 (sessions + signers + write path)

- **NIP-46 bunker:// URL parsing + connection flow.** A pasted `bunker://` URL is a first-class signer onboarding path; framework wires the rendezvous.
- **Create new nsec flow.** Generate, encrypt (NIP-49), and store via Keychain capability. User-initiated through a single action; no app needs to roll its own.
- These join the existing local-nsec + NIP-46-fallback work in M6.

### New M1.5 (or fold into M2 if size allows) — "Framework magic" contract

A dedicated **doctrine document** + **kernel-substrate test suite** that enumerates every behavior the framework guarantees without the app authoring code for it. Working title: "Things that just work." Includes (non-exhaustive):

- Kind:3 auto-tracking → subscription recompile (M2 implementation)
- Auto-resolve replaceable events on insert (already done; just documented + tested as a contract)
- Outbox routing on every read/write (M2)
- Subscription dedup + merge + auto-close (M2)
- Provenance preserved across redeliveries (M3)
- DM-private events refuse non-inbox relays (M9 — deferred but the structural ban stays in M2 outbox planner)
- Profile placeholders → in-place refinement (D1; already done)
- Watermarks consulted before issuing REQ for historical (M4)
- Bunker:// onboarding (M6)
- Session activity = view rebuild without imperative dance (M8)
- Reactive recompute on event arrival (already done; validated by reactivity-bench)

The deliverable: `docs/design/framework-magic.md` (the contract) + `crates/nmp-testing/tests/framework_magic_contract.rs` (one test per bullet). The contract evolves alongside milestones; each milestone owner adds a "framework-magic delta" section to their exit-gate report.

## What must precede the next plan revision

Four parallel exploration agents (dispatched at the timestamp on this file's commit), each producing notes in `docs/research/`:

1. **Applesauce explorer** — full deep-dive on `/private/tmp/nostr-research/applesauce` (git-tracked, May 2026). Focus: outbox impl, event-store query builders, subscription auto-tracking on kind:3, signer flows, missing features beneficial for NMP. Include git-log archaeology for known gotchas.
2. **NDK explorer** — same on `/Users/pablofernandez/Work/NDK-nhlteu` (git-tracked, April 2026). Plus the prior outbox research at `/Users/pablofernandez/outbox-research/nostr-outbox-analysis/ndk/`.
3. **Framework-magic designer** — define the "things that just work" contract + the kernel-substrate test scaffolding for it.
4. **Highlighter explorer** — survey `/Users/pablofernandez/Work/hl/app` (existing Rust+Kotlin+Swift), map to NMP extension-module shape, identify what `nmp-nip29` needs.

When all four land, a plan-revisor agent folds the findings into `docs/plan/m2-subscription-compilation.md`, `docs/plan/m6-signers-write.md`, the new `docs/plan/m11.5-highlighter.md`, the new `docs/design/framework-magic.md`, and removes M9 + M12 from the active ladder (moving them to a `docs/plan/post-v1.md` companion).

## Why these specific deferrals are safe

- DMs deferred ≠ no privacy at v1. The outbox planner's structural ban on routing private events to non-inbox relays (M2) stays. When DMs land post-v1, they slot in cleanly because the routing contract is already there.
- Wallet deferred ≠ no zaps. Zaps without NWC are still possible via LUD-16, but the v1 demos don't need that either. When wallet lands post-v1, NIP-57 ships with it.
- These deferrals shrink v1 surface area and let M10.5 + the two app-rebuilds (podcast + highlighter) be the load-bearing kernel-boundary proofs they need to be.

## Status of v1 ladder after these adjustments

```
M0    Kernel substrate + fixture                                      ✅ DONE
M1    Chirp social baseline on iOS                                    🟡 hardening
M2    Subscription compilation + outbox + KIND:3 auto-tracking        — design + impl
M3    Persistence (LMDB) + insert invariants                          — design + impl
M4    NIP-77 negentropy sync                                          — pending
M5    NIP-42 relay auth                                               — pending
M6    Sessions + signers (incl. bunker:// + nsec creation) + write    — pending
M7    Reactions + Thread + Reply                                      — pending
M8    Multi-session                                                   — pending
~~M9 DMs~~                                                            ↓ post-v1
M10   Blossom + media + long-running capabilities                     — pending
M10.5 FFI hardening + iOS empirical proof                             — design done
M11   ../podcast rebuild on NMP                                       — design done
M11.5 ../hl/app (Highlighter) rebuild + nmp-nip29 crate               ← NEW
~~M12 Wallet~~                                                        ↓ post-v1
M13   Web-of-Trust                                                    — pending
M14   UniFFI migration                                                — pending
M15   Cross-platform (Android + Desktop + Web)                        — pending
M16   CLI + starter app + recipes                                     — pending
M17   v1 release                                                      — pending
```

Plus the cross-cutting "framework magic" contract that grows across M2 + M6 + M8 with its own dedicated test suite.

This file is informational and is folded into `docs/plan/` when the four research agents land and the plan-revisor runs.
