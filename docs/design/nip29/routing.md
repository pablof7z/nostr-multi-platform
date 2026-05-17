# `nmp-nip29` — Routing Contract (Host-Relay-Pin)

> Sub-file of [`../nip29-crate.md`](../nip29-crate.md). Covers the routing inversion that NIP-29 forces on the M2 subscription compiler + publish planner.
> **Companion:** `docs/design/subscription-compilation.md` §3 (compiler pipeline), §7 (publish planner). This file extends those by adding a third routing lane.

## 1. The inversion in one paragraph

The M2 outbox planner routes subscriptions by **the author's NIP-65 mailboxes** (filters with `authors`) or **the recipient's inbox** (filters with `#p`). NIP-29 events have neither property as their primary routing key — they have an `h` tag scoping them to a group, and the group only exists on **one specific relay**, the host relay. The host relay is not derived from the author's mailboxes (the author may have no mailboxes set; they may have ten relays that don't include the host relay; none of that matters). The host relay is the truth.

This means: **for any filter containing `#h: [group_id]`, every other routing input is suppressed and the filter goes to the group's host relay only**. Symmetrically, **for any publish of an event carrying an `h` tag, the author's NIP-65 write relays are suppressed and the event goes to the host relay only**. This applies to user-side events (kinds 9, 11, 9021, 9022, etc.) as well as admin-side events (9000–9009).

The metadata events (39000–39003) are an even sharper case: they are signed by the relay's own keypair and only the host relay produces them, so even the concept of "the author's mailboxes" is meaningless — there is no human author.

## 2. Why this cannot be a hack inside `nmp-nip29`

A naive implementation would have `nmp-nip29`'s view modules construct their own raw REQs and write paths, bypassing M2 entirely. That fails three doctrines:

1. **D1 best-effort rendering with placeholders** — the diagnostics lane (ADR-0007) wouldn't see the wire activity because it didn't go through the compiler.
2. **Subscription dedup + merge + auto-close** — M2's wire-frame compiler dedups overlapping interests across modules; a bypass would issue parallel REQs for the same group, wasting relay budget and confusing the actor's mailbox bookkeeping.
3. **The framework-magic contract** — the user shouldn't have to wonder whether their per-group chat REQ got deduped against another tab also viewing that group. The compiler is the single source of truth.

So the host-relay-pin **must live inside the compiler**, surfaced by a typed signal `nmp-nip29` emits when it declares its dependencies.

## 3. The third routing lane: `RelayPinnedInterest`

Today the compiler reads a `LogicalInterest`'s filter and dispatches to one of two lanes (paraphrasing `subscription-compilation/compiler.md` §3 step 2):

| Lane | Filter shape | Resolves via |
|---|---|---|
| A: author-write | `authors: [a, b, c]` | NIP-65 outbox mailboxes for a, b, c (write relays) |
| B: recipient-read | `#p: [p, q]` | NIP-65 inbox mailboxes for p, q (read relays) |

M11.5 adds:

| Lane | Filter shape | Resolves via |
|---|---|---|
| C: relay-pinned | filter carries a `pin_to: RelayUrl` annotation | direct routing to `pin_to`, no NIP-65 lookup |

The `pin_to` annotation is not a regular Nostr filter field — it's an out-of-band hint carried by the `LogicalInterest` type itself, **not** sent on the wire. When a `LogicalInterest` arrives at the compiler with `pin_to: Some(url)`, the compiler skips lanes A + B entirely and produces a one-relay plan targeting `pin_to`. The `#h` value is *also* on the filter (relays expect it), but the pin is what determines routing.

Concretely, `nmp-nip29`'s `ViewModule::dependencies()` constructs interests like:

```rust
LogicalInterest::new()
    .filter(Filter::new()
        .kinds([Kind::Custom(9)])
        .custom_tag('h', [group_id.local_id.clone()]))
    .pin_to(group_id.host_relay_url.clone())   // <-- third-lane signal
```

The compiler's filter-merge lattice (`compiler.md` §3 step 3) extends to include `pin_to` as a merge-key: two interests with identical filters but different `pin_to` cannot merge, because they go to different relays. Interests with `pin_to = None` cannot merge with interests with `pin_to = Some(_)` for the same reason.

## 4. Multi-host aggregation: `JoinedGroups` view

The hard view in `nmp-nip29` is `JoinedGroups`, which has to answer "what communities is the user in across **all** the host relays they touch?" — a single user may be in groups on three or more different host relays.

Three valid strategies:

### 4.1 Strategy A — host-relay registry (rejected)

Have the user manually declare which host relays they care about (similar to NIP-65 but for group hosts). Reject: makes onboarding worse, and there's no NIP for it.

### 4.2 Strategy B — fan out across every connected relay (rejected)

Issue the 39001/39002 subscription on every relay in the user's pool. Reject: most relays don't host the user's groups; this is bandwidth + work waste, and a privacy leak (every connected relay learns the user's pubkey-of-interest).

### 4.3 Strategy C — per-host-relay registry derived from user's writes ✅

The framework discovers the user's host relays by tracking where the user has published events with an `h` tag. Each unique `h`-tagged publish records `(pubkey, host_relay_url, group_id)` in a small `nmp-nip29::JoinedHostsCache` table. The `JoinedGroups` view's dependency is the cross-product `(current_pubkey, host_relay_url)` for every host_relay in the cache, producing one `RelayPinnedInterest` per host relay for the 39001/39002 subscriptions.

This means the first time the user joins a group on `relay29.fiatjaf.com`, their JoinRequest write hits `relay29.fiatjaf.com` (via the pin), the cache records "this user has touched `relay29.fiatjaf.com` for groups", and subsequent `JoinedGroups` view re-renders include `relay29.fiatjaf.com` in the fan-out automatically. **First-time discovery of a host relay is the side-effect of joining; no separate setup step.**

