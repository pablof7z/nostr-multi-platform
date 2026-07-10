# 28a — Build a keyed live read-collection

> Status: **SHIPS**. Audience: builders + agents. Read after
> [28 — Concept-owned active reads](28-action-triggered-subscriptions.md).
> The primitive ships at `crates/nmp-core/src/trellis_reconciler.rs`
> (`KeyedReconciler<K, C>`), `crates/nmp-read-session/src/keyed_collection.rs`
> (`KeyedReadCollection<K, C>`), and
> `crates/nmp-uniffi-support/src/keyed_read_collection.rs` (the two facade
> constructors), proven by `keyed_collection_tests.rs` and this crate's own
> `#[cfg(test)]` modules (#3115/#3116). Design record:
> [ADR-0078](../decisions/0078-keyed-live-read-collection.md).

## The problem: N live resources over a changing key-set

Chapter 28's concept-owned active read (`open_<concept>(target) -> handle`)
covers one demand mounting one output. Some product surfaces need a
CALLER-CONTROLLED set of keys, each mounting its OWN independent live
resource, where the key-set changes as the user browses — a group list
scrolls into view, groups leave view, the user switches accounts. Naively:

- close every row and reopen the whole set on every key-set change (wasteful,
  and briefly blanks rows that were never actually removed), or
- hand-roll a `HashSet` diff per surface that needs this (the exact class of
  duplicated reconciler #3116 audited and consolidated).

`KeyedReadCollection<K, C>` is the reusable fix: feed it the full desired
`BTreeMap<K, C>` on every change, and it opens exactly the newly-desired
keys, closes exactly the no-longer-desired keys, and leaves an unchanged key
alone.

## The driving example: 29er's group-tree

29er's group list needs two independent live facts per group, both keyed by
`group_id`:

- **Collection A — per-group last-message preview.** A raw observed
  projection over each group's kind:9 events (no reducer, no typed output —
  the app reads admitted events directly off the kernel).
- **Collection B — per-group presence.** A full `nmp-read-session`
  read-session per group (its own reducer, its own typed output) that also
  depends on `active_pubkey` — a value that is NOT part of the group-id
  key-set.

These are TWO separate `KeyedReadCollection` instances, one per collection,
not one collection mounting two resources per key — each instance owns a
private Trellis graph, so instance A's keys can never coalesce with instance
B's even though both are keyed by the same `group_id` strings (see
[ADR-0078](../decisions/0078-keyed-live-read-collection.md)'s per-parent
ownership rule).

### Collection A: raw observed projection per group

```rust
use nmp_uniffi_support::keyed_observed_projection_collection;
use nmp_read_session::MemberKey;
use std::sync::Arc;

#[derive(Clone, PartialEq)]
struct GroupFeedDescriptor {
    group_id: String,
    host_relay_url: String,
}

// `group_last_message_projection` is your app-owned helper that returns the
// per-key observed read descriptor — a kind:9 read scoped to the group and
// pinned to its host relay. Its concrete substrate type and fields live in
// builder-guide/05a-substrate-traits.md; the collection constructor takes it
// opaquely and owns its open/close, so you never build or reconcile it by hand.
let last_message_collection = keyed_observed_projection_collection::<String, GroupFeedDescriptor>(
    Arc::clone(&app),
    "twentynineer.group-tree.last-message",
    |group_id| MemberKey::new(group_id.clone()),
    |member_key, descriptor| group_last_message_projection(member_key, descriptor),
);
```

Reconcile it whenever the visible group set changes — e.g. from the
group-tree's own scroll/viewport source, not from a render tick:

```rust
let mut desired = BTreeMap::new();
for group in visible_groups {
    desired.insert(
        group.id.clone(),
        GroupFeedDescriptor { group_id: group.id.clone(), host_relay_url: group.host_relay_url.clone() },
    );
}
last_message_collection.reconcile(desired);
```

A group that scrolls out of view and a group that was never opened both
simply stop appearing in `desired`; `KeyedReadCollection` closes exactly
that key's projection, once, and leaves every still-visible group's
subscription untouched.

### Collection B: a full read-session per group, with an exogenous scalar

Presence depends on `active_pubkey`, which is not part of the `group_id`
key. Embed it in the payload `C` instead of hand-rolling a
force-close+reopen on identity change:

