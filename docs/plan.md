# Build & Validation Plan

> Companion to `docs/product-spec.md` (what we ship) and the design docs in `docs/design/` (how each subsystem works). This document defines **the single ladder of milestones**, each one a runnable product that proves a specific architectural claim with real (not modeled) evidence.

> **Four arcs:** Kernel substrate + Nostr social stack (M0–M10) → FFI hardening + iOS empirical proof (M10.5) → kernel-boundary proof with a non-social-domain app (M11, the **`../podcast` rebuild on NMP**) → wallet/WoT + cross-platform + release (M12–M17).

> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.

> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.

---

## Supporting sections

- [Status — where we are right now](plan/status.md)
- [Principles of execution](plan/principles.md)
- [Subsystem coverage matrix + NIP roadmap](plan/subsystem-matrix.md)
- [Parallelization opportunities](plan/parallelization.md)
- [Test pyramid](plan/test-pyramid.md)
- [CI / pre-merge hygiene](plan/ci-hygiene.md)
- [Decision log](plan/decision-log.md)
- [Scope adjustments 2026-05-18](plan/scope-adjustments-2026-05-18.md) — M9 DMs + M12 Wallet deferred; M11.5 Highlighter added; framework-magic contract
- [Post-v1 milestones](plan/post-v1.md) — deferred M9 (DMs) and M12 (Wallet)

---

## The milestone ladder

Each milestone has: **demo product**, **scope (what gets built)**, **subsystem deliverables**, **exit gate (measurable)**, and **runnable artifact**. Estimates are for one experienced developer focused on the work; they are not commitments.

| Milestone | Title | Arc | Status |
|---|---|---|---|
| [M0](plan/m0-fixture.md) | Kernel substrate + non-Nostr fixture | Arc 1 — Social stack | ✅ DONE |
| [M1](plan/m1-twitter-slice.md) | Read-only Twitter slice on iOS | Arc 1 — Social stack | 🟡 Largely done |
| [M2](plan/m2-subscription-compilation.md) | Subscription compilation + outbox routing + kind:3 auto-tracking | Arc 1 — Social stack | |
| [M3](plan/m3-persistence.md) | Persistence (LMDB) + full insert invariants | Arc 1 — Social stack | |
| [M4](plan/m4-negentropy.md) | NIP-77 negentropy sync engine | Arc 1 — Social stack | |
| [M5](plan/m5-nip42.md) | NIP-42 auth | Arc 1 — Social stack | |
| [M6](plan/m6-signers-write.md) | Sessions + signers (incl. bunker:// + nsec creation) + write path | Arc 1 — Social stack | |
| [M7](plan/m7-interaction-loop.md) | Reactions + Thread + Reply | Arc 1 — Social stack | |
| [M8](plan/m8-multi-account.md) | Multi-session (multi-account) clients | Arc 1 — Social stack | |
| ~~M9~~ | ~~NIP-17 DMs + NSE~~ | ~~Arc 1~~ | [deferred post-v1](plan/post-v1.md) |
| [M10](plan/m10-blossom.md) | Blossom + media + long-running capabilities | Arc 1 — Social stack | |
| [M10.5](plan/m10.5-ffi-hardening.md) | FFI hardening + iOS empirical proof | Hard gate | |
| [M11](plan/m11-podcast.md) | Podcast app — kernel-boundary proof | Arc 2 — Kernel boundary | |
| [M11.5](plan/m11.5-highlighter.md) | Highlighter rebuild + `nmp-nip29` crate | Arc 2 — Kernel boundary | |
| ~~M12~~ | ~~Wallet (NWC + zaps + Cashu + nutzaps)~~ | ~~Arc 3~~ | [deferred post-v1](plan/post-v1.md) |
| [M13](plan/m13-wot.md) | Web-of-Trust | Arc 3 — Release | |
| [M14](plan/m14-uniffi.md) | UniFFI migration | Arc 3 — Release | |
| [M15](plan/m15-cross-platform.md) | Cross-platform: Android + Desktop + Web | Arc 3 — Release | |
| [M16](plan/m16-cli-starter.md) | CLI + starter app + recipe book | Arc 3 — Release | |
| [M17](plan/m17-release.md) | v1 release | Arc 3 — Release | |

---

## What this plan is not

- **Not a schedule.** No dates, no person-months. Milestones are sequential; their durations depend on team size and surface complexity. Estimates per milestone are guidance only.
- **Not a marketing roadmap.** v1 ships when M17 gates are met, not on a calendar.
- **Not exhaustive about post-v1 work.** NIP-29 groups, NIP-23 long-form, NIP-71 video, additional protocol modules, additional app demonstrations (Highlighter-lite, TENEX-lite, etc.) are post-v1 — they validate the kernel boundary further but are not v1 deliverables.
- **Not silent about gaps.** The [status doc](plan/status.md) names exactly what is and isn't built. As the ladder progresses, the status doc gets revised so the plan stays honest about state.

The plan exists so that any single milestone can be picked up cold by someone reading this doc + `product-spec.md` + the relevant ADRs and design docs, and they can execute without bothering the rest of the team.