For onboarding from an invite link, the link itself carries the host relay (NIP-29 URI format `<host>'<group-id>`), so the redeem action knows the host before any cache exists.

The cache can be persisted (M3 LMDB), shared across app instances of the same account.

## 5. Publish-planner integration: the `h`-tag override

The publish planner (M2 §7) today computes recipients per event as:

- author write-relays from NIP-65
- additionally, any `p`-tagged recipient's read-relays (inbox routing)
- additionally, any user-configured override

M11.5 adds a fourth, **higher-priority** rule:

> If the event being published carries any `["h", <group_id>]` tag, the host relay for `group_id` is the **sole** destination. Author-write relays and `p`-recipient inbox relays are explicitly **not** used. The user-configured override does **not** apply.

The "h-tag override is exclusive" is non-negotiable: publishing a group chat message to the author's write relays would leak it to everyone reading the author's mailbox, defeating the group-scoped privacy guarantee. The publish planner must enforce this with a structural rule, not a per-action opt-in.

How the planner resolves the host relay from the `h` tag value:

- For NIP-29-native publishes invoked via an `nmp-nip29::ActionModule`, the action's input includes the full `GroupId { host_relay_url, local_id }`. The publisher trusts the action.
- For cross-crate publishes that happen to carry an `h` tag (e.g. `nmp-nip84::PublishHighlightAndShare` from `feature-inventory.md` §2.1), the cross-crate action must declare a typed `share_to: GroupId` parameter, never just an `h_tag_value: String`. This is enforced by code review + by the fact that `nmp-nip84` will take a typed `nmp-nip29::GroupId` import in its `ShareTo` action input. **No string-typed `h` tags pass through the planner without a `GroupId` carrier.**

## 6. The "publish-and-share" dual-route problem (the load-bearing test case)

The Highlighter `publish_and_share` (`highlights.rs:22-83`) is the cleanest example of the dual-routing the framework must handle:

1. Publish a kind:9802 highlight to the user's NIP-65 write relays.
2. Publish a kind:16 generic repost with `["h", target_group_id]` to the target group's host relay.

Today Highlighter does this with two raw `client.send_event(&e)` calls in sequence. In NMP, the `ActionModule` definition has a *single* `dispatch()` entry point. M11.5's recommended design is:

- `nmp-nip84::PublishHighlight` is the simple kind:9802 publish, routes per author's write relays.
- `nmp-nip29::ShareEventIntoGroup { event_ref, group_id, relay_hint }` is the kind:16 share, routes pinned to the host relay.
- A higher-level UI action ("share this new highlight into a community") composes the two as a sequential plan: the second action waits on the first action's `ActionId` to confirm before firing. The kernel's ActionLedger (planned M7) already supports sequential dependencies between actions; this is a use case for that.

This composition is **how cross-protocol surfaces stay clean in NMP**: each protocol crate owns its own write path with its own routing rule; cross-protocol flows are sequenced at the action layer, not by special-casing inside any single crate.

## 7. Auth: the host relay is the only relay that needs NIP-42 for this crate

Most NIP-29 host relays require NIP-42 authentication for *any* operation, not just reads. M5's NIP-42 surface must therefore land before M11.5 — which it does in the milestone ladder (M5 is in M0–M10).

`nmp-nip29` declares all its `RelayPinnedInterest`s + publishes as **auth-mandatory** (a new annotation on `LogicalInterest` and on `PublishPlan`). The auth-paused relay state (`docs/design/subscription-compilation.md` §10 open question 6) applies: if the host relay's auth state is `ChallengeReceived`, all `nmp-nip29` activity for that host is paused until auth completes, then resumes.

This is also why `nmp-nip29` cannot run against a relay pool with anonymous-only members for its host relays — the framework will refuse to plan a NIP-29 action against an un-authenticatable relay.

## 8. Tests this contract requires (M11.5 exit gate)

These tests live in `nmp-testing/tests/` and run as part of the M11.5 exit gate audit:

1. `nip29_filter_routes_only_to_host_relay` — given a `RelayPinnedInterest` for a kind:9 `#h` query and a pool of 5 relays where only one is the host, the compiler produces a plan with exactly one wire-frame targeting only the host.
2. `nip29_publish_routes_only_to_host_relay` — given a kind:9 chat publish with an `h` tag and a pool where the author has 3 NIP-65 write relays plus the host relay, the publish planner produces a plan targeting only the host.
3. `nip29_publish_refuses_publish_with_unknown_host` — given an `h` tag whose group is unknown to the planner (no cache entry, no explicit `GroupId` carrier), the publish fails with a typed error rather than silently fanning out.
4. `nip29_pin_to_blocks_filter_merge` — two `LogicalInterest`s with identical filter shapes but different `pin_to` values do not merge into one wire-frame.
5. `nip29_joined_groups_fans_out_across_hosts` — given a cache containing two host relays, the `JoinedGroups` view's dependency expansion produces exactly two `RelayPinnedInterest`s, one per host.
6. `nip29_share_into_group_dual_routes_correctly` — the composed action `PublishHighlight` + `ShareEventIntoGroup` produces two wire-frame writes, one to author write-relays (highlight), one to host relay (share), in that order.
7. `nip29_unauth_host_relay_pauses_module_activity` — a host relay in `ChallengeReceived` causes all module activity for that host to pause, surfacing in the diagnostics lane as `LogicalInterestStatus::AuthPaused`.

Passing these seven tests is the M11.5 exit-gate proof that the routing contract holds. The full M11.5 milestone exit gate is in `docs/plan/m11.5-highlighter.md`.
