# Subscription Compilation §7 — Outbox Routing as a Planner Subsystem

> Parent: `docs/design/subscription-compilation.md`.
> Read first: `docs/product-spec/subsystems.md` §7.3 (outbox routing algorithm); `docs/aim.md` §4.4 ("outbox / smart relay routing") and §6 doctrines 5 and 10.

This section defines the **publish-side seam** the M2 milestone lands so the M6 write path has a ready surface. There is no publish code in the repo today (`crates/nmp-core/src/kernel/requests.rs` contains no `EVENT` outbound; the relay worker has no publish channel). M2 lands the trait and the override action; M6 writes the first concrete consumer (`SendNoteAction`).

This is the framing the parent index calls out as "design seam now, first concrete consumer in M6." Without this seam, M6 risks reinventing outbox routing inline.

Contrast with Applesauce: `ActionContext.publish(event, relays?)` passes outbox resolution
responsibility to each action (`docs/research/applesauce/outbox.md` §7 lines 116-138). Any
Applesauce action can pass any relay list. NMP's structural guarantee is stronger — D3
(outbox routing automatic; `docs/product-spec/overview-and-dx.md` §1.5): no normal publish
action carries a relay field, and `Nip65PublishPlanner` structurally excludes the indexer
parameter from its publish resolution path.

## 7.1 The `PublishPlanner` trait

```rust
// crates/nmp-core/src/kernel/planner/publish.rs (proposed)

#[async_trait]
pub trait PublishPlanner: Send + Sync {
    /// Compute the relay set for publishing a signed event. Pure (no side
    /// effects); the caller (action ledger) feeds the result into the
    /// per-relay publish state machine.
    fn plan_publish(
        &self,
        event: &Event,
        privacy: PublishPrivacy,
        override_: Option<PublishOverride>,
    ) -> Result<PublishPlan, PublishPlanError>;
}

pub struct PublishPlan {
    pub plan_id: PublishPlanId,                // hashes event coords + chosen relays
    pub assignments: Vec<PublishAssignment>,
    /// Ledger acceptance threshold: `usize` (not `u8`) so large recipient lists
    /// cannot overflow. For public events this is `max(1, ceil(N/3))`; for private
    /// this equals the per-recipient delivery requirements count.
    pub required_success_count: usize,
    /// For `PrivateToRecipients`: one entry per recipient with their specific
    /// inbox relay set and success threshold. The ledger fails the publish if
    /// any recipient has zero successful deliveries.
    pub per_recipient: Vec<RecipientDelivery>,  // empty for Public/PublicWithNotifications
    pub deadline_ms: u64,
}

pub struct RecipientDelivery {
    pub recipient: Pubkey,
    pub inbox_relays: BTreeSet<RelayUrl>,
    pub required_success_count: usize,          // must be ≥ 1
}

pub struct PublishAssignment {
    pub relay_url: RelayUrl,
    pub reason: PublishRouteReason,            // which lane motivated this relay
    pub privacy_role: PrivacyRole,             // for the audit log; not policy
}

pub enum PublishRouteReason {
    AuthorWriteRelay  { lane: RelayFactLane },     // Nip65 or UserConfigured
    RecipientInbox    { recipient: Pubkey, lane: RelayFactLane },
    Hint              { source: HintSource },
    Override          { audit: String },           // see §7.4
}

pub enum PrivacyRole {
    Author,        // this relay is in the plan as the author's outbox
    Recipient,     // this relay is in the plan as a recipient's inbox
    Both,          // single relay is both
}

#[derive(Clone, Debug)]
pub enum PublishPrivacy {
    /// Public events (kind:1, kind:0, kind:3, kind:10002, kind:7, ...).
    /// Fails with `NoAuthorRelays` if the author has no declared write relays.
    /// Does NOT fall back to indexers (D3) — publishes must reach only declared write relays.
    Public,
    /// Private/gift-wrapped events (kind:1059 wrapping NIP-44). Fails closed
    /// if any recipient has no inbox relays.
    PrivateToRecipients { recipients: Vec<Pubkey> },
    /// Notifications (kind:1 with `#p` tags, reactions, zaps, replies that
    /// the author wants to surface to the tagged pubkey). Combines author
    /// write + each `#p` inbox.
    PublicWithNotifications { notify: Vec<Pubkey> },
}

