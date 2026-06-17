# TENEX-edge Proposal — Doctrine "Exception Smell" Audit & Remediation

Status: Proposal
Date: 2026-06-17
Method: 20-agent parallel codebase scan → `codex exec` second-opinion validation of every candidate
Scope: `crates/*` (Rust substrate) + `apps/chirp` + `ios/Chirp`

---

## 0. Summary

A 20-agent fan-out scanned the codebase for **"exception smells"** — places where code carves out a special case *against* one of the project's own universal doctrines instead of following the single canonical mechanism. The raw scan surfaced 44 candidates; after dedup, 13 distinct high-signal candidates were put through an independent `codex exec` review that verified each against the actual code.

**Result: 12 of 13 candidates are REAL smells; 1 is a false positive (already fixed by #1490). Codex additionally found 2 missed HIGH-severity smells.** They cluster into three doctrine families:

1. **D0 — protocol kind-literals / nouns leaking into the substrate** (the largest family; 9 findings).
2. **Single canonical mechanism — parallel/duplicated paths** (3 findings, incl. the highest-blast item).
3. **Store-first / one-chokepoint** (1 live + 1 already-fixed exemplar).

The exemplar that motivated this audit — the bootstrap interests bypassing universal cache-serve (`startup.rs`) — was **already fixed in #1490** and is confirmed gone. This proposal is about generalizing that lesson: *carve-outs that cite a doctrine while violating it are a recurring, findable pattern.*

All file:line references below are codex-corrected against the current tree (`master` @ #1491; #1490 in history).

---

## 1. Findings (codex-validated)

| # | Verdict | Location | Doctrine | Sev | What to do (canonical action) |
|---|---|---|---|---|---|
| 1 | **REAL** | `crates/nmp-core/src/actor/commands/publish.rs:446,473,515,543,588,636` | D0 | **HIGH** | Move kind-specific builders (`publish_profile` k0 / `react` k7 / `follow` k3) to protocol crates; core only signs/publishes a generic `UnsignedEvent`. |
| 2 | **REAL** | `crates/nmp-core/src/kernel/update/views.rs:68,86` | D0 | MED | Move NIP-18 repost projection/parsing (`kind==6` branch + repost fields) into a registered projection parser / `nmp-nip01`. |
| 3 | **REAL** | `crates/nmp-planner/src/interest.rs:396,404` | D0 | MED | Replace `InterestShape::profile_for` (hardcodes kinds 0,3,10002) with a composition/protocol-owned constructor; pass the kind set in. |
| 4 | **REAL** | `crates/nmp-core/src/subs/recompile.rs:204,258,269` | single-mechanism | MED | Represent the mailbox probe as a canonical registered interest (real id, cache-serve/provenance), not an out-of-band `WireFrame` with `KIND_RELAY_LIST` + sentinel `InterestId(u64::MAX)`. |
| 5 | **REAL** | `crates/nmp-marmot/src/projection/ops.rs:621,622,642,648` | D0 | MED | Use `crate::interest::{KIND_GIFT_WRAP, KIND_GROUP_MESSAGE, KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY}` instead of raw 1059/445/30443/443 (`tap.rs` already does). |
| 6 | **REAL** | `crates/nmp-marmot/src/projection/state.rs:84,604` | D0 (SSOT) | MED | Delete local `MLS_KEY_PACKAGE_KIND`/`_LEGACY`; import the public `crate::interest` constants. |
| 7 | **REAL** | `crates/nmp-wot/src/interest.rs:11,17` (canonical at `nmp-kinds/src/lib.rs:93`) | D0 (SSOT) | MED | Re-export `nmp_core::kinds::KIND_MUTE_LIST`; drop the stale "not yet in registry" comment (it *is* in the registry). |
| 8 | **REAL** | `crates/nmp-core/src/kernel/ingest/timeline.rs:158` + `cache_serve/continuation.rs:224` | D0 (SSOT) | LOW | `metric_note_events` `kind==1` duplicated in two paths → use `KIND_SHORT_TEXT_NOTE` / a shared `is_note_kind` helper. |
| 9 | **REAL** | `crates/nmp-content/src/mode.rs:41,44` | D0 | LOW | Centralize content-kind constants (30023/30024/30818) in `nmp-kinds`/`nmp-content`; use them in mode + tests. |
| 10 | **FALSE POSITIVE** | `crates/nmp-core/src/kernel/requests/startup.rs:196,255` | store-first | n/a | Already fixed in **#1490** — bootstrap interests now cache-serve. No bypass remains. |
| 11 | **REAL** | `apps/chirp/nmp-app-chirp/src/ffi/interest_feed.rs:168,227,337,376` | one-chokepoint | MED | Remove the bespoke `seed_author_feed_from_store` / `seed_thread_feed_from_store` pre-seed (replays via `on_kernel_event` before registration); open the interest first and let canonical cache-serve hydrate. (FlatFeed dedups by id, so not duplicate rows — but it is a parallel replay path.) |
| 12 | **REAL** | `crates/nmp-nip60/src/relay.rs:7,51,60,110,144,152` | store-first / one-chokepoint | **HIGH** | Rebuild NIP-60 reads/writes as kernel interests + publish actions through the store/ingest/publish chokepoints. No private `tungstenite` WebSocket stack, no hardcoded `wss://purplepag.es`. (Currently parked; severe if reactivated as-is.) |
| 13 | **REAL** | `crates/nmp-router/src/nip65_resolver.rs:91`, `discovery.rs:13`, callers `router.rs:321,597` | single-mechanism | **HIGH** | One public discovery-kind classifier in `nmp-router`, used by both the resolver and the router (currently re-encoded in 3 places → drift between publish and subscribe routing). |

### Missed HIGH-severity smells (codex additions)

| # | Location | Doctrine | Sev | What to do |
|---|---|---|---|---|
| M1 | `crates/nmp-core/src/kernel/requests/startup.rs:30,97,223` | D0 | **HIGH** | #1490 fixed the store-first bypass, but core still **owns the bootstrap self-kind policy** (`SELF_KINDS_TAILING = [0,3,10002,10000,10006]`). Move the default self-kind set to composition/protocol modules; core consumes configured bootstrap interests only. |
| M2 | `crates/nmp-core/src/actor/commands/identity.rs:1026,1086,1155,1224` | D0 | **HIGH** | `create_account` builds + routes cold-start kind:0 / kind:10002 / kind:3 events directly in core. Core should create the identity and expose a cold-start publish-target seam; protocol/app modules build the unsigned profile / relay-list / contact-list events. |

---

## 2. The pattern (why these recur)

Every HIGH finding shares a signature: **a comment or structure cites a doctrine/ADR as authority while doing the opposite of what the doctrine requires**, or **re-encodes a fact that already has a single source of truth**. The store-first exemplar (#1490) literally said *"ADR-0045 — intentionally does NOT route through cache-serve"* — citing the ADR against its own text. The same shape appears in:

- `nmp-wot/interest.rs:17` — "not yet in the registry" (it is).
- `nmp-marmot/state.rs:84` — local constants "to avoid reaching into internals" (the file *is* the internals).
- the three discovery-kind classifiers — each a local "correct" copy that drifts.

**These are mechanically findable.** The recommendation in §4 includes a lint to catch the class, not just the instances.

---

## 3. Top 5 to fix first (severity × blast radius)

1. **#12 — `nmp-nip60` private relay stack.** Highest architectural divergence (own WebSocket + ingest + hardcoded indexer, entirely outside the kernel). Parked today, but a latent landmine: reactivating it reintroduces a second, unobservable event-acquisition path. Rebuild on kernel interests before it ships.
2. **#1 — core protocol publish handlers (`publish.rs`).** A *live*, high-traffic D0 violation: profile/reaction/follow — the app's most common writes — bake protocol kinds into the substrate. Move to per-NIP `ActionModule`s.
3. **M2 — `create_account` cold-start event construction in core.** Onboarding embeds kind:0/10002/3 wire policy in core. Expose a publish-target seam; let protocol modules build the events.
4. **#13 — duplicate discovery-kind classifiers in `nmp-router`.** Publish-routing and subscribe-routing can silently disagree about what an "indexer/discovery" kind is. Collapse to one classifier.
5. **M1 — core startup self-kind policy.** #1490 fixed *when* we serve; this fixes *who decides the kinds*. Move the bootstrap kind set out of core into composition.

---

## 4. Recommended remediation

- **Sequence:** land the two parked/structural items (#12, #13) and the live-path D0 items (#1, M1, M2) first; the constant-dedup items (#5–#9) are low-risk mechanical PRs that can be batched.
- **One PR per finding** (or per tight cluster, e.g. #5+#6 both in `nmp-marmot`), each with the canonical-action fix + a test that the carve-out is gone.
- **Add a doctrine lint** to `nmp-testing`'s `doctrine_lint_smoke` family that flags: (a) raw protocol kind integer literals in `nmp-core`/`nmp-planner`/substrate crates outside `nmp-kinds` (allowlist the registry), and (b) comments matching the citing-an-ADR-to-violate-it pattern for review. This converts "find the instances" into "prevent the class" — the durable win.
- **Track** each finding as a GitHub issue with `doctrine:d0` / `doctrine:single-mechanism` / `doctrine:store-first` labels and a `priority:*` per the §3 ranking, per the repo's planning discipline.

---

## 5. Provenance

- Discovery: 20 parallel agents, one per crate/subsystem territory, hunting the exception-smell signature.
- Validation: `codex exec` (independent model) read each cited location and returned a per-candidate verdict, corrected file:lines, and the two missed findings. No code was changed by the audit; this is read-only analysis.
- The single confirmed-fixed exemplar (#10) shipped as #1490 (store-first universal; ADR-0045 Revision 3).
