# Framework Magic §C13 — Best-Effort Rendering

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/product-spec/overview-and-dx.md` §1.5 doctrine D1; `docs/product-spec/subsystems.md` §7.6 (the per-field placeholder table + the `TimelineItem` concrete example); `docs/aim.md` §4.12; `docs/design/view-catalog/profile-timeline-thread-reactions.md`.

The "capabilities" filename in the user's directive maps here, not to capability bridges. The rendering contract is the **rendering capability** the framework grants the app: render now, refine in place, never withhold cached data behind a spinner. (Capability bridges in the technical sense — `KeyringCapability` etc. — are covered as plumbing in C11 and `kernel-substrate.md` §5; they are not themselves a contract bullet because they are infrastructure, not an observable app guarantee.)

## C13. Best-effort rendering: placeholders by construction; in-place refinement

**Statement.** Every display-bearing field of every view payload is **non-`Option`** and carries either an authoritative value or a defined placeholder. When the authoritative value later arrives — a kind:0 for an author, kind:9735 zap receipts for a note, the decrypted body for a DM — the same payload re-emits with the field updated in place. The platform's reactive primitive (`@Observable` / `Flow` / signals) sees the change and only the affected cell re-renders. **No spinner ever gates an already-rendered cell, and no view module ever exposes a `loading: bool` to the platform.**

**Framework does:**

- The placeholder contract at `docs/product-spec/subsystems.md` §7.6 lines 181–192 (the seven-row table: display name → npub-shortened, picture → identicon URI, NIP-05 → empty string, timestamp → "just now", reaction count → 0, zap total → 0 sats, content body → empty string).
- The view-payload typing at `subsystems.md` §7.6 lines 199–222 (the `TimelineItem` example with all fields non-`Option` except the optional `repost_of` / `quote_of` semantic-Option markers).
- The freshness surface at `subsystems.md` §7.6 line 196 (`xxx_freshness: FreshnessHint` is an optional **sibling** field; UI may render a badge; the framework never withholds the value).
- The in-place refinement mechanism: `ViewModule::on_projection_changed` (`docs/design/kernel-substrate.md` §3 lines 148–150). When a kind:0 lands for author X, the kernel's projection cache (a shared cross-view projection) updates X's display name; every view module that lists items by X re-runs `on_projection_changed`, produces a delta, and the wire-emitter sends a `ViewBatch` with the updated field.
- The platform-shadow domain key (`kernel-substrate.md` §3 line 128 `fn key(spec: &Self::Spec) -> Self::Key`) ensures the cell-level re-render is targeted: the platform's reactive primitive updates only the row whose key matches, not the entire list.

**App writes:** nothing. The app renders payload fields directly — `Text(item.author_display)`, `AsyncImage(url: item.author_picture)`. There is no `if has_profile { ... } else { Spinner() }` pattern because the API does not expose `has_profile`; the framework guarantees `author_display` and `author_picture` are always non-empty strings.

**Failure mode prevented:** the entire class of "Nostr-client cold-start UI" bugs `subsystems.md` §1.5 D1 enumerates as ruled out by construction:

- Hiding a post because the author's profile hasn't loaded yet.
- Replacing cached profile metadata with a spinner because "we might have something newer."
- Refusing to render threads because the root event isn't in cache.
- Profile-picture flicker between cached and placeholder.

The bug-extinction surface in `overview-and-dx.md` §3.3 does not have a single numbered bug for this because the failures are UX defects rather than data-corruption bugs, but the doctrine clause D1 is the explicit promise the contract holds.

**Test:** `c13_view_payload_uses_placeholders_then_refines_in_place`. The test:

1. **Placeholders at open:** open `TimelineView { authors: [alice], kinds: [1] }` against a fresh store with no kind:0 for Alice. Insert a kind:1 event by Alice. Assert the payload's `items[0]`:
   - `author_display` matches the expected npub-shortened form for `alice_pk` (compare against `Pubkey::shortened()` output — deterministic).
   - `author_picture` matches the expected identicon URI for `alice_pk` (deterministic from pubkey hash).
   - `author_nip05_domain` is the empty string.
   - `created_at_display` is "just now" (test uses `SimulatedClock` set to the event's `created_at`).
   - `reaction_summary` has 0 reactions; `zap_sats_total` is 0; `reply_count` is 0.
   - The payload contains **no** `loading`, `is_loaded`, `has_profile`, or `freshness_gate` field.
2. **In-place refinement on kind:0:** insert a kind:0 for Alice with `name = "Alice"`, `picture = "https://example/alice.jpg"`, `nip05 = "alice@example.com"`. Assert the same view emits a `ViewBatch` (not a `FullState`); the `items[0]` payload now has `author_display = "Alice"`, `author_picture = "https://example/alice.jpg"`, `author_nip05_domain = "example.com"`. Assert the `id` field of `items[0]` is unchanged (same event row; the row updated, did not re-create).
3. **In-place refinement on time:** advance the `SimulatedClock` by 5 minutes; trigger the per-tick re-format (per `kernel-substrate.md` §3 `fn on_tick` line 153). Assert `items[0].created_at_display` updates from "just now" to "5 min ago" without the row being torn down.
4. **In-place refinement on reaction arrival:** insert a kind:7 reaction targeting the kind:1 event. Assert `items[0].reaction_summary` updates from 0 to 1 in the next `ViewBatch`; no row re-creation; `id` stable.
5. **Freshness hint, not gate:** insert an older cached kind:0 for Alice (created two days ago), then a fresher one (created an hour ago). Assert the payload reflects the *fresher* one (per C1 supersession), and that the optional `author_display_freshness` field (if exposed by the view module) reads `Recent`, not `DaysOld`. Assert there is no API surface where the test can ask "is this stale?" and have the framework withhold the value pending re-fetch.

**Milestone owner:** **[PARTIAL]** for placeholder shape — the M1 timeline slice ships `author_display` as a non-`Option` `String` with shortened-npub fallback (verified in `crates/nmp-core` timeline tests). However, `author_picture_url` is currently `Option<String>` in the M1 payload: when no kind:0 is present the field is `None` rather than a deterministic identicon URI, which violates D1's "every display field carries a value" guarantee. The D1-compliant fix is to make `author_picture_url: String` non-optional and populate it with a deterministic identicon URI derived from the pubkey hash when no picture URL is known — making the placeholder computable without any network call and without an `Option` that tempts the platform to branch on `None`. This change lands in M2 alongside the view-module surface refactor. **[PENDING M2/M3]** for the full in-place refinement guarantees: sub-paths 2 and 4 require the kernel's projection cache (`kernel-substrate.md` §3 line 148 `on_projection_changed`) which graduates in M2; sub-path 3 requires the per-tick re-format hook (`fn on_tick`, M2's `ViewModule` trait work).

Test checked in **not** ignored for sub-paths 1 and 5; sub-paths 2/3/4 use a `#[cfg(feature = "m2_projection_cache")]` gate so they activate as M2 lands without a re-edit. The framework-magic delta at M2 exit removes the gate.

