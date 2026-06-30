# Write Intents, Composable Drafts, and Route Provenance

> Doctrine anchors: ADR-0064 (unified write boundary), ADR-0071 (publish intents + route
> provenance), D3 (automatic routing), D6 (errors as state), D7 (capabilities report, kernel
> decides), D10 (private fail-closed). See also `docs/builder-guide/12-publish-and-ledger.md`.

## The One Write Doorway

All app writes enter Rust through a single byte-transport door:

- **Native:** `dispatch_action(Vec<u8>) -> DispatchOutcome`
- **Wasm:** `WorkerRequest::DispatchBytes { bytes }`

The payload is a `DispatchEnvelope` (FlatBuffers, file id `NMPD`) carrying a `correlation_id`,
a generated `action_namespace`, and a typed per-crate payload. App code never hand-writes the
namespace; generated host builders stamp it. Adding a NIP write is zero new C symbols: register
an `ActionModule`, extend codegen.

## The Sanctioned App Write Door Is a Typed Intent

App code dispatches **typed, intentful** writes (`PublishProfile`, `PublishReply`, and the
generated builders) — not raw bytes. `PublishRaw` still exists as the generic substrate the
builders sit on, but WRITE-001 banned it as the *normal* DX/starter write path. A starter or
product screen reaching for `PublishRaw` is a smell: prefer the typed builder for the kind.

## Dispatch ≠ Success

`DispatchOutcome { correlation_id, error, code }` is the *acceptance* outcome — validation
passed, action enqueued. It is **not** publish success. Terminal status arrives
asynchronously via the `PublishStatusView` projection / `action_results` state on a later
actor tick. Shell code that marks a write "done", clears a spinner, or navigates away on the
basis of a non-null `correlation_id` is a D6 violation.

The `correlation_id` is the durable operation identity from dispatch through every engine
stage (`Accepted → Planning → InFlight → Published / Failed / Cancelled`). It must never be
re-bound to or replaced by the Nostr event id; the event id is output data, not operation
identity (ADR-0064 drift gate).

## Four-Stage Actor Pipeline

```
1. construct unsigned draft / builder
        ↓
2. finalize protocol envelope
   (NIP-29 h-tag, NIP-17 private envelope, client identity tag, NIP-22 reply shape)
        ↓
3. sign through the selected signer capability (ADR-0050 port)
        ↓
4. route via OutboxResolver or named Explicit set → publish engine
```

Stages are sequential and actor-owned; none may collapse or move to app shell code.
Finalizers may mutate unsigned content before signing. After signing, event id and signature
are immutable.

## Composable Draft Builders

A builder (`reply_to`, `react_to`, `new_article`, group-publish helpers) produces a mutable
**unsigned** draft. It does **not** imply where the event publishes, which signer signs it, or
whether the draft is final, and it must **not** call the signer, dispatch to the relay pool,
or mutate protocol envelope tags as a hidden side effect. The app hands the draft to the
dispatch path, which drives stages 2–4.

## Typed Signer Selection

```rust
enum PublishSigner { Active, Registered { pubkey, provenance } }
enum PublishSignerProvenance { AppManaged | UserSelected | ProtocolPinned | Diagnostic }
```

Shells dispatch these selectors. They do not hold `Arc<dyn Signer>`, call `.sign()`, or choose
the signing key. An unknown pubkey is a structured sign-stage failure (toast), not a dispatch
error. Whether the key is local (synchronous) or remote (NIP-46, parked async) is transparent
to the caller. Signing and publishing are orthogonal capabilities — never coupled.

## Typed Route Provenance

```rust
enum PublishTarget { Auto, Explicit { relays, route_class } }
enum PublishRouteClass {
    ManualOverride | GroupHostPin | VerifiedPrivateInbox | ImportedOrPresigned | Diagnostic
}
```

`PublishTarget::Auto` is the D3 default (NIP-65 outbox). `Explicit` is the named opt-out and
**both** fields are required. `PublishRouteClass` has no `Default` impl and no public
`manual_override()` helper — a doctrine-lint gate
(`crates/nmp-testing/bin/doctrine-lint/publish_route_gates.rs`) enforces this, so anonymous
explicit relay lists cannot be constructed by app code. Shells receive a
`PublishTargetSelection` from generated builders and pass it through without inspecting relay
URLs.

## Private Events: Fail-Closed (D10)

kind:1059 (gift-wrap) and kind:14 (sealed DM) with `PublishTarget::Auto` are rejected by
`PublishModule::start()`. They must carry `Explicit { VerifiedPrivateInbox }`. Unknown
recipient inboxes emit no wire frame — they are never widened to public relays. The publish
planner has no indexer parameter; a publish can never fall back to indexers.

## Pre-Signed Publish: Protocol-Owned Only

`PublishAction::Publish { event, target }` (pre-signed verbatim publish) is rejected at the
app-facing dispatch layer. WRITE-005 restricts pre-signed verbatim publish to protocol-owned
seams (e.g. Marmot/MLS wire events routed via the event's own pubkey outbox); it is not a
general app write door. Pre-signed paths require `ImportedOrPresigned`, `GroupHostPin`, or
`VerifiedPrivateInbox` provenance — `ManualOverride` and `Diagnostic` are rejected for
externally signed events.

## Three-Layer Intent Lifecycle (offline-durable)

```
PublishIntent          — created when the actor accepts dispatch (survives offline + signer wait)
TargetRelayResolution  — immutable relay fan-out after signing (retries reuse the stored set,
                         not a fresh NIP-65 query)
PublishRecord          — per-relay delivery state, attempts, retry deadlines
```

All three are Rust-owned (`crates/nmp-core/src/publish/`). Native holds none of them; a
SwiftData/Room/IndexedDB queue mirroring any layer is a D4 violation. The UI may clear the
composer and navigate away after `PublishIntent` is durably stored — not after relay
acceptance.

## Drain Triggers — Event-Driven, Not Polled

Drains fire on: actor Start, app foreground, relay `Connected` (records for that relay),
network-online capability, signer completion, kind:10002 / app-relay change (re-attempts
`blocked_no_targets`), and per-relay retry deadlines. No shell may sleep-loop or use a platform
timer to drain publishes. Retry classification (`classify_ack`) is the engine's, not native's
(D7).

## Blocking Anti-Patterns

| Anti-pattern | Rule |
|---|---|
| Shell treats `correlationId != null` as publish confirmation | D6 — dispatch ≠ success |
| `Explicit` relay target without a typed `PublishRouteClass` | ADR-0071 — anonymous route |
| App builds, signs, and websocket-sends by hand | D3/D4 — no ledger, no retry, no outbox |
| App reaches for `PublishRaw` as the normal write path | WRITE-001 — use the typed builder |
| Native stores pending publishes (SwiftData, Room, …) | D4 — second writer |
| Native decides retry or returns `isTransient` | D7 — `classify_ack` is engine-owned |
| Draft builder calls signer or dispatches directly | ADR-0071 — construction ≠ publish |
| `correlation_id` re-bound to event id at terminal state | ADR-0064 — identity must not switch |
| Private/DM publish with `Auto` routing | D10 — must be `VerifiedPrivateInbox`, fail-closed |
