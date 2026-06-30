# ADR-0060 - NIP-29 Admin Actions and Joined Groups Projection

> **Status:** Implemented. `PutUserAction`, `CreateInviteAction`, and
> `JoinedGroupsProjection` are imported, registered (`app.register_action(...)` /
> `wire_joined_groups`), and exported from
> `crates/nmp-nip29/src/register.rs`.
> **Date:** 2026-06-19.
> **Issue:** #1559.
> **Companions:** `docs/design/nip29-crate.md`,
> `docs/design/nip29/kinds.md`, `docs/design/nip29/routing.md`,
> `docs/design/nip29/moderation.md`, ADR-0013.

> **Amended (2026-06-30, #2509 / #2513).** This ADR's "current surface"
> enumeration below is preserved as a faithful record of the 2026-06-19 shipped
> code, but it is **superseded** on one point: `nmp-nip29` is now a *kind-blind
> transport*. The per-kind named group actions listed below —
> `PostChatMessageAction`, `ReactInGroupAction`, `ShareEventInGroupAction`,
> `RepostInGroupAction` — were **deleted** in #2509. The sole group-event write
> surface is the generic `PublishGroupEventAction`
> (`nmp.nip29.publish_group_event`), which injects only the `h` / `previous` /
> host-pin envelope around a caller-built event of any kind. Foreign kinds are
> authored by their owning NIP (kind:7 = `nmp-nip25`, kind:16 = `nmp-nip18`,
> kind:11/other = app layer) and routed through that one action. The
> `nip29_kind_blind` doctrine-lint rule (#2513) is the durable backstop. This
> ADR's admin/lifecycle decisions (`PutUserAction`, `CreateInviteAction`,
> `JoinedGroupsProjection`) are unaffected.

## Context

NIP-29 admin actions and joined-group status are reusable Nostr group
infrastructure. They belong in `nmp-nip29`; `nmp-core` must stay generic and
must not grow NIP-29 command variants, group nouns, or router branches.

`nmp-nip29` now registers these live action surfaces (the full set in
`crates/nmp-nip29/src/register.rs::register_actions`):

- `action`: `PublishGroupEventAction` (the SOLE kind-agnostic write surface —
  any caller-supplied event, h/previous/host-pin envelope injected),
  `CreatePublicGroupAction` (9007 then 9002), `DiscoverGroupsAction`,
  `JoinGroupAction` (9021 with optional `code`), and — added by this ADR —
  `PutUserAction` (9000) and `CreateInviteAction` (9009). Per-kind event
  construction (kind:7 reactions in `nmp-nip25`, kind:16 reposts in `nmp-nip18`,
  kind:11/other content in the app) lives in the owning NIP/app and is routed
  through `PublishGroupEventAction`; the per-kind `react`/`unreact`/`share`/
  `repost`-in-group actions were removed as kind-blind-transport violations
  (#2513, codifying the #2504/#2505 correction).
- `kinds`: constants and classification for 9000, 9001, 9002, 9005, 9007,
  9008, 9009, 9021, 9022, and relay-signed 39000-39003.
- `interest`: `joined_groups_for_host`, which builds one host-pinned
  39001/39002 interest for one `(user_pubkey, host_relay_url)`.
- `projection`: `GroupChatProjection`, `DiscoveredGroupsProjection`,
  `GroupDefaultsProjection`, `GroupEventsProjection`, and — added by this ADR —
  `JoinedGroupsProjection` (wired via `wire_joined_groups`).
- `register`: wiring for group chat, group discovery, group events, defaults,
  joined groups, and the actions above.

> **Note (historical; superseded by the 2026-06-30 amendment above).** At the
> time of this ADR, `register.rs` also registered `RepostInGroupAction` (group
> reposts). That per-kind action — along with `PostChatMessageAction`,
> `ReactInGroupAction`, and `ShareEventInGroupAction` — was deleted in #2509;
> `nmp-nip29` no longer names any foreign kind. See the amendment note above.

That surface is not enough for #1559. `DiscoveredGroupsProjection` is scoped to
one host relay and surfaces group metadata plus `member_count` and
`admin_count`; it intentionally does not expose the member/admin pubkey sets or
filter by the active account. `JoinedHostsCache` records verified hosts for
fanout, but it is a registry, not the canonical joined-state read model.

## Decision

Add the next NIP-29 admin increment in `nmp-nip29` only:

- `PutUserAction` for kind 9000.
- `CreateInviteAction` for kind 9009.

Do not include 9001 remove-user, 9005 delete-event, 9008 delete-group, 9022
leave, or a standalone 9002 edit-metadata action in this increment. Those stay
separate because they require different UI affordances and rejection semantics;
bundling them would expand #1559 beyond add-member and invite flows.

Add a new crate-owned `JoinedGroupsProjection` because the existing raw
discovery projection cannot derive the screen state without a second writer:
it lacks the active-pubkey membership predicate and the member/admin set facts.
The projection derives joined/admin status only from trusted, latest
relay-signed 39001 and 39002 snapshots, with optional display metadata from
the latest trusted 39000.

The projection identity key is always `(host_relay_url, group_id)` where
`group_id` is the NIP-29 local id from the `d` tag. If two relays publish the
same local id, they are two rows.

## Authority Model

Relay-signed 39001 and 39002 snapshots are the only source of truth for admin
and membership status. User-signed 9000 and 9009 events are requests/audit
events; they never mutate joined/admin state directly.

`PutUserAction` and `CreateInviteAction` pre-validate against the latest local
39001 when it is available:

- If the active signer is present in the latest trusted 39001, publish.
- If the latest trusted 39001 exists and the active signer is absent, reject
  before wire activity.
- If the cache is cold, publish and let the host relay arbitrate.

This preserves ADR-0013 and `moderation.md`: the host relay is the trust
anchor, and clients never trust user-signed admin events over relay-signed
metadata.

## Invite Codes

`CreateInviteAction` must not call hidden randomness. The publish action takes
validated invite codes as explicit input and fans them out across one or more
kind 9009 events when needed.

Code generation, when a UI wants generated codes rather than pasted codes, is
a deterministic helper over explicit entropy bytes. Tests provide fixed bytes;
host integrations may source entropy through an OS/random capability and pass
the bytes back into Rust as data. Rust owns code length, encoding, fanout, and
validation policy. Native code may supply raw entropy only.

## Projection And FFI Shape

Register the projection under `nmp.nip29.joined_groups` with the same pattern
as the existing NIP-29 projections:

- a `ObservedProjectionSink` folds trusted 39000, 39001, and 39002 events;
- a generic JSON snapshot remains available as the permanent fallback;
- a typed FlatBuffers sidecar is emitted under the same key.

The snapshot rows should be flat and protocol-owned:

```rust
pub struct JoinedGroup {
    pub host_relay_url: String,
    pub group_id: String,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub about: Option<String>,
    pub public: bool,
    pub open: bool,
    pub member_count: u32,
    pub admin_count: u32,
    pub is_member: bool,
    pub is_admin: bool,
}
```

Rows are included only when the active pubkey appears in the latest trusted
39001 or 39002 for that `(host_relay_url, group_id)`. The projection may use
`JoinedHostsCache` and `joined_groups_for_host` for fanout, but the cache is
not authority; the relay-signed snapshots decide row inclusion.

## Consequences

Implementation PRs must add round-trip tests for 9000 and 9009 publish plans,
typed projection tests for `nmp.nip29.joined_groups`, and a regression proving
9000/9009 do not mutate joined/admin status without a relay-signed 39001 or
39002 follow-up.

No Highlighter-only policy is allowed. Highlighter and other hosts consume the
typed NIP-29 projection and actions through existing registration seams.

No `nmp-core` NIP-29 noun is allowed. If a missing generic seam is discovered,
it must be added as a protocol-neutral substrate capability and proven by a
non-NIP-29 name.

This is no longer deferred because `nmp-nip29` now has the prerequisite generic
host-pinned routing, action registration, typed sidecar, and single-host
metadata projections. #1559 only needs the accepted boundary and read-model
shape before implementation.