## Why this is one bullet, not several

The five sub-paths are five facets of one observable: *the payload field is always renderable, and updates appear without the row being destroyed.* Splitting them would suggest the platform might see a `ViewBatch` for kind:0 arrival but `FullState` for reaction arrival, or that some fields are non-`Option` and others are. The contract is uniform; the test enumerates the field categories that exercise it.

## Doctrine alignment

C13 is the canonical instance of cardinal doctrine **D1**. The doctrine clause's wording — *"There is no `if has_profile { render } else { spinner }` pattern available in the API"* — is testable through the payload shape itself, which is what sub-path 1's "no `loading` field" assertion checks. The framework cannot guarantee the app does not implement its own spinner over the payload, but it can guarantee the API does not give the app a way to ask the question that would justify one.

C13 also intersects D4 (single writer per fact; caches derive). The "fact" is the projection (Alice's display name); the "caches" are every timeline cell, profile chip, thread author marker rendering that name. The in-place refinement is the derivation.

C13 is delivered in practice by **D8** (reactivity contract: composite reverse index · ≤60Hz/view · working-set bounded). The projection cache's composite reverse index (`docs/design/reactivity/loop-and-reverse-index.md` ADR-0001) is what makes `on_projection_changed` fire only for the views that mention Alice, not for every view in the store.

## Cross-references

- `docs/design/view-catalog/profile-timeline-thread-reactions.md` — the concrete view-module catalog with each view's payload shape.
- `docs/design/reactivity/view-deltas-and-projections.md` — the projection cache that backs the cross-view refinement.
- `docs/design/kernel-substrate.md` §3 — `ViewModule` trait including `on_projection_changed`, `on_tick`.
- `docs/product-spec/subsystems.md` §7.6 — the placeholder table and the `TimelineItem` example.

## What this chapter does not cover

- **Per-view payload byte budgets.** `subsystems.md` §7.16 owns those. The contract guarantees the rendering shape; the budget is a perf concern.
- **Cross-platform pixel-parity.** `subsystems.md` §3.5 owns the cross-platform consistency tests. C13 asserts the payload values are correct; the platforms agree to render the same payload identically.
- **Long-form content parsing nodes.** `subsystems.md` §7.6 "Post-v1 content rendering contract" — explicitly post-v1; the v1 contract is summary-shaped payloads.
- **DM body decryption inside the view payload.** The decrypted body fits the same C13 pattern (placeholder = empty string; in-place refinement when decrypt succeeds), but the decryption path itself is M9 territory and is not v1.
