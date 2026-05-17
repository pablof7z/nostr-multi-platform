# Subscription Compilation §7 — Outbox Routing as a Planner Subsystem

> Parent: `docs/design/subscription-compilation.md`.
> Read first: `docs/product-spec/subsystems.md` §7.3 (outbox routing algorithm); `docs/aim.md` §4.4 ("outbox / smart relay routing") and §6 doctrines 5 and 10.

This section defines the **publish-side seam** the M2 milestone lands so the M6 write path has a ready surface. There is no publish code in the repo today (`crates/nmp-core/src/kernel/requests.rs` contains no `EVENT` outbound; the relay worker has no publish channel). M2 lands the trait and the override action; M6 writes the first concrete consumer (`SendNoteAction`).

This is the framing the parent index calls out as "design seam now, first concrete consumer in M6." Without this seam, M6 risks reinventing outbox routing inline.

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
    pub required_success_count: u8,            // ledger acceptance threshold
    pub deadline_ms: u64,
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
    /// Falls back to indexer set if author has no write relays.
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
    pub indexer_set:      &'a [RelayUrl],
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

```
1. If `override_` is Some:
     return plan from override (see §7.4); set every PublishRouteReason::Override.
2. Resolve author write relays:
     author_outbox = resolve_author_outbox(cache, user_configured, indexer, event.pubkey)
     If author_outbox.relays is empty:
         return Err(NoAuthorRelays { ... })  // never fall back to indexer for writes
3. Match on privacy:
   a. Public:
        assignments = [each author_outbox.relays → AuthorWriteRelay { lane }]
        required_success_count = max(1, ceil(N/3))   // configurable
   b. PrivateToRecipients { recipients }:
        For each recipient r:
            inbox = resolve_author_inbox(cache, user_configured, indexer, r)
            If inbox.source == Indexer or inbox.relays is empty:
                return Err(PrivateRecipientUnroutable { recipient: r })
        assignments = union(each recipient's inbox.relays → RecipientInbox { recipient, lane })
        // intentionally NO author-write inclusion: private events do not go to public outbox
        required_success_count = recipients.len() as u8  // at least one per recipient
   c. PublicWithNotifications { notify }:
        assignments = author_outbox ∪ union(each notify pubkey's inbox)
        required_success_count = max(1, ceil(author_outbox.len() / 3))
4. plan_id = blake3(event.id, sorted assignments)
5. deadline_ms = now + AppConfig.publish_deadline_ms (default 30_000)
6. Return PublishPlan { plan_id, assignments, required_success_count, deadline_ms }
```

Notes on the algorithm:

- **Step 2's "no indexer fallback for writes"** is the structural enforcement of the doctrine `docs/product-spec/subsystems.md` §7.3 line 99: "fall back to indexer set for reads only; do not publish to indexers." A failed Step 2 surfaces in the action ledger as `Failed { reason: NoAuthorRelays }`, which the UI renders as a toast per ADR-0007's `SideEffect` lane.
- **Step 3(b)'s `Indexer` check** is the structural enforcement of bug-extinction #4 (`docs/plan.md` line 306 — "DM to public: no API path can send a DM to a non-inbox relay"). Indexer-sourced inbox means we have no NIP-65-declared inbox; for private events that is fail-closed. The recipient gets nothing rather than getting a public broadcast.
- **`required_success_count`** is the threshold below which the ledger marks the publish `PartiallyFailed`. The default ⅓-of-fan-out is tunable per `AppConfig.publish_quorum_ratio`.

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

The override action's existence is what test #2 in the bug-extinction list (`docs/plan.md` line 134) asserts: "no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning." The `PublishWithOverride` variant is the *only* `AppAction` that carries a relay set; the audit string is required (compile-time non-optional); the warning fires unconditionally on dispatch.

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

The bug-extinction #7 test (`docs/plan.md` line 234) — "publish OK / store fail and store OK / publish fail both roll back atomically" — runs against the M6 implementation. The seam M2 lands here must make that test possible. Specifically:

- The publish-fanout step in `PublishWithOverride::reduce` is `AwaitCapability { request: CapabilityRequest::Publish { ... }, next_step }` per the `ActionTransition` enum in `docs/design/kernel-substrate.md` §4. The kernel owns the publish attempts and reports per-relay outcomes back into the next `reduce`.
- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).

## 7.6 What M2 does not cover (deferred)

- **Action ledger schema** — `docs/design/kernel-substrate.md` §4 is the design; M6 implements.
- **Retry policy** — exponential backoff parameters land in M6.
- **Concurrent publish coalescing** — if two actions publish the same event (a republish), the planner can dedupe to one wire EVENT per relay. Defer to M6 / M7 stress test.
- **NIP-42 auth challenge during publish** — relays may demand AUTH before accepting an EVENT. Wires up in M5.

The publish-planner trait is intentionally finished enough that the M6 implementation does not need to extend it. That is the seam the milestone gates against.
