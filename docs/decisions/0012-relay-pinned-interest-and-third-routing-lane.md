# ADR-0012 — `relay_pin` and the Third Routing Lane

> **Status:** Accepted (landed alongside T42 / commit `5178cfc`; field renamed
> from `pin_to` to `relay_pin` and abstraction generalised away from NIP-29
> nouns under T55 / D0 cleanup).
> **Date:** 2026-05-18.
> **Companion:** `docs/design/nip29/routing.md` §3 (the first consumer's
> design doc); `docs/plan/m11.5-highlighter.md` §Exit Gate.

## Context

Some Nostr protocols require subscriptions and publishes to be addressed to a
specific host relay regardless of the author's NIP-65 mailboxes. A
subscription that targets a single host (and is meaningless against others)
cannot be expressed by the M2 subscription compiler's two-lane model
(Outbox / Inbox) alone — the routing is *inverted* relative to NIP-65: it
follows the destination relay, not the author.

NIP-29 relay-based groups are the canonical example (group events are bound
to the group's host relay; cross-host routing would be incorrect), but the
same shape recurs for future relay-pinned NIPs (some livestream/NIP-53-style
use cases, closed-relay community protocols, etc.).

Three viable shapes were considered:

- **(a)** A protocol crate returns a typed `RelayPinnedInterest` and the
  compiler's outer dispatch handles it by protocol.
- **(b)** The compiler exposes a generic "honor a relay-pin hint set by any
  consumer" mechanism via a first-class field on `InterestShape`; protocol
  crates participate by populating that field.
- **(c)** The protocol crate constructs its own raw REQs and publish paths,
  bypassing the M2 compiler entirely.

Shape (c) fails three doctrines:
- D1 (best-effort rendering + diagnostics) — the diagnostics lane would not
  observe the wire activity.
- D8 (subscription dedup / merge / auto-close) — parallel REQs to the same
  host from independent surfaces would not collapse.
- The framework-magic contract — the user should not need to reason about
  per-tab REQ dedup.

Shape (a) couples the kernel to per-protocol nouns, violating D0 (the kernel
never grows app/protocol nouns).

A routing carrier must therefore live inside the compiler **and** stay
protocol-agnostic.

## Decision

Ship **(b) generic**: `nmp_core::planner::InterestShape::relay_pin:
Option<RelayUrl>` is a first-class field. The compiler's filter-merge lattice
gains **Rule 9** (`relay_pin` equality; `None` does NOT absorb `Some`); the
partition gains **Case E** which short-circuits the four-lane dispatch when
`relay_pin.is_some()`.

`nmp-nip29` is the first consumer; future relay-pinned NIPs participate
through the same generic surface with **zero compiler changes**.

### Concretely

- `InterestShape::relay_pin: Option<RelayUrl>` — purely out-of-band; never
  sent on the wire as part of the filter.
- `planner::lattice::rules::rule9_relay_pin` — equality check; refuses merge
  across different hosts, refuses to absorb `None` into `Some(_)` (mixing
  pinned + unpinned would either leak pinned content to other relays or
  narrow the unpinned scope). When two shapes share a host, the rest of the
  lattice coalesces them — chiefly Rule 2's tag-value union, which collapses
  many per-room subscriptions (each carrying its own per-room `h` tag)
  into a single per-host REQ. That coalesce pattern is what the third
  routing lane is named after ("h-tag coalesce") even though Rule 9 itself
  is generic and tag-agnostic.
- `planner::compiler::partition::case_e_relay_pinned::route` —
  short-circuits Cases A/B/C/D when `relay_pin.is_some()`. Authors /
  addresses / `#p` on the same interest are retained on the wire filter
  but ignored for routing.
- Routing source for pinned entries is `RoutingSource::UserConfigured(Debug)`
  in the diagnostics lane (the pin is an explicit consumer-injected override
  that bypasses the four-lane discipline by design).
- Plan-id hashing auto-includes `relay_pin` because `InterestShape`
  serialises it; no separate hash input is required.
- The kernel grows zero protocol nouns. The integration test
  `crates/nmp-nip29/tests/lifecycle.rs` proves the protocol crate is a pure
  consumer — `nmp-nip29::interest::host_pinned_interest` simply populates
  the generic `relay_pin` field, and the planner emits the same per-relay
  plan as a hand-built generic interest does.

### Publish-side mirror (`nmp_nip29::action::PublishPlan`)

Per `routing.md` §5, the publish side has its own typed pin carrier on the
protocol crate: every NIP-29 action emits a `PublishPlan` whose own
`pin_to: Option<RelayPin>` field is set. The publish planner consults the
typed pin for routing and does NOT inspect tags to *derive* routing. The
single tag-inspection it performs is a structural refusal: any event
carrying `["h", _]` with the publish-plan pin unset is rejected at
construction time with `PublishPlanError::MissingHostPinForGroupEvent`.

The refusal is the privacy-leak prevention guard (publishing a group chat to
the author's write relays would leak it to everyone reading the author's
mailbox).

This `PublishPlan` field lives in the **protocol crate**, not in `nmp-core`
— the kernel only knows the subscription-side `relay_pin`.

## Consequences

**Positive:**

- Other relay-pinned NIPs (livestream NIPs, future closed-relay protocols)
  use the same lane without further compiler changes.
- Pinned subscriptions never trigger NIP-65 lookups, indexer fallback, or
  `request_probe` — keeping the four-lane discipline intact in the
  diagnostics surface.
- Lattice + partition tests are local to `nmp-core`; protocol-crate
  behaviours are local to `nmp-nip29` — the boundary stays clean (D0
  upheld: `nmp-core` has zero NIP-29 / group nouns).

**Negative:**

- `InterestShape` now has 9 fields where it had 8; serializers in any
  external consumer must accept the new field (mitigated by `Option<T>` serde behaviour
  — missing `relay_pin` deserialises to `None`).
- The plan-id hash now varies on `relay_pin`; an interest's plan-id is no
  longer compatible across pinned-vs-unpinned re-issues. This is the desired
  behaviour (different routing should produce different plans).

**Neutral:**

- Sub-categorising the pinned entry under `UserConfigured(Debug)` keeps the
  diagnostics surface at four lanes. If a future ADR wants a dedicated
  "host-pinned" sub-category, that's a non-breaking enum extension.

## Tests landed with this ADR

In `nmp_core::planner::lattice::tests` (`crates/nmp-core/src/planner/lattice/mod.rs`):

- `rule9_identical_relay_pin_coalesces_h_tags`
- `rule9_different_relay_pin_refuses`
- `rule9_pinned_does_not_absorb_unpinned`
- `rule9_both_none_merges`

In `nmp_nip29::tests` (`crates/nmp-nip29/src/tests.rs`):

- `nip29_group_lifecycle_create_then_ingest_metadata`
- `nip29_lattice_rule9_relay_pin_blocks_cross_host_merge` (end-to-end
  through `SubscriptionCompiler::compile`)
- `nip29_moderation_audit_does_not_mutate_canonical_membership`

External integration test at `crates/nmp-nip29/tests/lifecycle.rs` proves
the protocol crate is a pure consumer of the generic kernel API:

- `generic_relay_pinned_interest_routes_to_host_only`
- `nip29_protocol_crate_consumes_generic_relay_pin_api`
- `different_relay_pins_emit_distinct_per_relay_plans`
- `same_host_pinned_interests_coalesce_h_tag_values`
- `pinned_interest_with_authors_skips_outbox_lookup`

The full 7-test routing audit (`routing.md` §8) lands in `nmp-testing` once
the M2 publish planner trait surface is fully wired; the M11.5 Step 0
landing ships the lattice + partition halves, which are the load-bearing
kernel-side proof.

## Open questions deferred to follow-up ADRs

- **Multi-pin interests** (one logical interest fanning to N hosts atomically) —
  currently expressed as N separate `LogicalInterest`s each with their own
  `relay_pin`. ADR follow-up if a load-bearing case for atomic multi-pin
  emerges (M11.5 `JoinedGroupsView` is the obvious candidate; current
  design fans out in the kernel without atomic semantics).
- **Diagnostics sub-category for host-pinned** — the current `Debug`
  sub-category is honest but coarse. Promotion to a dedicated `HostPinned`
  sub-category is a non-breaking change; defer until the diagnostics UI
  needs the distinction.

Related: ADR-0009 (kernel-boundary doctrine), ADR-0013 (NIP-29 metadata-signer
trust model, landed alongside this ADR).