#[derive(Clone, Debug)]
pub enum PublishPlanError {
    NoAuthorRelays { author: Pubkey, lane_facts: ByLaneCounts },
    PrivateRecipientUnroutable { recipient: Pubkey },
    OverrideRejected { reason: String },
}
```

The trait is consumed by the action ledger (per `docs/design/kernel-substrate.md` §4 — the kernel owns "per-relay publish attempts" provenance). When an `ActionModule::reduce` reaches its publish step, it calls the planner, gets a `PublishPlan`, and the kernel fans out to relays with the standard ledger-correlated retry/cancel semantics.

## 7.2 Default implementation: `Nip65PublishPlanner`

```rust
// crates/nmp-core/src/kernel/planner/publish_default.rs (proposed)

pub struct Nip65PublishPlanner<'a> {
    pub mailbox_cache:    &'a dyn MailboxCache,
    pub user_configured:  &'a UserConfiguredRelays,
    // intentionally no `indexer_set` field: publish NEVER falls back to indexers (D3).
    // The indexer set is a read-only discovery mechanism (§3.2). Publishing to an
    // indexer the author has not declared as a write relay violates D3 (outbox routing
    // automatic — `docs/product-spec/overview-and-dx.md` §1.5) and the invariant at
    // `docs/product-spec/subsystems.md` §7.3 line 99.
    pub active_account:   Option<AccountId>,
}

impl PublishPlanner for Nip65PublishPlanner<'_> {
    fn plan_publish(&self, event: &Event, privacy: PublishPrivacy,
                    override_: Option<PublishOverride>) -> Result<PublishPlan, PublishPlanError>
    { /* algorithm in §7.3 */ }
}
```

This is the only `PublishPlanner` impl shipped in v1. The trait exists so a future `Wot​PublishPlanner` (M13 WoT subsystem) or a sandbox planner used in tests can replace it without touching action-ledger code.

## 7.3 Write fan-out algorithm (per `docs/product-spec/subsystems.md` §7.3)

Inputs: a signed `event`, a `PublishPrivacy` mode, an optional `PublishOverride`.

The algorithm deliberately does **not** accept an indexer set. Indexers are read-only
discovery infrastructure (compiler Stage 2, §3.2). A publish planner that accepted an
indexer argument would make it too easy to accidentally route writes to indexers.
Compare: Applesauce's `ActionContext.publish(event, relays?)` is caller-responsibility —
any action can pass any relays (`docs/research/applesauce/outbox.md` §7 lines 116-138).
NMP's planner is structural: publish resolution and read-fallback are separate code paths.

The legitimate fallback when an author has no declared NIP-65 write relays is the
operator-/user-configured **app-relay** set (`UserConfiguredCategory::AppRelay`). App
relays are **additive** to NIP-65 in both directions (read and publish) — they are the
correct cold-start substitution that the indexer was historically (and wrongly) being
asked to provide on the read side. The "no indexer fallback" structural argument
stands: app relays are a declared operator/user choice, not a global discovery commons.

```
1. If `override_` is Some:
     a. Derive the allowed base set first (steps 2-3 without override).
     b. For PrivateToRecipients: validate that override_relays ⊆ declared inboxes.
        If any override relay is not a declared inbox, return Err(OverrideRejected).
     c. Apply the override (narrow the base set to override_relays).
     d. Set every PublishRouteReason::Override; continue to step 4.
2. Resolve author write relays (no indexer fallback; app_relays are additive):
     author_outbox = resolve_author_outbox_no_indexer(cache, user_configured, event.pubkey)
     app_relays    = user_configured.app_relays()
     write_set     = author_outbox.relays ∪ app_relays
     If write_set is empty:
         return Err(NoAuthorRelays { ... })  // fail; the kernel surfaces a toast.
                                             // App relays are the legitimate fallback;
                                             // indexers remain out of the publish path.
