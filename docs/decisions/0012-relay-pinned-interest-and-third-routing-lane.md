# ADR-0012 — `RelayPinnedInterest` and the Third Routing Lane

> **Status:** Accepted (landed alongside T42 / commit `5178cfc`).
> **Date:** 2026-05-18.
> **Companion:** `docs/design/nip29/routing.md` §3; `docs/plan/m11.5-highlighter.md` §Exit Gate.

## Context

NIP-29 group events are routed by their host relay, not by the author's NIP-65
mailboxes. A subscription with `#h: [group_id]` only makes sense against the
single relay that holds the group; publishing an event with an `h` tag only
makes sense to that same relay. This is the *routing inversion* that the M2
subscription compiler's two-lane model (Outbox / Inbox) cannot express on its
own.

Three viable shapes were considered (per `nip29-crate.md` §8 question 1):

- **(a)** `nmp-nip29` returns a typed `RelayPinnedInterest` that the compiler's
  outer dispatch handles.
- **(b)** The compiler grows a generic "honor pin-hints from any crate"
  mechanism and `nmp-nip29` participates via a trait.
- **(c)** `nmp-nip29` constructs its own raw REQs and publish paths,
  bypassing the M2 compiler entirely.

Shape (c) fails three doctrines:
- D1 (best-effort rendering + diagnostics) — the diagnostics lane would not
  observe the wire activity.
- D8 (subscription dedup / merge / auto-close) — parallel REQs to the same
  group from independent surfaces would not collapse.
- The framework-magic contract — the user should not need to reason about
  per-tab REQ dedup.

A routing carrier must therefore live inside the compiler.

## Decision

Ship **(b) generic**: `nmp_core::planner::InterestShape::pin_to: Option<RelayUrl>`
is a first-class field. The compiler's filter-merge lattice gains **Rule 9**
(`pin_to` equality; `None` does NOT absorb `Some`); the partition gains
**Case E** which short-circuits the four-lane dispatch when `pin_to.is_some()`.

`nmp-nip29` is the first consumer; future relay-pinned NIPs participate
through the same generic surface with **zero compiler changes**.

### Concretely (post-T42 master state)

- `InterestShape::pin_to: Option<RelayUrl>` — purely out-of-band; never sent
  on the wire as part of the filter.
- `planner::lattice::rules::rule9_pin_to` — equality check; refuses merge
  across different hosts, refuses to absorb `None` into `Some(_)` (mixing
  pinned + unpinned would either leak pinned content to other relays or
  narrow the unpinned scope).
- `planner::compiler::partition::case_e_pin_to::route` — short-circuits
  A/B/C/D when `pin_to.is_some()`. Authors / addresses / `#p` on the same
  interest are retained on the wire filter but ignored for routing.
- Routing source for pinned entries is `RoutingSource::UserConfigured(Debug)`
  in the diagnostics lane (the pin is an explicit operator-injected override
  that bypasses the four-lane discipline by design).
- Plan-id hashing auto-includes `pin_to` because `InterestShape` serializes
  it; no separate hash input is required.

### Publish-side mirror (`nmp_nip29::action::PublishPlan`)

Per `routing.md` §5: every NIP-29 action emits a `PublishPlan` with a typed
`pin_to: Some(RelayPin { relay_url, source_group })`. The publish planner
consults the typed pin for routing and does NOT inspect tags to *derive*
routing. The single tag-inspection it performs is a structural refusal: any
event carrying `["h", _]` with `pin_to: None` is rejected at construction
time with `PublishPlanError::MissingHostPinForGroupEvent`.

The refusal is the privacy-leak prevention guard (publishing a group chat to
the author's write relays would leak it to everyone reading the author's
mailbox).

## Consequences

**Positive:**

- Other relay-pinned NIPs (livestream NIPs, future closed-relay protocols)
  use the same lane without further compiler changes.
- Pinned subscriptions never trigger NIP-65 lookups, indexer fallback, or
  `request_probe` — keeping the four-lane discipline intact in the
  diagnostics surface.
- Lattice + partition tests are local to `nmp-core`; protocol-crate behaviors
  are local to `nmp-nip29` — the boundary stays clean.

**Negative:**

- `InterestShape` now has 9 fields where it had 8; serializers in any
  external consumer must accept the new field (mitigated by `serde` field
  defaults — `pin_to` defaults to `None` on deserialization).
- The plan-id hash now varies on `pin_to`; an interest's plan-id is no
  longer compatible across pinned-vs-unpinned re-issues. This is the desired
  behavior (different routing should produce different plans).

**Neutral:**

- Sub-categorising the pinned entry under `UserConfigured(Debug)` keeps the
  diagnostics surface at four lanes. If a future ADR wants a dedicated
  "host-pinned" sub-category, that's a non-breaking enum extension.

## Tests landed with this ADR

In `nmp_core::planner::lattice::tests` (`crates/nmp-core/src/planner/lattice/mod.rs`):

- `rule9_identical_pin_to_merges`
- `rule9_different_pin_to_refuse`
- `rule9_pinned_does_not_absorb_unpinned`
- `rule9_both_none_merges`

In `nmp_nip29::tests` (`crates/nmp-nip29/src/tests.rs`):

- `nip29_group_lifecycle_create_then_ingest_metadata`
- `nip29_lattice_rule9_pin_to_blocks_cross_host_merge` (end-to-end through
  `SubscriptionCompiler::compile`)
- `nip29_moderation_audit_does_not_mutate_canonical_membership`

The full 7-test routing audit (`routing.md` §8) lands in `nmp-testing` once
the M2 publish planner trait surface is fully wired; the M11.5 Step 0
landing ships the lattice + partition halves, which are the load-bearing
kernel-side proof.

## Open questions deferred to follow-up ADRs

- **Multi-pin interests** (one logical interest fanning to N hosts atomically) —
  currently expressed as N separate `LogicalInterest`s each with their own
  `pin_to`. ADR follow-up if a load-bearing case for atomic multi-pin emerges
  (M11.5 `JoinedGroupsView` is the obvious candidate; current design fans
  out in the kernel without atomic semantics).
- **Diagnostics sub-category for host-pinned** — the current `Debug`
  sub-category is honest but coarse. Promotion to a dedicated `HostPinned`
  sub-category is a non-breaking change; defer until the diagnostics UI
  needs the distinction.

Related: ADR-0009 (kernel-boundary doctrine), ADR-0013 (NIP-29 metadata-signer
trust model, landed alongside this ADR).
