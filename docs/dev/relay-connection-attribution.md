# Relay Connection Attribution — Product Spec, Architecture & Implementation Plan

Status: Proposed — Revision 2 (codex-validated). Revision 1 was found unsound by a codex architecture review; this revision fixes 5 problems (per-lane author capture, the diagnostic/wire split for blocked relays, the missing blocked-cache composition wiring, the router-owned kind:10006 builder for D0, and the full codegen chain).
Author: Principal architect (design pass) + codex review
Date: 2026-06-17
Area: `area:core`, `area:ffi`, `area:ios`, `doctrine:d0`
Related code: `crates/nmp-core/src/kernel/relay_diagnostics.rs`, `crates/nmp-planner`, `crates/nmp-router/src/blocked_relays.rs`, `ios/Chirp/Chirp/Features/RelayDetailView.swift`

---

## 0. One-paragraph summary

When the user taps a relay in Chirp ▸ Diagnostics, the relay detail view must explain **why** NMP is connected (or attempting to connect) to that relay: it is an app/account relay, it is in the outbox (NIP-65 write relays) of people we follow, we followed a relay hint, and/or we have an active interest (kinds + authors) routed there — and any combination of these at once. The user can **block** the relay from that screen (writes the relay into the active account's kind:10006 blocked-relay list and republishes it). A blocked relay **stays visible** in diagnostics with status `Blocked` and still shows why it *would* have been connected. The doctrine is non-negotiable: NMP's planner already knows the answer; we materialize that answer as an extension of the **existing** `relay_diagnostics` projection and let Chirp render it. No app-layer attribution logic, no parallel mechanism.

---

## 1. Product spec

### 1.1 Connection-reason variants

A relay's detail view shows a list of **reasons**. Each reason is one of the variants below. Multiple reasons can apply to the same relay simultaneously and are all listed.

| Variant | Meaning | Supporting data carried |
|---|---|---|
| **App relay** | Explicitly configured by the app or user (account read/write relay, indexer, wallet relay, debug, bootstrap default). | Sub-category label (`Account read`, `Account write`, `Indexer`, `App relay`, `Bootstrap`, `Debug`). |
| **Outbox** | The relay is a NIP-65 write relay of one or more people we care about. | List of author pubkeys whose outbox includes this relay, plus the **total count**. (List is capped for display; count is exact.) |
| **Relay hint** | We followed a `nevent`/`nprofile` relay hint, or a relay hint embedded in an event. | Hint origin: the originating event id (when the hint came from an event tag) and/or the hint kind (`pointer hint` vs `event-embedded hint`). |
| **Interest** | We have an active subscription/interest routed here (e.g. "load kind:1 from pubkeys 1,2,3"). | Pre-formatted kinds label (`kind:0, kind:1`), the author pubkeys (capped) and **total count**, the originating logical-interest key. |

Notes:
- The variant set is deliberately the *app-facing* projection of the planner's internal `RoutingSource` lanes (`crates/nmp-planner/src/plan.rs:93`). "App relay" collapses the planner's `UserConfigured(UserConfiguredCategory)` lane (`plan.rs:28`: `AccountRead | AccountWrite | Indexer | AppRelay | Debug | Bootstrap`); "Outbox" = `Nip65`; "Relay hint" = `Hint` + `Provenance`; "Interest" is the synthesized "we are tailing kinds X for authors Y here" view derived from the per-relay sub-shape. The mapping is one-way and lives in Rust.

### 1.2 Presentation of combinations + counts

- Reasons render as a vertically stacked `reasonsSection` on `RelayDetailView`, one card per reason, in a fixed priority order decided in Rust (App relay → Outbox → Relay hint → Interest), so the shell never sorts.
- Each reason card carries a **shell-derived headline label** computed from the raw `kind` token on the wire (consistent with the existing diagnostics projection contract — `relay_diagnostics.rs:69-84`; tone is derived by the shell, not emitted by Rust) plus its structured payload.
- Pubkey lists are **capped at a small display N** (proposed: 8) with an exact `total` count so the UI can render "alice, bob, … +142 (150 total)". Capping happens in Rust; the cap and the total are both projection fields.
- Kinds render from a Rust pre-formatted `kinds_label` (mirroring the existing `discovery_kinds_label` precedent — `relay_diagnostics.rs:118-121`) so the shell never switches on kind numbers (aim.md §4.5; ADR honored by the existing projection).

### 1.3 Block-relay UX

- `RelayDetailView` gains a **Block relay** button (destructive role).
- Tapping it dispatches a single app→kernel action (§3.4). The action adds the relay's canonical URL to the **active account's** kind:10006 blocked-relay list and republishes the kind:10006 event.
- Idempotent: blocking an already-blocked relay is a no-op (the kind:10006 builder/guard returns `None`, no publish — §3.4).
- The button's terminal verdict surfaces through the existing `action_results` projection (the same correlation-id mechanism used by `nmp.nip65.publish_relay_list` — `crates/nmp-router/src/publish_relay_list.rs`).
- An **Unblock** affordance is the symmetric removal (remove the `relay` tag, republish). This is included for completeness; the empty-list guard (§3.4) ensures unblocking the last entry republishes an empty kind:10006 deliberately, not by accident.

### 1.4 Blocked-still-shown behavior

- A blocked relay **remains a row** in both `DiagnosticsView`'s relay list and reachable via `RelayDetailView`.
- Its connection status renders as `Blocked`, distinct from `Disconnected` (the wire carries the raw `connection` token `"blocked"`; the shell derives the label and tone from that token).
- It **still shows its reasons** (Outbox / Interest / Hint / App relay) — i.e., why it *would* be connected — because attribution is computed by the planner *before* the blocked-relay subtractive filter is applied (§3.3). We compute "why", then separately decide "but we don't connect".
- A relay that is blocked **and** has no other reason (the user blocked something we never wanted anyway) still appears as long as it is in the kind:10006 list, so the user can find and unblock it (§3.3, "blocked-set seeding").

### 1.5 Enable/disable toggle decision

**No toggle.** The attribution computation is already gated by an existing mechanism and is cheap. Justification in §4. Adding a speculative diagnostics-mode flag would violate "single canonical mechanism" and duplicate the ADR-0053 declared-projection gate that already exists. The full perf analysis backing this decision is §4.

---

## 2. Where the "why" already lives

This section is the build-cost determinant. **Key finding: the attribution is computed inside the planner on every recompile, but it is collapsed/discarded before it reaches anything durable. Nothing materializes per-relay, per-reason attribution today.** There are two routing subsystems; only one is the source of truth for *standing* connections.

### 2.1 System A — the live subscription planner (source of truth for standing connections)

The path that compiles logical interests into the wire REQs we actually keep open. This is what determines which relays we tail, and therefore the authority for "why are we connected".

- **`RelayEntry`** (`crates/nmp-planner/src/compiler/partition/mod.rs:58-71`) is the per-`(relay, interest)` slice produced by Stage 1 partitioning. Crucially it already carries **`authors_for_relay: BTreeSet<Pubkey>`** and **`sources: BTreeSet<RoutingSource>`** — i.e., *which authors landed on this relay and via which lanes*. The doc-comment at `mod.rs:48-57` explicitly states `sources` is a set so "a relay reached by two different lanes (e.g. NIP-65 for author A, Indexer for author B) preserves both lanes." **This is exactly the attribution we want, and it exists transiently.**
- **Outbox resolution (authors → write relays)** happens in `case_a_authors::route` (`crates/nmp-planner/src/compiler/partition/case_a_authors.rs:124-134`), consulting `MailboxCache::get(author).outbox_relays()` and tagging the lane `RoutingSource::Nip65` at `case_a_authors.rs:132`. The per-relay author accumulator is `per_relay: BTreeMap<RelayUrl, (authors, addresses, sources)>` at `case_a_authors.rs:58` — it maps relay → author set, but **the per-lane→per-author mapping is already lossy here** (codex correction): authors are unioned at `:127` and sources at `:147` into one `(authors, sources)` tuple per relay, so a relay that is NIP-65 for author A *and* AppRelay for author B becomes `authors={A,B}, sources={Nip65, AppRelay}` — you cannot reconstruct "which author came via outbox" after this point. **Implication:** to report exact `outbox_authors`, attribution must be captured *at the lane-tagging site* (where `RoutingSource::Nip65` is assigned, `:124-134`), tracking outbox authors as a distinct per-lane set as they accumulate — NOT reconstructed post-hoc from the collapsed tuple.
- **Relay hints** become `RelayHint { url, source: HintSource }` with the originating event id/tag in `HintSource::EventTag { event_id, tag, position }` and `Provenance { event_id }` (`crates/nmp-planner/src/interest.rs:440-460`), consumed in `case_a_authors.rs:233-259` and `case_d_no_author.rs:42-67`, canonicalized in `compiler/partition/hint_helper.rs:6-45`.
- **THE COLLAPSE**: at Stage 3 merge (`crates/nmp-planner/src/compiler/mod.rs:332-415`, specifically the `role_tags.insert(src)` union at `mod.rs:364-366`), the per-author lane mapping and the hint's originating event id are **flattened into a relay-level `BTreeSet<RoutingSource>` (`role_tags`)** and `RelayEntry` is consumed by `into_shape` (`partition/mod.rs:75-91`), which folds `authors_for_relay` back into `base_shape.authors`. The bare `Hint`/`Provenance` lanes drop the event id (`hint_helper.rs:13-19`).
- **What survives** on the durable `CompiledPlan`:
  - `CompiledPlan` (`crates/nmp-planner/src/plan.rs:205`): `per_relay: BTreeMap<RelayUrl, RelayPlan>`.
  - `RelayPlan` (`plan.rs:183`): `role_tags: BTreeSet<RoutingSource>` (relay-level lane union) + `sub_shapes: Vec<SubShape>`.
  - `SubShape` (`plan.rs:129`): `shape: InterestShape` (carries the per-relay `authors` + `kinds`) + `originating_interests: Vec<InterestId>`.
  - So **post-compile we retain**: per-relay authors, per-relay kinds, per-relay lane *set*, and originating interest ids. **We lose**: which author came via which lane, and the hint's originating event id.
- **Post-compile narrowing (codex correction).** After Stage 3, `apply_selection_with_lookup` (`crates/nmp-planner/src/selection.rs:296,319`) can DROP relays and NARROW `sub.shape.authors` (coverage/selection trimming) before the plan becomes authoritative. So even pre-merge attribution captured during partition must be **pruned in lockstep with selection** — otherwise it reports relays/authors that selection removed and that carry no standing REQ.
- **The plan is held live** on the subscription lifecycle as `current_plan: Option<CompiledPlan>` (`crates/nmp-core/src/subs/mod.rs:227`, written at `crates/nmp-core/src/subs/recompile.rs:199`, after selection). Diagnostic accessors already exist on it: `current_plan_frames()` and `current_plan_unroutable()` (`crates/nmp-core/src/subs/handlers.rs:34,69`). **Critical (codex):** `current_plan_frames()` and `handle_reconnect()` (`subs/handlers.rs:34,95`) materialize wire REQs *directly* from `current_plan` — so `current_plan` is the WIRE-AUTHORITATIVE plan and must never contain relays we refuse to connect to (e.g. blocked). The kernel owns the lifecycle as `lifecycle: SubscriptionLifecycle` (`crates/nmp-core/src/kernel/mod.rs:1007`). **This is the seam we extend — but diagnostic attribution must be a SEPARATE retained snapshot, not `current_plan` itself (see §3.3 split model).**

### 2.2 System B — the substrate router + routing-trace (NOT the standing authority)

A richer 7-lane attribution model that *does* materialize, but only for publish-resolution and kernel-driven one-shot REQs, into a bounded ring buffer:

- `RoutedRelaySet { relays: BTreeMap<RelayUrl, BTreeSet<RoutingSource>>, kind_overrides }` (`crates/nmp-core/src/substrate/routing.rs:185`) with a 7-lane `RoutingSource` (`routing.rs:87`).
- `RoutingTraceProjection` (`crates/nmp-core/src/kernel/routing_trace.rs:78`) — two bounded `VecDeque` ring buffers, default capacity **64** (`routing_trace.rs:44`), oldest-dropped. Each entry has `urls: Vec<(RelayUrl, BTreeSet<RoutingSource>)>` plus `RouteAttempt { lane, outcome }` chains (`crates/nmp-core/src/substrate/routing_trace.rs:71-173`). This is the "why did event Y go to relay B?" feature (#968).

**Why System B is not the source of truth for this feature:** it is (a) keyed by route *call*, not by standing connection; (b) a 64-entry transient ring, so an old subscription falls off; (c) driven for publish + one-shot/discovery REQs via `crates/nmp-core/src/kernel/mailboxes.rs:207-432`, **not** for the bulk tailing follow-feed REQs that `SubscriptionCompiler` emits. We do **not** build the feature on System B. (We may, post-v1, unify the two `RoutingSource` enums — flagged in §6.)

### 2.3 Bottom line for build cost

- Attribution at the granularity the product wants (relay → {lane, authors, kinds, hint source}) is **partly computed every recompile and thrown away**, but is **not fully materialized even transiently** — the per-lane→author mapping collapses in the Case A accumulator (§2.1) before `RelayEntry`.
- To deliver the feature we must: (1) **capture** per-lane attribution at the accumulation/lane-tagging sites (outbox authors as a distinct set; hint origin event ids that `hint_helper.rs:13-19` currently drops); (2) **carry** it through Stage-3 merge instead of collapsing into `role_tags`; (3) **prune** it in lockstep with `apply_selection_with_lookup` so it matches the standing plan; (4) **retain** it as a diagnostic snapshot SEPARATE from the wire-authoritative `current_plan` (§3.3 split). This is *mostly* retention plus a small amount of additional capture at known sites — still far cheaper than a separate recompute, but it is **not** literally "carry an already-complete struct," as the first draft implied.

---

## 3. Architecture

### 3.1 Design principle

One mechanism, end-to-end:

```
SubscriptionCompiler — capture per-lane attribution at the lane-tag sites   [NMP planner]
   └─ carry through Stage-3 merge; prune in lockstep with apply_selection
        └─ recompile SPLITS into two outputs:                                [nmp-core subs]
             ├─ (A) diagnostic attribution snapshot (pre-block, post-select) ─┐
             └─ (B) current_plan = wire-authoritative, BLOCK-FILTERED ────────┤ both from one compile
        kernel reads (A) via current_plan_attribution() accessor             │
             └─ relay_diagnostics projection adds `reasons` per row  ◄────────┘ [existing projection]
                  └─ full codegen chain: .fbs → Rust+Swift readers, model, DTOs, glue (ADR-0037)
                       └─ Chirp renders reasonsSection + Block button         [thin shell]
```

No new projection key, no new FFI snapshot, no app logic. We extend the **existing** `relay_diagnostics` projection (`crates/nmp-core/src/kernel/relay_diagnostics.rs`) — which already pre-rolls every relay row with raw protocol tokens (shells derive labels/tones), and is decoded by Chirp at `ios/Chirp/Chirp/Bridge/TypedProjectionGlue.swift:225-291`.

### 3.2 Planner: retain attribution (no recompute)

Add a structured attribution value that rides alongside `role_tags` on `RelayPlan`. It is populated from the per-relay data that `RelayEntry`/`case_a_authors` already accumulates, captured **before** the Stage-3 collapse.

Proposed types in `crates/nmp-planner/src/plan.rs` (new module `plan/attribution.rs` if `plan.rs` approaches the file-size gate — see §5 risk):

```rust
/// Per-relay provenance retained for diagnostics. Computed during partition,
/// preserved through Stage-3 merge instead of being collapsed into role_tags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelayAttribution {
    /// UserConfigured sub-categories that placed this relay (App relay).
    pub user_configured: BTreeSet<UserConfiguredCategory>,
    /// NIP-65 outbox: authors whose write-relay set includes this relay.
    pub outbox_authors: BTreeSet<Pubkey>,
    /// Relay-hint origins that pointed here (event id when known).
    pub hints: BTreeSet<HintOrigin>,
    /// Interest provenance: (interest_id, kinds, authors) routed to this relay.
    pub interests: Vec<InterestAttribution>,
}
```

- `HintOrigin` keeps `HintSource`'s event id (currently dropped at `hint_helper.rs:13-19`); thread it through instead of discarding.
- `InterestAttribution { interest_id: InterestId, kinds: BTreeSet<u32>, authors: BTreeSet<Pubkey> }` is derived directly from the `RelayEntry` (`base_shape.kinds`, `authors_for_relay`) keyed by `interest_id`.
- Populate during the per-relay accumulation in `case_a_authors.rs` (the `per_relay` map at `:114` already holds `(authors, addresses, sources)` — add the structured attribution there) and Case D, then carry it on `RelayEntry` and merge it (union) at `compiler/mod.rs:332-415` in parallel with `role_tags`, instead of letting `into_shape` drop it. Store the merged `RelayAttribution` as a new field on `RelayPlan` (`plan.rs:183`).
- Add a read accessor on the lifecycle mirroring `current_plan_frames`: `current_plan_attribution(&self) -> BTreeMap<RelayUrl, RelayAttribution>` in `crates/nmp-core/src/subs/handlers.rs` (near `:34`). It clones the per-relay attribution out of `self.current_plan`.

This keeps the planner the single owner of routing provenance (D0: kinds are `u32`, pubkeys are `Pubkey` — no app/protocol-display nouns leak into the planner; formatting stays in the projection layer).

### 3.3 Projection: extend `relay_diagnostics` with `reasons` + `Blocked` status

In `crates/nmp-core/src/kernel/relay_diagnostics.rs`:

1. New struct (app-facing, pre-formatted), serialized like the existing rows:

```rust
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct RelayConnectionReason {
    /// Variant discriminant: "app_relay" | "outbox" | "hint" | "interest".
    pub(super) kind: String,
    /// Pre-formatted headline, e.g. "App relay (Account write)",
    /// "Outbox of 150 people", "Relay hint", "Interest: kind:1".
    pub(super) label: String,
    /// Semantic hue key, reusing the existing tone vocabulary.
    pub(super) tone: String,
    /// Capped author pubkey list (hex). Empty for app_relay/hint.
    pub(super) author_pubkeys: Vec<String>,
    /// Exact author total (>= author_pubkeys.len()).
    pub(super) author_total: u32,
    /// Pre-formatted kinds label ("kind:0, kind:1"). Empty unless interest.
    pub(super) kinds_label: String,
    /// Hint origin event id (hex) when known. None otherwise.
    pub(super) source_event_id: Option<String>,
}
```

2. Add `pub(super) reasons: Vec<RelayConnectionReason>` to `RelayDiagnosticsRow` (`relay_diagnostics.rs:63`). The builder (`relay_diagnostics_snapshot`, `:196`) gains: read the **diagnostic attribution snapshot** (the pre-block, post-selection candidate attribution — see the split model below) once, bucket by URL, and for each relay row build the ordered reason list (App relay → Outbox → Hint → Interest). Capping + label/tone formatting happens here (this is where formatting belongs — the planner stays noun-free). Reuse `format::title_case`, `compact_count`, and add a `kinds_label` helper alongside the existing `discovery_kinds_label` (`relay_diagnostics/discovery.rs`). **File-size:** put the reason-building logic in a new submodule `relay_diagnostics/reasons.rs`.

3. **The split model (codex correction — this is the load-bearing redesign).** The original draft proposed reading `current_plan` and "computing reasons pre-block-filter." That is unsafe: `current_plan` is the WIRE-AUTHORITATIVE plan (`current_plan_frames()`/`handle_reconnect()` emit REQs straight from it, `subs/handlers.rs:34,95`), so it must be filtered to EXCLUDE blocked relays — it cannot also be the pre-block diagnostic source. And today System A applies **no** blocked filter at all (`recompile_and_diff_with_lookup`, `crates/nmp-core/src/subs/recompile.rs:132,199`, reads no blocked set); blocked filtering lives only in System B's router (`crates/nmp-router/src/router.rs:461,502,561` skipping `ctx.blocked_relays`, context from `snapshot_blocked_relays`, `mailboxes.rs:88`). So we introduce a **two-output recompile**:
   - **(A) Diagnostic attribution snapshot** — the per-relay `RelayAttribution` for the **unblocked** candidate plan, post-selection (so it matches what *would* stand if nothing were blocked), retained on the lifecycle as a NEW field `current_plan_attribution: BTreeMap<RelayUrl, RelayAttribution>` (NOT inside `current_plan`). Read by the projection. Includes blocked relays' would-be reasons.
   - **(B) Wire-authoritative plan** — `current_plan`, now additionally filtered to **remove blocked relays** before `plan_diff` and before store/replay, so `current_plan_frames()`/`handle_reconnect()` never emit a REQ to a blocked relay. This adds System-A blocked filtering (reading `snapshot_blocked_relays`) as a post-compile, post-selection step in `recompile_and_diff_with_lookup`, immediately before `current_plan` is written (`recompile.rs:199`). The wire plan and the diagnostic snapshot are produced from the same compile; they differ only by the block subtraction.
   This makes "compute why, then decide whether to connect" a real two-output step, not an unsafe single unfiltered plan. The router's existing System-B block filter (`router.rs`) stays as defense-in-depth.

   **Recompile-invalidation corrections (codex Rev-2 review — REQUIRED for blocking to take effect):**
   - **Fingerprint the blocked set in the compile memo.** `recompile_inner`'s memo fingerprint (`subs/recompile.rs:~134`) hashes interests / mailbox generation / dead relays / relay lists / etc. — but NOT the blocked set. A kind:10006-only change (a block) would otherwise leave the fingerprint unchanged, the memo guard returns the cached plan early, SPLIT B never re-runs, and the newly-blocked relay keeps its REQ. Hash the blocked set (or a generation counter) into the fingerprint so a block forces a real recompile.
   - **Enqueue a recompile when the blocked set changes.** The post-ingest transition sweep (`kernel/ingest/projection.rs`) observes mailbox / DM-relay / profile / contacts transitions but not blocked relays. A blocked-cache update (from the republished kind:10006 round-trip) must enqueue a recompile trigger (`CompileTrigger::BlockedRelaysChanged`, or an existing invalidate) for the active account — otherwise System-A filtering waits for an unrelated trigger.
   - **Complete `InterestAttribution` pruning.** `selection.rs` prunes `outbox_authors` and each `InterestAttribution.authors`, but must ALSO drop `InterestAttribution` entries whose author-shaped subshape was fully removed by selection (rebuild against surviving `sub_shapes`), so the diagnostic snapshot never reports an interest that carries no standing REQ.

4. **Blocked status + blocked-set seeding.** In the projection builder:
   - Read `self.snapshot_blocked_relays()` (`crates/nmp-core/src/kernel/mailboxes.rs:88`) once.
   - For any relay URL whose canonical form is in the blocked set, emit the raw `connection` token as `"blocked"` (the shell derives the muted hue and "Blocked" label from it — there is no kernel `connection_tone`/`connection_label`; #1802 moved tone/labels to the shells). The relay keeps its `reasons` — sourced from the diagnostic snapshot (A), which retained the would-be attribution.
   - **Seed the row order set** (`order` in `relay_diagnostics_snapshot`, `:207`) with: (a) `relay_statuses`, (b) `wire_subs`, (c) the diagnostic attribution snapshot URLs, **and (d) blocked-set URLs**. (c) guarantees a would-be-connected relay shows before a wire sub opens; (d) guarantees a blocked relay the planner never routed to is still findable for unblock.

5. **FlatBuffers sidecar (ADR-0037) — full codegen chain (codex correction: NOT "just add a field").** Adding `reasons` touches the entire generated pipeline, all of which must be regenerated/kept in sync: (1) `crates/nmp-core/schema/relay_diagnostics.fbs` — add a `RelayConnectionReason` table + `reasons:[RelayConnectionReason]` on `RelayDiagnosticsRow`; (2) the generated Rust reader/writer; (3) the generated Swift reader (`ios/Chirp/Chirp/Bridge/Generated/RelayDiagnostics.generated.swift`); (4) the Rust typed model + encoder + decoder for the projection; (5) the Swift DTOs (`KernelSnapshotTypes.swift`); (6) the glue (`TypedProjectionGlue.swift`). Regenerate via `crates/nmp-codegen`. The kernel already captures the produced struct once per tick into `captured_relay_diagnostics` (`crates/nmp-core/src/kernel/update/projections.rs:274`) so JSON + typed forms stay byte-identical; add an ADR-0037 parity test for a row carrying reasons. The `builtin_projection_keys_const_matches_runtime` gate is unaffected (no new key); the **generated-code drift gate IS affected** and must pass.

### 3.4 Block-relay FFI action

Mirror the kind:10002 relay-list edit precedent (`maybe_publish_relay_list_after_edit`, `crates/nmp-core/src/actor/dispatch.rs:154`; `build_relay_list_event`, `crates/nmp-core/src/actor/commands/relays.rs:102`; `publish_unsigned_event`, `crates/nmp-core/src/actor/commands/publish.rs:112`) — but with TWO codex-mandated corrections.

**Prerequisite (codex correction — currently MISSING, must be added first).** `nmp-defaults` installs only `Kind10002Parser` (`crates/nmp-defaults/src/tiers.rs:236`). There is **no** default `InMemoryBlockedRelayCache` + `Kind10006Parser` + `set_blocked_relay_lookup` registration — the kernel holds only a read-only `Arc<dyn BlockedRelayLookup>` (`crates/nmp-core/src/substrate/blocked_relays.rs:56`) with **no mutation API**. So the original "upsert into the cache the kernel holds" is not implementable today. Step 0 of the plan adds this composition wiring (mirror the kind:10002/mailbox cache install): construct an `InMemoryBlockedRelayCache`, register `Kind10006Parser` on the ingest dispatcher, and `set_blocked_relay_lookup` so reads + the tailing-republish round-trip actually populate it. Without this, blocking has no effect even before any UI.

**D0 boundary (codex correction).** The kind:10006 wire shape (kind number, `["relay", <url>]` tags) lives in `nmp-kinds` + `nmp-router`, and `blocked_relays.rs:40` states `nmp-core` never names it. So the event **builder must NOT live in `nmp-core`** (the original `commands::build_blocked_relays_event` in `nmp-core` violated this). Instead:
- The **edit + builder** is a **router-owned `ActionModule`** (`nmp.nip51.block_relay` / `unblock_relay`) in a new `crates/nmp-router/src/block_relay.rs`, mirroring `publish_relay_list.rs` (`crates/nmp-router/src/publish_relay_list.rs`). The module reads the current blocked set, applies the edit, builds the kind:10006 unsigned event in the wire shape `parse_blocked_relay_list` consumes (`blocked_relays.rs:170`) with `created_at = 0` (D7 sentinel), and hands the unsigned event to the kernel's generic sign+publish seam. `nmp-core` stays wire-shape-agnostic.
- The kernel exposes a **mutation seam** for the in-memory blocked cache — either extend the `BlockedRelayLookup` trait family with an `upsert`/`remove` (the concrete `InMemoryBlockedRelayCache` already has `upsert` at `blocked_relays.rs:84`; surface it through a substrate writer trait the way the mailbox/contacts caches expose their writers), or let the optimistic local edit ride entirely on the published-event round-trip. Prefer the published-event round-trip as the source of truth (the republished kind:10006 re-ingests through `Kind10006Parser` and re-upserts the same `Arc<InMemoryBlockedRelayCache>`), with an optional optimistic local upsert for latency — same pattern as the kind:3 follow edit.

**Guards** (mirror `maybe_publish_relay_list_after_edit`): active signer present; projection changed (idempotent no-op block of an already-blocked relay); builder returns `Some` (never publish a destructive list built from nothing); unblocking the last entry republishes an empty kind:10006 deliberately.

**Round-trip:** the republished kind:10006 returns through the cold-start tailing subscription (`SELF_KINDS_TAILING = &[0,3,10002,10000,10006]`, `crates/nmp-core/src/kernel/requests/startup.rs:30`) → `Kind10006Parser` (`blocked_relays.rs:129`) → re-upserts the cache. The System-A wire-plan block filter (§3.3 split, B) drops the relay from `current_plan` on the next recompile; the diagnostics projection flips it to `Blocked` while retaining its reasons from the diagnostic snapshot (§3.3, A).

**Terminal verdict** lands in `action_results` via the registry-minted `correlation_id` (same as `nmp.nip65.publish_relay_list`).

**Chirp dispatch:** the app sends raw intent through the existing `nmp_app_chirp_action_spec` → `nmp_app_dispatch_action` flow (`ios/Chirp/Chirp/Bridge/KernelBridge.swift:551-590`). Add `blockRelay(url:)`/`unblockRelay(url:)` factories to `ChirpActionIntent` (`ios/Chirp/Chirp/Bridge/ChirpActionSpecBridge.swift:40-178`, alongside `follow`/`unfollow`) mapping to `{namespace: "nmp.nip51.block_relay", body_json}`, plus a `model.blockRelay(url:)` convenience (`KernelModel.swift:628`). No new FFI symbol — reuses the dispatch flow `follow` uses end-to-end (`ProfileView.swift:72` → `ChirpActionSpecBridge.swift:75` → `KernelModel.swift:638` → `KernelBridge.swift:551`).

### 3.5 Chirp shell (render only)

- `RelayDetailView` (`ios/Chirp/Chirp/Features/RelayDetailView.swift`) gains a `reasonsSection` iterating `row.reasons` (new field on `RelayDiagnosticsRow`, `ios/Chirp/Chirp/Bridge/KernelSnapshotTypes.swift:407`), each a card rendering `reason.label` (tinted by `reason.tone` via existing `DiagnosticsColor.color(forTone:)`), the capped `author_pubkeys` + `author_total` (reuse the shared npub chip), `kinds_label`, and an optional source-event row.
- A **Block relay** button (and `Unblock` when `row.connectionLabel == "Blocked"`) calling `model.blockRelay(url:)`.
- The `Blocked` status renders automatically — `connectionLabel`/`connectionTone` are already consumed (`RelayDetailView.swift:38-141`); the new `"Blocked"`/`"muted"` values need no Swift change beyond the tone map already handling `"muted"`.
- Glue: add `reasons` mapping in `ios/Chirp/Chirp/Bridge/TypedProjectionGlue.swift:232-259` (`relayDiagnosticsRow`).

---

## 4. Performance analysis (and why no toggle)

### 4.1 Where the cost is

Two phases:

1. **Retention at compile time (planner).** The attribution data (`authors_for_relay`, per-lane `sources`, hint origins) is *already materialized on the stack* during partition (`case_a_authors.rs:114`, `RelayEntry`). Today it is dropped at `compiler/mod.rs:364`. Retaining it is O(relays × authors-per-relay) additional `BTreeSet`/`Vec` clones held on `RelayPlan` — the same order as `role_tags` + `sub_shapes` we already keep. Recompile is **event-driven** (interest add/remove, mailbox/NIP-65 update — `crates/nmp-core/src/subs/recompile.rs`), not per-tick. Added cost: a few extra allocations per recompile and a bounded memory increment on `current_plan`. Negligible.

2. **Formatting at projection time (kernel).** `relay_diagnostics_snapshot` runs when a snapshot is pulled (~4 Hz). Building `reasons` adds, per relay: read the pre-bucketed attribution map, cap author lists at N=8, format labels. This is O(relays + total-authors-capped). For realistic relay counts (tens) and capped author lists this is sub-millisecond.

### 4.2 The toggle already exists — it's the ADR-0053 declared-projection gate

`relay_diagnostics` is only computed when the host declares/consumes it: the producer is guarded by `declared.permits("relay_diagnostics")` (`crates/nmp-core/src/kernel/update/projections.rs:268`; gate at `:87`). Chirp only subscribes to `relay_diagnostics` while the Diagnostics screen is mounted. So the *expensive* (formatting) phase already runs **only when the Diagnostics UI is open**. This is precisely the "diagnostics mode" toggle the product owner asked us to consider — and it exists, is canonical, and requires no new flag. Phase 1 (retention) runs always but is negligible (§4.1).

### 4.3 Decision

**Do not add a toggle.** It would (a) duplicate the declared-projection gate (violating "single canonical mechanism"), (b) add an `AppAction`/state flag with no measurable benefit, and (c) the only always-on cost (compile-time retention) is below noise. If profiling ever shows compile-time retention matters (it won't at v1 relay/author scale), the natural lever is to compute `RelayAttribution` lazily *only* when `declared.permits("relay_diagnostics")` — but that requires the kernel to re-run partition, trading memory for CPU, and is explicitly **not** recommended now. Document this as the escape hatch, do not build it.

---

## 5. Implementation plan (ordered, file-by-file, each with its proving test)

Each step is independently compilable/testable. Tests are scoped per crate (`cargo test -p <crate>`) plus the always-on `cargo test -p nmp-testing --test doctrine_lint_smoke` (per CLAUDE.md). Do **not** run `cargo test --workspace`.

### Phase 1 — Planner: retain attribution

**Phase 0 — Composition prerequisite (codex correction — do FIRST).**

0. **`crates/nmp-defaults/src/tiers.rs`** (`:236`, where `Kind10002Parser` is installed): install the blocked-relay backend — construct an `InMemoryBlockedRelayCache`, register `Kind10006Parser` on the ingest dispatcher, and `set_blocked_relay_lookup` so the cache is both read (routing) and written (tailing-republish round-trip). Until this lands, blocking is a no-op regardless of UI.
   *Test:* `nmp-defaults`/`nmp-core` integration test: ingesting a kind:10006 with `["relay", url]` populates the blocked lookup the kernel reads (`snapshot_blocked_relays` returns the url).

### Phase 1 — Planner: capture + retain attribution

1. **`crates/nmp-planner/src/plan.rs`** (or new `plan/attribution.rs` if near the file-size gate): add `RelayAttribution`, `HintOrigin`, `InterestAttribution`; add `attribution: RelayAttribution` to `RelayPlan` (`:183`). Derive `Default`.
   *Test:* `nmp-planner` unit test: `RelayPlan::default().attribution` is empty; round-trips.
2. **`crates/nmp-planner/src/compiler/partition/hint_helper.rs`** (`:6-45`): stop dropping the `HintSource` event id; produce a `HintOrigin` carrying it.
   *Test:* a hint from an event tag yields a `HintOrigin` with the originating event id.
3. **`case_a_authors.rs`** (capture at the lane-tag sites `:124-134`, NOT post-hoc from the collapsed `(authors, sources)` tuple at `:58/:127/:147`) and **`case_d_no_author.rs`** (`:42-67`): as each lane is assigned, accumulate per-lane attribution — `outbox_authors` as a DISTINCT set when tagging `RoutingSource::Nip65`, interest kinds/authors, hint origins — and carry it on `RelayEntry`.
   *Test:* a relay that is NIP-65 for author A **and** AppRelay for author B yields `outbox_authors = {A}` exactly (not `{A,B}`) — the per-lane fidelity the accumulator union loses.
4. **`crates/nmp-planner/src/compiler/mod.rs`** (`:332-415`): merge (union) `RelayAttribution` in parallel with `role_tags`; store on `RelayPlan` instead of discarding.
   *Test:* a relay reached by NIP-65 (author A) + `AppRelay` simultaneously yields `attribution.outbox_authors = {A}` **and** `attribution.user_configured ⊇ {AppRelay}`.
5. **`crates/nmp-planner/src/selection.rs`** (`:296,:319`): prune `RelayAttribution` in lockstep with `apply_selection_with_lookup` — when a relay is dropped or its `authors` narrowed, narrow/drop its attribution to match, so retained attribution never claims relays/authors absent from the standing plan.
   *Test:* an author trimmed out by selection is also removed from that relay's `outbox_authors`; a relay dropped by selection has no attribution entry.

### Phase 2 — Kernel: split outputs + expose attribution + extend projection

6. **`crates/nmp-core/src/subs/recompile.rs`** (`:132,:199`): implement the §3.3 SPLIT. After selection, (A) snapshot per-relay `RelayAttribution` for the unblocked candidate into a NEW lifecycle field `current_plan_attribution` (`subs/mod.rs:227` neighborhood); (B) filter blocked relays (`snapshot_blocked_relays`) out of the plan BEFORE `plan_diff` and before writing `current_plan` (`:199`), so the wire-authoritative plan never carries a blocked relay.
   *Test:* `nmp-core` subs test: block a relay in a followed author's outbox → `current_plan_attribution` still contains it with `outbox_authors` non-empty, but `current_plan_frames()` emits NO REQ to it.
7. **`crates/nmp-core/src/subs/handlers.rs`** (near `:34`): add `current_plan_attribution(&self) -> BTreeMap<RelayUrl, RelayAttribution>` reading the new field (NOT `current_plan`).
   *Test:* mirrors `current_plan_frames` tests; empty before first compile.
8. **`crates/nmp-core/src/kernel/relay_diagnostics.rs`** + new **`relay_diagnostics/reasons.rs`** + **`/format.rs`** + **`/discovery.rs`**: add `RelayConnectionReason`, `reasons` field, the ordered builder (App relay → Outbox → Hint → Interest) reading the diagnostic snapshot, a `kinds_label` formatter, `Blocked` label/tone, blocked-set seeding ((c) snapshot URLs + (d) blocked URLs into `order`).
   *Test:* given a stubbed snapshot + blocked set, the row carries ordered `reasons`, capped author list + exact total, and a blocked relay shows `connection_label == "Blocked"` while retaining reasons.

### Phase 3 — Typed sidecar / codegen (FULL chain)

9. **`crates/nmp-core/schema/relay_diagnostics.fbs`** → regenerate ALL: generated Rust reader/writer, generated Swift (`RelayDiagnostics.generated.swift`), Rust typed model+encoder+decoder, Swift DTOs (`KernelSnapshotTypes.swift`), glue (`TypedProjectionGlue.swift`). Via `crates/nmp-codegen`.
   *Test:* generated-code drift gate passes; ADR-0037 parity test — JSON and FlatBuffer encodings of a row with `reasons` agree.

### Phase 4 — Block-relay action (router-owned builder; nmp-core stays wire-agnostic)

10. **`crates/nmp-router/src/block_relay.rs`** (new): `nmp.nip51.block_relay` / `unblock_relay` `ActionModule` mirroring `publish_relay_list.rs` — reads the current blocked set, applies the edit, builds the kind:10006 unsigned event (`["relay", url]`, `created_at=0`), hands it to the kernel's generic sign+publish seam; registered in `register_default_action`; guards (signer present, changed/idempotent, non-empty builder); correlation-id verdict to `action_results`.
    *Test:* `nmp-router` test: spec validates + threads a correlation id; idempotent re-block is a no-op; non-`wss` URL → terminal error; unblock of last entry republishes an empty list.
11. **Kernel sign+publish seam:** the action's unsigned kind:10006 routes through the existing generic `publish_unsigned_event` (`crates/nmp-core/src/actor/commands/publish.rs:112`); the republished event re-ingests via `Kind10006Parser` (Phase 0) → re-upserts the cache → §3.3(B) drops it from the wire plan. NO `build_blocked_relays_event` in `nmp-core` (D0).
    *Test:* `nmp-core` integration: dispatching the action publishes one kind:10006; the round-trip flips the relay to `Blocked` in `relay_diagnostics` without emitting a REQ to it.

### Phase 5 — Chirp shell

11. **`ios/Chirp/Chirp/Bridge/ChirpActionSpecBridge.swift`** (`:40-178`): add `blockRelay(url:)`/`unblockRelay(url:)` factories. **`KernelModel.swift`** (near `:628`): add `blockRelay(url:)` convenience. **`KernelBridge.swift`**: no new FFI symbol (reuses dispatch flow).
12. **`ios/Chirp/Chirp/Bridge/KernelSnapshotTypes.swift`** (`:407`): add `reasons: [RelayConnectionReason]` + the new Swift struct. **`TypedProjectionGlue.swift`** (`:232-259`): map `reader.reasons`.
13. **`ios/Chirp/Chirp/Features/RelayDetailView.swift`**: add `reasonsSection` + Block/Unblock button. **File-size risk:** if the view grows past the gate, extract `RelayReasonsSection.swift`.
    *Test:* iOS UI/unit test (XCTest) that decodes a fixture `relay_diagnostics` envelope with reasons and asserts the reasons render; a snapshot/UI test that the Block button dispatches the `nmp.nip51.block_relay` intent (assert via the action-spec bridge fake).

### Cross-cutting

- Run `cargo test -p nmp-planner`, `-p nmp-core`, `-p nmp-router`, and `-p nmp-testing --test doctrine_lint_smoke` after each Rust phase.
- File-size gate: this repo splits large files (AGENTS.md). The three highest-risk files are `crates/nmp-core/src/kernel/relay_diagnostics.rs`, `crates/nmp-planner/src/plan.rs`, and `ios/Chirp/Chirp/Features/RelayDetailView.swift` — pre-emptively extract submodules/subviews as noted.

---

## 6. Open questions / risks (need an owner decision)

1. **~~Pre-block-filter attribution location~~ — RESOLVED by the §3.3 split model (codex-validated).** Codex confirmed System A applies no blocked filter today and that `current_plan` directly drives wire REQs (`current_plan_frames`/`handle_reconnect`), so storing an unfiltered plan was unsafe. The resolution is the two-output recompile (§3.3, Phase-2 step 6): a retained **diagnostic attribution snapshot** (pre-block, includes would-be-connected blocked relays) feeds the projection, while the **wire-authoritative `current_plan`** is block-filtered before diff/store. No remaining owner decision — this is the implementation.
2. **Author-list display cap.** Proposed N=8 capped + exact total. Owner to confirm the cap and whether tapping a count should expand the full list (would need a follow-up interaction, possibly a separate projection slice or an on-demand expansion — out of scope for v1 of this feature).
3. **Two `RoutingSource` enums.** The planner's 5-lane enum (`plan.rs:93`) and substrate's 7-lane enum (`routing.rs:87`) are not unified. This feature builds on the planner enum only. The crate-boundary spec flags planner/substrate mailbox-trait convergence (V-40 follow-up) as the place to unify them. **Risk:** if convergence lands mid-flight, the attribution mapping must move with it. Recommend sequencing this feature before or well after V-40, not concurrently.
4. **Interest reason granularity.** The product example is "kind:1 from pubkeys 1,2,3". Post-Stage-3 we retain per-relay kinds + authors but the `originating_interests` are interest ids, not human labels. The projection synthesizes an "Interest" reason from the per-relay sub-shape; confirm the owner is satisfied with kinds + author list (not a named interest like "Home feed"). Naming interests would require a label registry — out of scope.
5. **Hint origin completeness.** Restoring the dropped event id (step 2) covers `HintSource::EventTag`/`Provenance`. For `nevent`/`nprofile` *pointer* hints that arrive without an enclosing event, the origin is the bech32 entity the user opened; confirm whether the projection should carry that pointer context (likely yes — thread the opening context through the interest that the pointer created).
6. **Unblock UX surface.** This doc specifies Unblock symmetrically. Confirm the owner wants Unblock on the same screen (recommended) vs. a separate relay-block-list management screen.
