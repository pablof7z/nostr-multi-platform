# Offline-First Publish Intents

**Status.** Doctrine design for all NMP apps.
**Audience.** Kernel, app-shell, and test authors.
**Anchors.** [12 - Publishing + the publish engine](../builder-guide/12-publish-and-ledger.md),
[10 - Outbox routing](../builder-guide/10-outbox-routing.md),
[13 - Sync engine](../builder-guide/13-sync-engine.md),
[06 - Reactivity contract](../builder-guide/06-reactivity-contract.md).

This document names the contract above the current `PublishEngine`: every user
publish request is a durable local intent before signing, relay resolution, or
socket availability can fail. Relay delivery is a later phase of that intent.
All NMP apps get the same behavior because the state and policy live in Rust;
native shells render the queue and execute capabilities only.

## Existing Anchors

- `crates/nmp-core/src/publish/engine.rs` persists a `PublishRecord` before
  dispatch, resumes it via `resume_from_store`, and owns retry classification.
- `crates/nmp-core/src/publish/traits.rs` defines `PublishStore`,
  `PublishRecord`, and persisted `pending_retries`.
- `crates/nmp-core/src/publish/nip65/mod.rs` resolves `PublishTarget::Auto`
  from kind:10002 and fails closed for non-discovery events with no targets.
- `crates/nmp-core/src/kernel/publish_engine.rs` maps engine errors into
  snapshot state and drains due retries through the actor.
- `crates/nmp-nip77/src/triggers.rs` is the model for foreground and
  relay-reconnect fan-out without app-side polling.

Current code persists the signed delivery record after relay resolution. This
doctrine adds the earlier row: the user's publish intent must survive even
when signing is remote, target relays are not yet known, or the device is
offline.

## Required State

Persist three related records, all owned by Rust:

| Record | Created | Purpose |
|---|---|---|
| `PublishIntent` | immediately when the actor accepts the app action | Durable user intent, status, correlation ids, sanitized action tag. |
| `TargetRelayResolution` | after a signed publishable event and route decision exist | Immutable sorted relay set and route reasons used for delivery. |
| `PublishRecord` | before first `EVENT` frame | Existing per-relay delivery state, attempts, and retry deadlines. |

`PublishIntent` is the root. It carries an `intent_id`, original action id,
created time from the kernel clock, active account pubkey or signer handle,
event kind when known, queue status, and log-safe labels. It must not carry
raw nsecs, bearer tokens, bunker secrets, or plaintext private-message content
in action history or diagnostics. If a private publish cannot yet be converted
to encrypted publishable bytes, the app-domain draft is stored only in the
domain's secure Rust-owned store, referenced by id from the intent.

`TargetRelayResolution` stores the exact delivery fan-out: `Auto` or
`Explicit`, sorted relay URLs, route reasons (`author_write`,
`recipient_inbox`, `discovery_indexer`, `override`), source kind:10002 event
ids or relay-config generation, and `resolved_at_ms`. Retries use this stored
resolution. They do not silently re-run NIP-65 and change the target set.

`PublishRecord` remains the delivery-attempt row. Its `per_relay` states and
`pending_retries` are the source of truth for retry timing after restart.

## State Machine

```text
AppAction accepted
  -> persist PublishIntent(status=queued)
  -> emit AppState publish_queue row
  -> sign or await signer capability
  -> build signed publishable event
  -> resolve target relays
       no targets: persist blocked TargetRelayResolution, emit visible status
       targets: persist TargetRelayResolution + PublishRecord
  -> dispatch EVENT per stored relay
  -> fold OK / OK-false / transport facts into PublishEngine
  -> retry, settle ok/mixed/failed, or cancel
```

Fail-closed means "no wire frame is emitted." It does not mean "forget the
intent." A public note with no author write relays becomes
`blocked_no_targets` until a kind:10002 or user-configured app relay change
lets Rust resolve a valid target set. Private/gift-wrap publishes remain
blocked unless every required recipient inbox is known; they never widen to
public relays.

## Drain Triggers

Pending publish drains are event-driven:

