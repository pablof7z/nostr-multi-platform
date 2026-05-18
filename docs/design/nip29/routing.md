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

### 4.3 Strategy C — per-host-relay registry, multi-sourced ✅

The framework maintains a `JoinedHostsCache` of `(pubkey, host_relay_url, group_id)` rows. The `JoinedGroups` view's dependency is the cross-product `(current_pubkey, host_relay_url)` for every host_relay in the cache, producing one `RelayPinnedInterest` per host relay for the 39001/39002 subscriptions.

The cache is **populated from three trusted sources** (each gives a verified host_relay_url), plus a **bootstrap discovery channel** that produces *candidates* that must be verified before they enter the cache:

1. **Own writes (trusted)** — every `h`-tagged event the user publishes (any kind: 9, 11, 9021, 9022, …) records `(self_pubkey, host_relay_url, group_id)` where `host_relay_url` is the relay the publish was routed to. The publish was routed by the planner's host-relay-pin rule, so the cache entry is correct by construction.
2. **Invite-link redemption (trusted)** — an invite URI carries the host relay in NIP-29's `<host>'<group-id>` format, so the redeem action knows the host before any cache exists; the redeem itself becomes source (1) once the kind:9021 fires.
3. **Explicit user import (trusted)** — pasting a NIP-29 group URI (e.g. from a friend) into the app's "join a community" surface records the host_relay before any wire activity.
4. **Bootstrap candidate discovery (untrusted; needs signer-identity + membership match)** — at session open, issue an indexer-style probe with filter `kinds: [39001, 39002], #p: [self_pubkey]` against the active relay pool. Each hit gives a *group_id candidate* (from the event's `d` tag) but **does not** identify the host relay (the relay that forwarded the event may be an indexer/cache, not the host; and per NIP-29 the same `local_id` may exist on multiple hosts as distinct groups). For each candidate group_id, the framework then re-issues *targeted* per-relay queries. A candidate relay `R` is accepted as the verified host of *the group the user is actually a member of* **iff all four hold**:
   - `R`'s NIP-11 document declares NIP-29 support (29 in `supported_nips`)
   - `R`'s NIP-11 document declares a `pubkey` field
   - The 39000 returned by `R` for the candidate group_id is signed by *that exact NIP-11 pubkey*
   - The 39001 *or* 39002 returned by `R` for the candidate group_id contains the user's pubkey, signed by the same NIP-11 pubkey

   If any of the four fails, `R` is rejected as a host candidate for this user-membership claim (it may still be a fine indexer for discovering further candidates; or it may host a *different* group that happens to share the local_id). The cache entry only materialises after this four-way match.

Without the membership check in (4), two hosts that reuse the same `local_id` would both pass the signer-identity check, and the cache would record whichever responded first — likely the wrong group. With the membership check, only the host whose membership snapshot actually contains the user can win, which is exactly the group the user is a member of.

Without the signer-identity match in (4), a general indexer (e.g. `purplepag.es`) that happens to forward a 39002 — or even an indexer that *also* forwards the host's 39000 verbatim — would be poisonously cached as the host, breaking subsequent host-pinned writes. The signer-identity match is what distinguishes "this relay produces 39000s for this group" from "this relay merely caches them".

Relays without a NIP-11 `pubkey` field cannot be auto-verified as hosts even if they are the actual host; their groups need explicit user import via source (3) or invite-link redemption via source (2). This is a deliberate trade-off: cheap auto-discovery for well-behaved relays, manual flow for the rest.