```rust
use nmp_uniffi_support::keyed_read_session_collection;
use nmp_read_session::{ReadSpec, ReadDemand, ReadReplayPolicy, InterestLifecycle};

#[derive(Clone, PartialEq)]
struct PresenceDescriptor {
    group_id: String,
    active_pubkey: String, // exogenous scalar — not part of K
}

let presence_collection = keyed_read_session_collection::<String, PresenceDescriptor>(
    Arc::clone(&app),
    "twentynineer.group-tree.presence",
    |group_id| MemberKey::new(group_id.clone()),
    |member_key, descriptor| ReadSpec {
        projection_key: presence_projection_key(member_key.as_str()),
        demands: vec![ReadDemand {
            filter_json: format!(
                r##"{{"kinds":[10312],"authors":["{}"],"#h":["{}"]}}"##,
                descriptor.active_pubkey, member_key.as_str()
            ),
            consumer_id: format!("group-presence::{}", member_key.as_str()),
            scope: 1,
            relay_pin: None,
            is_indexer_discovery: false,
            lifecycle: InterestLifecycle::Tailing,
            replay_limit: 1,
            replay: ReadReplayPolicy::Structural,
        }],
        observer: presence_sink(),
        output_encoder: presence_output_encoder(),
        dependent_demands: Vec::new(),
        keep_open_without_live_demand: false,
    },
);
```

When the active account switches, re-supply the SAME `group_id` keys with a
`PresenceDescriptor` carrying the NEW `active_pubkey`:

```rust
let mut desired = BTreeMap::new();
for group in visible_groups {
    desired.insert(
        group.id.clone(),
        PresenceDescriptor { group_id: group.id.clone(), active_pubkey: current_pubkey.clone() },
    );
}
presence_collection.reconcile(desired);
```

`C: PartialEq` means Trellis detects that every live key's payload changed
(the `active_pubkey` field diverged) and emits `Replace` for each — the
collection withdraws and remounts exactly the presence sessions, in `Vec`
order, without touching the last-message collection at all. This replaces a
force-close+reopen of the whole group list on every account switch with a
real diff.

## Where to call `reconcile`

Both collections above must be reconciled from the read/actor lane — the
group-tree's own scroll/viewport source callback or an identity-change
observer, never from inside a snapshot-tick or render closure. Calling a
`.sync()`-shaped reconcile from a closure that runs under a registry lock
and itself opens a read session is the exact deadlock class #3078-#3081
fixed the symptom of; this primitive's `open` closure runs with no lock the
collection itself holds, but the CALLER still owns keeping `reconcile`/`close`
off the render/snapshot lane. See
[ADR-0078](../decisions/0078-keyed-live-read-collection.md)'s lane-discipline
section.

## Choosing shape (a) vs. shape (b)

- Need every member to fold into ONE shared output (a timeline, a merged
  set)? Use shape (a) — `nmp_read_session::open_read_demand_set` /
  `reconcile_read_demand_set` (see
  [28 — Concept-owned active reads](28-action-triggered-subscriptions.md)'s
  NIP-29 relay-discovery example).
- Need each key to own its OWN independent output/lifecycle? Use shape (b) —
  `KeyedReadCollection<K, C>`, this chapter.

Both compile to the same `KeyedReconciler<K, C>` core
(`nmp-core/src/trellis_reconciler.rs`); the choice is about the shape of the
OUTPUT, not about how membership is diffed.

## Anti-patterns

1. **Reaching for a fresh `HashSet`/`HashMap` diff without considering
   `KeyedReconciler`/`KeyedReadCollection` first.** #3116 consolidated three
   separate hand-rolled implementations onto this core, and it's usually the
   less work to reuse than to rebuild — but per
   [ADR-0077](../decisions/0077-doctrines-are-guardrails-not-dogma.md),
   hand-rolling the diff is a legitimate choice when it's the better fit for
   the case at hand, not a mistake in itself.
2. **Force-close+reopen the whole collection on an exogenous-scalar change.**
   Embed the scalar in `C` and let `Replace` do the narrow withdraw+remount.
3. **An under-specified `key_fn`.** Omitting a parameter that distinguishes
   two members (a relay pin, a filter shape) either silently coalesces them
   or aborts the whole commit — see ADR-0078's member-identity rule.
4. **Calling `reconcile`/`close` from a snapshot/render-tick closure.** Open
   once at discovery-open time on the read/actor lane; the tick closure only
   reads current outputs.
5. **Sharing one `KeyedReadCollection` instance across two logically
   distinct collections to save a Trellis graph.** Per-parent ownership (a
   private graph per collection) is the v1 default specifically to prevent
   cross-collection coalescing.

## See also

- [28 — Concept-owned active reads](28-action-triggered-subscriptions.md) -
  the single-resource concept-read pattern this chapter extends.
- [ADR-0078](../decisions/0078-keyed-live-read-collection.md) - full design
  record: the member-identity rule, the drain-order executor obligation, and
  the lane-discipline rule.
- [ADR-0075](../decisions/0075-trellis-private-reconciliation-substrate.md) -
  Trellis as private reconciliation substrate; why this chapter never names
  raw Trellis types directly.
- `skills/nmp-app-architecture/references/read-sessions.md` - the
  demand-reconciler-family rule for reviewers/agents.