3. Match on privacy:
   a. Public:
        // author_outbox.relays carries the Nip65 lane; app_relays carry
        // the UserConfigured(AppRelay) lane. Tag each relay with the lane
        // that introduced it.
        assignments = [each r ∈ write_set → AuthorWriteRelay { lane: lane_of(r) }]
        required_success_count = max(1, ceil(N/3))   // configurable via AppConfig
   b. PrivateToRecipients { recipients }:
        For each recipient r:
            inbox = resolve_author_inbox(cache, user_configured, r)
            // no indexer arg — NDK gotcha a912a2c2: bootstrap timing race means
            // we may not have inbox relays yet; fail-closed is correct for privacy
            // Check for unroutable inbox: either no relays at all, or the only
            // routing source was indexer fallback (UserConfigured::Indexer means
            // we have no declared inbox — a privacy-fail-closed condition).
            let inbox_from_indexer = matches!(
                inbox.source, RoutingSource::UserConfigured
            ) && user_configured.is_indexer_relay(&inbox.relays);
            If inbox.relays.is_empty() || inbox_from_indexer:
                return Err(PrivateRecipientUnroutable { recipient: r })
        assignments = union(each recipient's inbox.relays → RecipientInbox { recipient, lane })
        // intentionally NO author-write inclusion: private events must not go to public outbox
        // per-recipient delivery requirements: one requirement per recipient
        per_recipient_required = recipients.iter().map(|r| {
            RecipientDelivery {
                recipient: r.clone(),
                inbox_relays: recipient_inboxes[r].clone(),
                required_success_count: 1usize,  // usize, not u8 — avoid overflow for large recipient lists
            }
        }).collect()
        required_success_count = per_recipient_required.len()  // all recipients must receive
   c. PublicWithNotifications { notify }:
        assignments = write_set ∪ union(each notify pubkey's inbox)
        // write_set already includes app_relays; #p-tagged recipients add
        // their NIP-65 inbox relays on top.
        required_success_count = max(1, ceil(write_set.len() / 3))
4. plan_id = blake3(event.id, sorted assignments)
5. deadline_ms = now + AppConfig.publish_deadline_ms (default 30_000)
6. Return PublishPlan { plan_id, assignments, required_success_count, deadline_ms }
```

Notes on the algorithm:

- **Step 2's `resolve_author_outbox_no_indexer`** is the structural enforcement of D3 and
  `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only;
  do not publish to indexers." The function signature deliberately omits the indexer set so
  the caller cannot accidentally pass it. The union with `app_relays` is the legitimate
  cold-start fallback: app relays are operator/user-declared, lane-tagged
  (`UserConfiguredCategory::AppRelay`), and additive to NIP-65 — they take over the role
  indexers were previously (and wrongly) being asked to play. A failed Step 2 (both
  NIP-65 write relays AND app relays empty) surfaces as `Failed { reason: NoAuthorRelays }`
  in the action ledger, rendered as a toast per ADR-0007's `SideEffect` lane.
- **Step 1's override validation order** (derive base set first, validate override as subset)
  ensures the privacy constraint is always checked — the override cannot bypass the
  `PrivateToRecipients` fail-closed check by returning early before validation.
- **Step 3(b)'s `UserConfigured`-indexer-fallback check** is the structural enforcement of
  bug-extinction #4 ([`docs/plan/m9-messaging.md`](../../plan/m9-messaging.md) — "DM to
  public: no API path can send a DM to a non-inbox relay"). An inbox sourced from
  `RoutingSource::UserConfigured` where the relay carries `UserConfiguredCategory::Indexer`
  means we have no NIP-65-declared inbox — fail-closed is correct. Compare: NDK gotcha
  `a912a2c2` (`docs/research/ndk/gotchas.md`) shows bootstrap timing can leave inbox relays
  empty at first query — NMP's fail-closed is intentional rather than "retry later."
- **`required_success_count: usize`** prevents overflow when `recipients.len()` is large. The
  `u8` type in the original draft was wrong (max 255 recipients before silent truncation). For
  `PrivateToRecipients`, the per-recipient `RecipientDelivery` set is what the ledger actually
  consults; `required_success_count` is the aggregate guard.

## 7.4 The `PublishOverride` escape hatch

The override exists for tests, migration tools, and operator power-user flows. Per `docs/aim.md` §6 doctrine 5 ("manual relay selection is the opt-out, not the default") and `docs/product-spec/subsystems.md` §7.3 line 90 ("explicit overrides are named, one-shot, and debug-flagged in logs"), the override must be:

1. **Named** — its own typed `AppAction` variant, not a hidden parameter on `SendNote`.
2. **One-shot** — does not persist as a default for future publishes.
3. **Audited** — emits a `Diagnostic::PublishOverrideUsed { reason, action_id }` on the `SideEffect` lane and writes a debug-level log line on every dispatch.
4. **Refused for privacy-sensitive modes** — `PublishPrivacy::PrivateToRecipients` rejects an override that adds non-inbox relays. The override may *narrow* a private fan-out to a subset of declared inboxes; it may not *widen* to public relays.

### The override action

```rust
// crates/nmp-core/src/kernel/actions/publish_override.rs (proposed)

#[derive(Clone, Serialize, Deserialize)]
pub struct PublishWithOverride {
    pub inner: AppAction,                    // the underlying publish action
    pub override_relays: Vec<RelayUrl>,
    pub override_audit: String,              // human-readable justification
}

pub struct PublishOverride {
    pub relays: Vec<RelayUrl>,
    pub audit:  String,
}

// In the action ledger:
impl ActionModule for PublishWithOverride {
    const NAMESPACE: &'static str = "kernel.publish_override";
    type Action = PublishWithOverride;
    type Step   = PublishOverrideStep;
    type Output = PublishResult;

    fn start(cx: &mut ActionContext, a: Self::Action)
        -> Result<ActionPlan<Self::Step>, ActionRejection>
    {
        // Emit the debug warning immediately. This is the audit trail.
        cx.emit_side_effect(SideEffect::Diagnostic(
            Diagnostic::PublishOverrideUsed {
                action_id: cx.id(),
                reason: a.override_audit.clone(),
                relays: a.override_relays.clone(),
            },
        ));
        cx.log_warn(format!(
            "OUTBOX OVERRIDE used by action {} → {} relays: {}",
            cx.id(), a.override_relays.len(), a.override_audit
        ));
        // ... validate that inner action's privacy mode permits override ...
    }

    fn reduce(...) { /* delegate to inner action, but pass `override_` to PublishPlanner */ }
}
```

The override action's existence is what test #2 in the bug-extinction list ([`docs/plan/m2-subscription-compilation.md`](../../plan/m2-subscription-compilation.md)) asserts: "no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning." The `PublishWithOverride` variant is the *only* `AppAction` that carries a relay set; the audit string is required (compile-time non-optional); the warning fires unconditionally on dispatch.

### Diagnostic shape

```rust
pub enum Diagnostic {
    PublishOverrideUsed {
        action_id: ActionId,
        reason: String,
        relays: Vec<RelayUrl>,
    },
    // ... other variants ...
}
```

This is the SideEffect-lane payload per ADR-0007. The platform diagnostic UI renders an entry on every override; the count over a session is a soft metric (Cargo dependents using overrides routinely should re-examine their flow).

## 7.5 Atomicity contract

Per `docs/design/kernel-substrate.md` §4 ("Atomicity"): the action ledger ensures the action's local store insert (for the signed event) happens in the same actor message as the ledger transition. The publish plan's per-relay attempts are *not* atomic with the local insert — relays may NACK over a long window — but the ledger correlates them.

The bug-extinction #7 test ([`docs/plan/m6-signers-write.md`](../../plan/m6-signers-write.md)) — "publish OK / store fail and store OK / publish fail both roll back atomically" — runs against the M6 implementation. The seam M2 lands here must make that test possible. Specifically:

- The publish-fanout step in `PublishWithOverride::reduce` is `AwaitCapability { request: CapabilityRequest::Publish { ... }, next_step }` per the `ActionTransition` enum in `docs/design/kernel-substrate.md` §4. The kernel owns the publish attempts and reports per-relay outcomes back into the next `reduce`.
- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).

## 7.5.1 Address-pointer routing on the publish path

When an event references a `NaddrCoord` (via an `#a` tag, e.g. a NIP-22 kind:1111 comment on a NIP-23 article), the publish routing follows `PublicWithNotifications`:

- The **author's own write relays** receive the event (standard D3 outbox rule).
- If the addressed event's author is `#p`-tagged, their **inbox relays** are added via the existing `notify: Vec<Pubkey>` path in `PublishPrivacy::PublicWithNotifications`.

The publish planner does **not** require a new routing variant for address pointers — the `#p` inbox lane already covers the notification path. What M2 adds is the *subscription-side* routing: `InterestShape::addresses` causes Stage 1 to resolve each `NaddrCoord::pubkey` as an Outbox direction (the addressed author's write relays), routing the REQ to where the article lives. The publish fan-out is unchanged.

## 7.6 What M2 does not cover (deferred)

- **Action ledger schema** — `docs/design/kernel-substrate.md` §4 is the design; M6 implements.
- **Retry policy** — exponential backoff parameters land in M6.
- **Concurrent publish coalescing** — if two actions publish the same event (a republish), the planner can dedupe to one wire EVENT per relay. Defer to M6 / M7 stress test.
- **NIP-42 auth challenge during publish** — relays may demand AUTH before accepting an EVENT. Wires up in M5.

The publish-planner trait is intentionally finished enough that the M6 implementation does not need to extend it. That is the seam the milestone gates against.