| Trigger | Drain scope |
|---|---|
| Actor `Start` / kernel boot | load every pending intent and due `PublishRecord`. |
| App foreground | inspect all pending intents; enqueue due delivery work. |
| Relay `Connected` after offline/reconnect | drain only records targeting that relay. |
| Network reachability capability reports online | dispatch a Rust event that fans out like foreground; native does not choose retries. |
| Signer capability completes | continue that intent from `awaiting_signer`. |
| kind:10002 or app-relay config changes | re-attempt resolution for `blocked_no_targets` intents only. |
| Retry deadline due | dispatch relays whose persisted deadline is reached. |

The NIP-77 trigger model is the template: foreground covers every registered
pair; relay reconnect covers only that relay. Publish uses the same shape so
offline recovery is predictable and bounded.

## No Polling Loops

No layer may create a sleep-check loop to drain publishes. The actor may use
blocking channel waits with a timeout equal to the next retry deadline, or it
may piggy-back on an existing event tick that already wakes for other work.
Swift, Kotlin, TypeScript, and desktop shells must not schedule timers that
query publish state. They render `AppState` / `ViewBatch` updates pushed by
Rust and report raw OS facts such as foreground or network-online.

## Offline UI Contract

The UI may clear the composer and navigate away after `PublishIntent` is
durably stored, not after relay acceptance. `publish_queue` rows refine in
place:

`queued -> awaiting_signer -> resolving_targets -> blocked_no_targets |
accepted_locally -> ok | failed | cancelled`

Rows are bounded snapshots. They expose event id, kind, target count, coarse
status, and per-relay terminal outcomes. They do not expose unbounded engine
history or raw event bodies. Apps may render optimistic local content from the
Rust-owned store; they must not keep a parallel SwiftData, Room, IndexedDB, or
frontend-only queue.

## Secret Hygiene

Logs, action history, replay traces, and diagnostic snapshots may include:
intent id, action tag, kind, event id or short id, relay URL, route reason,
status token, retry count, and redacted error text.

They must not include:
raw nsecs, NIP-46 secrets, bearer/API tokens, password material, plaintext DMs,
unsealed gift-wrap content, signer challenge secrets, or full private content
that is not already a signed public Nostr event. Capability results are raw
inputs to Rust policy, but history stores only log-safe tags and correlation
ids.

## Validation Plan

- Unit tests: intent persists before signer invocation; target resolution
  persists before first dispatch; stored resolution is reused across retry;
  `blocked_no_targets` keeps the intent and emits no wire frame; secret
  redaction rejects raw nsec / plaintext private content in diagnostics.
- `nmp-core` integration: restart with pending intents resumes delivery;
  retry deadlines survive restart; relay `Connected` drains only matching
  relay targets; kind:10002 arrival resolves a blocked public intent; private
  unknown inbox remains blocked with zero outbound frames.
- `nmp-testing`: foreground trigger drains all pending publishes against
  `MockRelay`; network-online capability reports a fact and Rust chooses the
  retry; no test uses `sleep` loops or wall-clock polling.
- Cross-platform consistency: the same offline-compose scenario emits
  byte-equivalent `AppState` on iOS, Android, desktop, and web once M15
  harnesses are active.
- Live smoke: opt-in real-relay run proves airplane-mode compose, app kill,
  relaunch, foreground, relay reconnect, and eventual OK/mixed/failed status.

## Decisions

- Separate `PublishIntent` from `PublishRecord`. The extra row is the cost of
  supporting remote signers, offline start, and no-target blocking without
  losing the user's request.
- Store relay resolution immutably. This preserves the user's original fan-out
  and prevents silent target drift. Replanning is an explicit Rust transition
  for blocked intents or user-visible override flows.
- Treat no targets as blocked, not forgotten. D3 fail-closed still holds
  because no EVENT is emitted; D1/D6 improve because the user sees a stable
  queue row and can fix relay config.
- Keep native shells stateless. Platform queues are rejected even if they look
  convenient, because they split D4 ownership and cannot share NIP-65/NIP-77
  policy across apps.