The verification step is bounded **per (candidate_relay, candidate_group_id) pair**, not per group_id alone — same `local_id` may exist on multiple hosts as distinct groups, so each candidate relay must be probed independently for each candidate group. Concretely: for each `group_id` in the indexer-style hit set, the framework determines the *candidate relay set* as `{R : R is in the user's active relay pool AND R returned at least one 39001/39002 hit during the bootstrap probe for this group_id}` (other relays in the pool are excluded — they didn't claim to know this group at all). Each `(R, group_id)` pair gets one NIP-11 fetch + one 39000 query. The total bound is `O(|hit-relays| × |unique group_ids|)`, dominated in practice by the number of distinct groups (NIP-11 fetches dedupe per-relay across all groups). Results persist (M3 LMDB) per `(R, group_id)` so re-runs are O(0) for already-verified pairs.

Source (4) is the channel that prevents the silent-miss failure mode for users who become members via 9000 add or after device-restore. The probe in source (4) runs once at session open + on every `kind:0/3/10002` update for self, and dedups against the cache.

The cache is persisted (M3 LMDB), shared across app instances of the same account.

## 5. Publish-planner integration: the `h`-tag override

The publish planner (M2 §7) today computes recipients per event as:

- author write-relays from NIP-65
- additionally, any `p`-tagged recipient's read-relays (inbox routing)
- additionally, any user-configured override

M11.5 adds a fourth, **higher-priority** rule:

> If the event being published carries any `["h", <group_id>]` tag, the host relay for `group_id` is the **sole** destination. Author-write relays and `p`-recipient inbox relays are explicitly **not** used. The user-configured override does **not** apply.

The "h-tag override is exclusive" is non-negotiable: publishing a group chat message to the author's write relays would leak it to everyone reading the author's mailbox, defeating the group-scoped privacy guarantee. The publish planner must enforce this with a structural rule, not a per-action opt-in.

How the planner resolves the host relay from the `h` tag value:

- The `PublishPlan` type (M2 §7) gains an `Option<RelayPin>` field carried alongside the signed event. The pin is set by the action that constructs the plan, *not* derived from the event's tags at plan time.
- For NIP-29-native publishes invoked via an `nmp-nip29::ActionModule`, the action's input includes the full `GroupId { host_relay_url, local_id }`. The action sets `pin_to: Some(group_id.host_relay_url)` on the resulting `PublishPlan` before handing it to the planner. The publisher trusts the action; the planner sees only the typed pin.
- For first-time publishes (CreateGroup, JoinRequest from an invite URI, explicit group import) the `GroupId` exists on the action input *before* any cache entry — the action carries `host_relay_url` directly into the pin without consulting `JoinedHostsCache`. The cache materialises *after* the publish succeeds (via the source-1 path in §4.3), so there's no chicken-and-egg.
- For cross-protocol "publish in protocol X, then host-pin share into a group" flows (e.g. publish-and-share-highlight per `feature-inventory.md` §2.1), the *composing action lives in `highlighter-core`*, not in the X protocol crate. `nmp-nip84::PublishHighlight` stays group-unaware (it constructs a `PublishPlan` with `pin_to: None`, routing per author write relays). The composing action `highlighter-core::PublishHighlightAndShareToGroup` invokes `nmp-nip84::PublishHighlight` for the kind:9802 leg, awaits its `ActionId`, then invokes `nmp-nip29::ShareEventIntoGroup` (which takes a typed `GroupId` and sets `pin_to: Some(host)`) for the kind:16 host-pinned leg. **No protocol crate ever imports another protocol crate's `GroupId` or any other NIP-specific type; sequencing happens at the app layer.**

This means **no string-typed `h` tags pass through the planner without a `RelayPin` carrier** — the carrier is on the `PublishPlan` itself, set by the action that knows the pin. The planner consults the typed pin field for routing; **it does NOT inspect event tags to *derive* routing**. However — to prevent the privacy leak that would otherwise occur if a (possibly future, possibly third-party) action constructs an h-tagged event without setting `pin_to` — the planner DOES perform a single **defensive structural check** before dispatch: *if the event being published carries any `["h", _]` tag AND `pin_to` is `None`, the publish is rejected at construction time with a typed `MissingHostPinForGroupEvent` error*. The action must either set `pin_to` (correct) or strip the `h` tag (also correct; the event then routes normally). The test `nip29_publish_refuses_unpinned_h_tag_event` asserts this. This is the *only* tag-inspection the planner performs for NIP-29 — and it's a refusal, not a routing decision, so the "planner is crate-agnostic" property holds.

## 6. The "publish-and-share" dual-route problem (the load-bearing test case)

The Highlighter `publish_and_share` (`highlights.rs:22-83`) is the cleanest example of the dual-routing the framework must handle:

1. Publish a kind:9802 highlight to the user's NIP-65 write relays.
2. Publish a kind:16 generic repost with `["h", target_group_id]` to the target group's host relay.

Today Highlighter does this with two raw `client.send_event(&e)` calls in sequence. In NMP, the `ActionModule` definition has a *single* `dispatch()` entry point. M11.5's design is:

- `nmp-nip84::PublishHighlight` is the simple kind:9802 publish, routes per author's write relays. **Group-unaware** — it imports nothing from `nmp-nip29` and knows no `GroupId` type.
- `nmp-nip29::ShareEventIntoGroup { event_ref, group_id: GroupId }` is the kind:16 host-pinned share. **Highlight-unaware** — it imports nothing from `nmp-nip84` and knows no `Highlight` type.
- `highlighter-core::PublishHighlightAndShareToGroup { draft, target_group: GroupId }` is the composing action that lives at the *app* layer. It invokes `nmp-nip84::PublishHighlight` first, awaits the resulting `ActionId`, then invokes `nmp-nip29::ShareEventIntoGroup { event_ref: <first_action_event_id>, group_id: <target_group> }`. The kernel's ActionLedger (planned M7) supports sequential dependencies between actions; this is a textbook use case.

This composition is **how cross-protocol surfaces stay clean in NMP**: each protocol crate owns its own write path with its own routing rule, neither importing the other; the cross-protocol sequencing lives in the app's own extension crate. **Protocol crates never import each other.**

## 7. Auth: the host relay is the only relay that needs NIP-42 for this crate

Most NIP-29 host relays require NIP-42 authentication for *any* operation, not just reads. M5's NIP-42 surface must therefore land before M11.5 — which it does in the milestone ladder (M5 is in M0–M10).

`nmp-nip29` declares all its `RelayPinnedInterest`s + publishes as **auth-mandatory** (a new annotation on `LogicalInterest` and on `PublishPlan`). The auth-paused relay state (`docs/design/subscription-compilation.md` §10 open question 6) applies: if the host relay's auth state is `ChallengeReceived`, all `nmp-nip29` activity for that host is paused until auth completes, then resumes.

This is also why `nmp-nip29` cannot run against a relay pool with anonymous-only members for its host relays — the framework will refuse to plan a NIP-29 action against an un-authenticatable relay.

## 8. Tests this contract requires (M11.5 exit gate)

These tests live in `nmp-testing/tests/` and run as part of the M11.5 exit gate audit:

1. `nip29_filter_routes_only_to_host_relay` — given a `RelayPinnedInterest` for a kind:9 `#h` query and a pool of 5 relays where only one is the host, the compiler produces a plan with exactly one wire-frame targeting only the host.
2. `nip29_publish_routes_only_to_host_relay` — given a kind:9 chat publish with an `h` tag and a pool where the author has 3 NIP-65 write relays plus the host relay, the publish planner produces a plan targeting only the host.
3. `nip29_publish_refuses_unpinned_h_tag_event` — given a publish whose event carries any `["h", _]` tag and whose `PublishPlan.pin_to` is `None`, the planner refuses with typed `MissingHostPinForGroupEvent` at construction time. This is the privacy-leak prevention: the only structural inspection the planner does on event tags, and it's a refusal rather than a routing derivation.
4. `nip29_pin_to_blocks_filter_merge` — two `LogicalInterest`s with identical filter shapes but different `pin_to` values do not merge into one wire-frame.
5. `nip29_joined_groups_fans_out_across_hosts` — given a cache containing two host relays, the `JoinedGroups` view's dependency expansion produces exactly two `RelayPinnedInterest`s, one per host.
6. `nip29_share_into_group_dual_routes_correctly` — the composed action `PublishHighlight` + `ShareEventIntoGroup` produces two wire-frame writes, one to author write-relays (highlight), one to host relay (share), in that order.
7. `nip29_unauth_host_relay_pauses_module_activity` — a host relay in `ChallengeReceived` causes all module activity for that host to pause, surfacing in the diagnostics lane as `LogicalInterestStatus::AuthPaused`.

Passing these seven tests is the M11.5 exit-gate proof that the routing contract holds. The full M11.5 milestone exit gate is in `docs/plan/m11.5-highlighter.md`.
