# 10 — Outbox routing (NIP-65)

> Status: **SHIPS** (planner + lifecycle). Audience: builders + agents.
> Doctrine: **D3** — outbox routing is automatic; manual relay selection is
> the opt-out, not the default.

Outbox routing is the policy that decides *which relays* a read or write
touches, from the authors/recipients alone — never from an app-supplied
relay list. NMP makes the wrong thing structurally impossible: there is no
relay field on a view spec or on `SendNote`. The only API surfaces that name
a relay are the audited publish override, the diagnostic accessors
(read-only), and the user-configured-relays settings surface
(`docs/design/framework-magic/outbox.md`).

## Deliverable: routing table (verbatim from spec §7.3)

From `docs/product-spec/subsystems.md:78-86`:

| Operation | Relay set |
|---|---|
| Subscription with `authors` filter | Per author: union of NIP-65 write relays ∪ app relays. Authors with neither land in `CompiledPlan.unroutable_authors` so the kernel can surface a "no relay to ask" diagnostic. Indexer is **never** consulted for content. |
| Subscription with `p` tag filter or notifications | Union of each tagged pubkey's inbox relays. |
| Subscription with neither | Active account's NIP-65 read relays ∪ app relays. |
| Publish of any signed event | Author's NIP-65 write relays ∪ app relays. Fails closed if both are empty. |
| Publish with `p` tags (DMs, mentions, reactions) | (Author's write relays ∪ app relays) plus each tagged pubkey's NIP-65 inbox relays. |
| DM (NIP-17 gift-wrapped) | **Only** resolved recipient inbox relays. Never the author's write relays. Never the active session's "default" relays. Missing recipient inbox relays fail closed. |
| Discovery (kind-10002 fetch for unknown pubkeys) | Configurable indexer relay set (default: a curated list of high-coverage relays). |

The DM row is fail-closed: a gift-wrap with no resolved recipient inbox is
**not emitted on any relay** (`PublishPlanError::PrivateRecipientUnroutable`,
`docs/design/subscription-compilation/outbox.md` §7.3 step 3b). There is no
"send everywhere" fallback for private events — that is the doctrine-10
footgun the structure removes.

## How the planner implements it

Read direction is `SubscriptionCompiler` Stage 1
(`crates/nmp-core/src/planner/compiler/mod.rs:60-189`); the direction table
on `SubscriptionCompiler` (`compiler/mod.rs:52-67`) maps shape → direction.
**The indexer set is now strictly a discovery lane** — kind:0 / kind:3 /
kind:10002 fetches only. Content REQs (kind:1, kind:7, kind:9735, …) never
ride the indexer; `case_a_authors` / `case_b_addresses` route per author
to *NIP-65 write relays ∪ configured app relays* (additive in both
"NIP-65 known" and "NIP-65 unknown" cases). The app-relay lane is now
first-class: `UserConfiguredCategory::AppRelay`, distinct from the
`Indexer` sub-category (`crates/nmp-core/src/planner/plan.rs`).

When an author has **neither** a NIP-65 entry **nor** any configured app
relay, the planner does not silently widen — the author is collected into
`CompiledPlan::unroutable_authors: BTreeSet<Pubkey>`. The kernel
consumes this set to surface a "no relay to ask" diagnostic / toast
rather than failing or sending a relay-less REQ. This replaces the
previous behaviour of falling through to the indexer set, which had
the wrong privacy/performance shape (indexers are not content hosts).

Construction: prefer the new
`SubscriptionCompiler::with_relays(cache, indexer, account_read, app)`
constructor. The older `new(...)` and `with_active_account_read_relays(...)`
ctors still exist for backward compatibility and default the app-relay
slice to empty.

The no-authors-no-`#p` case (case D — hashtag firehose, kind-only
broad subs) routes to `active_account_read_relays ∪ app_relays`. Empty
app-relay config keeps the historical behaviour intact.

The mailbox seam is the `MailboxCache` trait
(`crates/nmp-core/src/planner/compiler/mailbox.rs:54-70`):
`get` / `snapshot_all` / `generation` / `request_probe`. Phase-1 impls ship
in-crate: `EmptyMailboxCache` (everything → indexer) and
`InMemoryMailboxCache` (`mailbox.rs:89-119`). The wiring example is in the
planner module doc (`crates/nmp-core/src/planner/mod.rs:18-25`): construct
`InMemoryMailboxCache::new()`, pass `&cache` + an indexer slice to
`SubscriptionCompiler::new`. The live `SubscriptionLifecycle` does exactly
this (`crates/nmp-core/src/subs/mod.rs:100-114` — `InMemoryMailboxCache::new()`,
`indexer_relays = ["wss://purplepag.es"]`). M3 swaps the trait impl for an
LMDB-backed one; the compiler never knows the backend
(`docs/design/subscription-compilation/nip65.md` §6.3).

The write direction is the `PublishPlanner` trait
(`docs/design/subscription-compilation/outbox.md` §7.1). Its default impl
`Nip65PublishPlanner` **structurally omits an indexer parameter** — a publish
can never fall back to indexers (D3; `subsystems.md:99` "do not publish to
indexers"). `PublishPrivacy` is `Public` / `PublicWithNotifications` /
`PrivateToRecipients`; the privacy mode is determined by the action type, not
an app parameter (`docs/design/framework-magic/outbox.md` §C7).

## Deliverable: explicit-override call-site checklist

The override exists for tests, migrations, and operator power-user flows. The
audit path is in `docs/design/subscription-compilation/outbox.md` §7.4. Before
using `PublishWithOverride`, confirm:

- [ ] You are calling the **named** `PublishWithOverride` variant — not a
      hidden `relays:` parameter on `SendNote` (there is none).
- [ ] `override_audit` is a non-empty human-readable justification (it is
      compile-time non-optional).
- [ ] The inner action's privacy mode permits override.
      `PrivateToRecipients` **rejects** any override that adds a non-inbox
      relay (`PublishPlanError::OverrideRejected`, rule 4): an override may
      *narrow* a private fan-out to a subset of declared inboxes, never
      *widen* to public relays.
- [ ] You expect a `Diagnostic::PublishOverrideUsed` on the SideEffect lane
      and a debug-level log line on **every** dispatch (one-shot, not a
      persisted default).
- [ ] You are not using it to "just add a relay" on a normal read or publish
      path — that is the anti-pattern the override audit is designed to
      surface.

## Deliverable: kind:3-arrives sequence diagram

```
relay ──kind:3 (active acct, follows {A,B,D})──▶ kernel.ingest
   │
   ├─ replaceable-supersession: fresher? ──no──▶ drop, no trigger
   │                                      └─yes─▶ replace stored kind:3
   │
   ├──▶ Trigger::FollowListChanged { prev:{A,B,C}, next:{A,B,D} }
   │
   ├──▶ SubscriptionCompiler re-runs interests() for follow-dependent views
   │      Stage 1: resolve A,B,D mailboxes
   │         (D unknown → if app_relays configured: route to app_relays;
   │                       else: D collected into unroutable_authors)
   │      Stage 3: per-relay author merge
   │      Stage 4: new plan_id
   │
   ├──▶ wire-emitter diffs plan v1 vs v2:
   │      relay3 slice for C  ──▶ CLOSE
   │      app_relay slice for D ──▶ REQ (or no slice if D unroutable;
   │                                  kernel surfaces diagnostic toast)
   │      relay1{A}, relay2{B}  ──▶ untouched (zero churn)
   │
   └──▶ view payload recomputes reactively; handle NOT destroyed
        (later: D's kind:10002 lands → Nip65Arrived → D migrates onto its
         declared write relays; app_relay slice may drop out)
```

The app writes nothing. The "following timeline" view spec names no authors;
the view module consumes the active account's follow-set internally
(`docs/design/framework-magic/kind3.md`). This structurally forbids the
classic NDK-era bug: app listens for kind:3, manually closes subs, re-derives
authors, re-issues REQs, and races itself or leaks the old REQ. NDK's
`refreshRelayConnections` only *adds* relays and never removes stale ones
(`docs/research/ndk/outbox.md` "Live subscription refresh"); Applesauce's
`OutboxModel` switchMaps each contact into its own mailbox sub but leaves
debounce to the caller (`docs/research/applesauce/outbox.md` §2). NMP's
wire-emitter diff (CLOSE removed slices, REQ added slices) avoids the
race window entirely.

## Reality check: what's still on the constant relay

**The planner and `SubscriptionLifecycle` SHIP, but the kernel's legacy REQ
emitters have not migrated to consume the `CompiledPlan` yet.** The kernel
demo still issues REQs through `RelayRole::{Content,Indexer}`
(`crates/nmp-core/src/relay.rs:14-38`), whose `.url()` returns the two
hardcoded constants `CONTENT_RELAY_URL = "wss://relay.primal.net"` and
`INDEXER_RELAY_URL = "wss://purplepag.es"`
(`crates/nmp-core/src/relay.rs:1-2`). `crates/nmp-core/src/kernel/requests/`
(profile/thread builders) still carry "scheduled for replacement by
`SubscriptionCompiler`-driven" comments
(`kernel/requests/profile.rs:5`, `kernel/requests/thread.rs:5`), and
`kernel/status.rs` / `kernel/update.rs` still render those constants.

So today's demo timeline does **not** fan per-author to NIP-65 write relays
at the wire — the compiler computes the correct `CompiledPlan`, but nothing
in the kernel REQ path applies it as a diff yet. This wiring gap is a §27
discrepancy (drift-class), recorded in the DRIFT REPORT of this delivery. Do
not assume opening a view today routes by mailbox; verify against
`subs::SubscriptionLifecycle`, not `kernel::requests`.

### `apply_selection` — landed, not yet wired

`crates/nmp-core/src/planner/selection.rs` ships an applesauce-style
greedy max-coverage post-compile mutator:
`apply_selection(&mut plan, max_connections, max_per_user)`. It trims
plans that would otherwise open too many wire connections (the
applesauce `selectOptimalRelays` algorithm, ported to Rust). It calls
`SubShape::recompute_hash()` on changed shapes and intentionally does
**not** touch `plan_id` — the plan-id contract is unchanged: same
inputs → same id.

The function is present and tested but **not yet wired** into the
kernel's live recompile path. That's a separate follow-up; today it
sits as an opt-in mutator the wiring layer will invoke once
connection-cap policy is settled.

## Anti-patterns

1. **Passing relays to `SendNote`.** No such surface exists. If you think you
   need it, you want `PublishWithOverride` (audited) or you have a routing
   bug the planner should fix.
2. **DM fallback to public relays.** A gift-wrap with no resolved recipient
   inbox must fail closed. Never widen a `PrivateToRecipients` plan to the
   author's write relays or the active session defaults.
3. **Reading mailboxes for a publish without recipients.** A plain `Public`
   event resolves only the author's write relays — no `#p`, no recipient
   inbox lookups, no indexer.
4. **Per-call relay lists in app code.** Outbox is automatic; a relay list
   threaded through app code is the hand-rolled fan-out D3 forbids.
5. **Trusting the kernel demo's wire as mailbox-routed.** Per the reality
   check, the legacy REQ path is still on the two constants.

See also: [07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md) ·
[11 — Sessions + signers + identity scopes (`nmp-signers`)](11-sessions-signers.md) ·
[12 — Publishing + the publish engine](12-publish-and-ledger.md) ·
[21 — The framework-magic contract](21-framework-magic.md)
