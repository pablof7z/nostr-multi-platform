# ADR-0064 — Unified write/command boundary: one typed byte transport, open per-crate FlatBuffers payloads, signing as a capability round-trip

- **Status:** Accepted and implemented for the production binding transport
  (owner-decided 2026-06-21; confirmed 2026-06-25 after the JSON-dispatch
  cleanup landed); amended by ADR-0071.
- **Date:** 2026-06-21
- **Decides:** Collapse the write/command path to **one boundary shape, shared by
  the native FFI and the wasm worker**: a single generic *byte* transport doorway
  carrying an **open envelope** (`correlation_id` + generated `action_namespace` +
  typed FlatBuffers `payload`), where each action's payload is a **typed
  FlatBuffers table owned by its own crate** and exposed to apps only through
  **generated typed builders** (`client.publishNote(...)`), never a hand-written
  namespace string. Signing for **every** backend (local key, NIP-07 browser,
  NIP-46 bunker, NIP-55 Android) becomes one replayable **capability round-trip**
  on the ADR-0050 signer-session port — the wasm `Arc<dyn Signer>.await`-inside-the-
  publish-flow path is deleted. `correlation_id` is the operation identity from
  dispatch to terminal, and never switches to the event id.
- **Extends:**
  - **ADR-0027** (unify the `ActionModule` trait) — keeps the single `ActionModule`
    registry as the one dispatch authority; this ADR changes only the *wire shape*
    the registry is reached through (JSON → typed FlatBuffers bytes) and finishes
    the seam ADR-0027 left structurally vestigial (multiple bespoke event-producing C
    symbols bypassing the one `dispatch_action` doorway).
  - **ADR-0050** (signer-session capability port: `sign | nip44_encrypt |
    nip44_decrypt`, mailbox-delivered completions) — this ADR carries that port
    **across the wasm boundary unchanged in shape**: NIP-07 becomes one more
    fulfiller of the existing `sign` verb, delivered by the same mailbox-completion
    model. No second signing mechanism.
  - **ADR-0040** (capability-worker seam) and **ADR-0024** (async capability
    protocol) — the sign request/response rides the established capability channel
    (native reports, the kernel decides — D7), not a bespoke publish-only path.
  - **ADR-0037 / 0044 / 0055** (typed FlatBuffers projections, read direction) —
    applies the same IDL + drift-gate treatment the *read* direction already has to
    the *write* direction, so one schema language and one `rust-flatc-drift` gate
    cover both.
- **Reaffirms:**
  - **V-78 / ADR-0050 binding doctrine** — the signer backend (local vs NIP-07 vs
    NIP-46 vs NIP-55) is invisible to the action vocabulary. Action payloads carry
    **no** `signer_hint` / backend selector; the kernel owns which fulfiller answers
    a sign request.
  - **D0** — `nmp-core` names no app/protocol noun: the write envelope is open and
    enumerates no action; protocol crates contribute typed payloads through the
    registry seam.
  - **D6** — no `Result`/exception/panic crosses the boundary; failures are data
    (action-stage state).
- **Doctrines touched:** D0 (open seam, no app noun in core), D4 (one writer / one
  dispatch authority), D6 (errors are state), D7 (capabilities report, kernel
  decides), D8 (no polling; completions wake the actor as explicit events).

- **Current disposition under ADR-0071:** the one typed byte/action doorway
  remains. Event construction, finalization, signing, and publishing are distinct
  Rust-owned stages, and publish status must preserve durable intent identity and
  structured route provenance. Anonymous explicit relay lists are not sufficient
  product publish state.

---

## Context

### The write path needs one vocabulary for one concept

Native and wasm writes must reach the same `ActionModule` registry through one
open envelope. A NIP crate self-registers an `ActionModule` (`const NAMESPACE`,
typed payload, `start()` validator, `execute()`), and adding an action needs zero
new C symbols.

The wasm worker must not carry a second, platform-specific typed write vocabulary
beside the shared registry. Signing also uses the same ADR-0050 capability port
as native so browser and native writes share one replayable side-effect model.

### Why the string namespace is not the problem (and where the real fix is)

The owner's discomfort — *"why is the action's identity a string literal? why not
a typed `nmp_publish()`?"* — is correct only insofar as **a human should never
write that string**. The string is the extension key of an *open* registry over one *generic* doorway;
that openness is load-bearing (D0: the kernel never learns the noun "react"). The defect is when app and host code hand-assemble
wire payloads. The fix is not to close the command set into a
central enum (that would break the open seam and force a `nmp-core` edit per NIP) —
it is to push **typing to a generated layer above one open transport**.

### Alternatives considered

- **Make the wasm writes first-class `WorkerRequest::{PublishNote, React, …}`
  variants** (the original framing of issue #1743). **Rejected** — it deepens the
  native/wasm divergence by promoting the anomaly into the protocol, and hard-codes
  a closed, wasm-only action set that no NIP crate can extend.
- **One central typed command enum / FlatBuffers union as the single source of
  truth** (`enum NmpWrite { PublishNote, React, … }`, codegen both ends).
  **Rejected** — compile-time exhaustive but *closes* the command set: a NIP crate
  could not add a write without editing the central enum, violating D0 and the open
  seam.
- **N real typed C symbols** (`nmp_publish`, `nmp_react`, …). **Rejected** — every
  symbol must be hand-bound per host (Swift/Kotlin/wasm) and breaks the C-ABI
  doorway; adding a NIP becomes an FFI-surface change.

---

## Decision

### 1. One generic *byte* transport doorway per boundary

The write path is a single, action-agnostic doorway carrying FlatBuffers bytes:

- **Native:** `nmp_app_dispatch_action_bytes(app, ptr, len) -> accepted_or_error`
  (returns the correlation id or a data-shaped error — D6, no `Result` across FFI).
- **Wasm:** `WorkerRequest::DispatchBytes { bytes }`.

There is exactly one event-producing write doorway on each boundary.

### 2. Open envelope, typed per-crate payloads

`nmp-core` owns an **open** envelope that enumerates no action:

```fbs
// nmp-core — open; never names an action
table DispatchEnvelope {
  correlation_id:string;     // host-supplied operation identity (§4)
  action_namespace:string;   // GENERATED stable key, never hand-written by app code
  schema_version:uint;       // payload schema version for forward-compat
  payload:[ubyte];           // a FlatBuffers root owned by the action's crate
}
root_type DispatchEnvelope;
```

Each action's payload is a **typed FlatBuffers table owned by the registering
crate**, compile-time-checked in isolation:

```fbs
// nmp-publish
table PublishNote { content:string; tags:[Tag]; }
// nmp-nip25
table React { target_event_id:string; reaction:string; }
// nmp-nip02
table Follow { pubkey:string; }
```

The `ActionModule` registry stays the one dispatch authority; an `ActionModule`
now declares its FlatBuffers payload type and stable namespace, and the registry
decodes `payload` against the registered schema before `start()`.

### 3. Generated typed builders are the only app-facing surface

App and host code **never** spell `action_namespace`. Codegen emits typed builders
per host from the registered modules:

```ts
client.publishNote({ content, tags })   // TS / wasm
client.react({ eventId, reaction })
client.follow({ pubkey })
```

with Swift/Kotlin equivalents. The builder stamps the generated namespace +
schema version into a `DispatchEnvelope` and sends the bytes through the one
doorway. The namespace string exists only inside generated code; the open
transport and the typed interface coexist via the codegen layer.

### 4. `correlation_id` is the operation identity, end to end

The host-supplied `correlation_id` identifies the operation from dispatch through
every stage — `Accepted → BuildingUnsigned → WaitingForSignature →
SignatureRejected → Publishing → Published / Failed / Cancelled` — surfaced via the
existing `action_stages` / `action_results` projections. It **must never** be
replaced by the event id (the current pre-signed-publish defect, where the event id
becomes the terminal correlation id and breaks host spinner matching). Event id is
output *data*, not operation *identity*.

### 5. Signing is one capability round-trip for every backend

Signing is not a synchronous native call and not a wasm `await`-inside-the-flow. It
is the ADR-0050 capability port, identical in shape on both boundaries:

```
dispatch (typed bytes)
  → ActionModule::start() validates
  → actor builds the UNSIGNED event in Rust
  → PendingSign { correlation_id, unsigned, publish_ctx }
  → emit Sign capability request   (ADR-0050 `sign` verb, FlatBuffers)
  → [fulfiller signs or rejects] → completion re-enters as an explicit actor event
  → actor validates signed event → publishes (automatic outbox routing) → terminal action_result
```

- **Local key** — the inline fulfiller; completes synchronously, same event path.
- **NIP-07 web** — the capability bridge calls `window.nostr.signEvent()` and posts
  `SignSuccess` / `SignFailure` bytes back to the worker; the actor resumes.
- **NIP-46 bunker / NIP-55 Android** — the existing remote fulfillers report raw
  success/rejection; the kernel decides next state (D7).

The reducer never awaits the world; the signer backend stays invisible to the
action vocabulary (V-78). The sign request/response is typed FlatBuffers, e.g.:

```fbs
// nmp capability channel — sign verb (ADR-0050)
table SignRequest  { capability_id:string; correlation_id:string; unsigned_event:[ubyte]; }
table SignSuccess  { capability_id:string; correlation_id:string; signed_event:[ubyte]; }
table SignFailure  { capability_id:string; correlation_id:string; code:string; message:string; }
```

### 6. Drift gates lock the shape in

CI gates (extending `rust-flatc-drift` and the doctrine lint) enforce:

- no new event-producing C symbol (the one byte doorway is the only one);
- no event-producing path that bypasses `ActionModule::start()` validation
  (closes the current `PublishSignedEvent` validation-bypass hole);
- no hand-written wasm action enum;
- no `signer_hint` / backend selector on an action payload;
- a `correlation_id` that never re-binds to an event id.

---

## Consequences

- **One write vocabulary, two boundaries.** Native and wasm reach the identical
  registry through the identical envelope; there is no platform-specific write
  protocol. App authors get typed builders on every platform.
- **Adding a NIP write stays a zero-FFI, zero-core-edit operation.** The crate
  registers an `ActionModule` with a FlatBuffers payload; codegen surfaces the
  builder; no new C symbol, no central-enum edit.
- **Signing is solved once.** NIP-07 joins the ADR-0050 signer port as a
  fulfiller. Web and native share one replayable flow.
- **Known defects are fixed by construction:** the `PublishSignedEvent`
  validation-bypass, the pre-signed-publish event-id-as-correlation-id spinner bug,
  and the `app_action` divergence all close in the migration rather than persisting.
- **The write wire is binary FlatBuffers**, matching the read direction; the JSON
  control/action envelopes on wasm shrink to the lifecycle/handshake events that are
  genuinely control-plane.

## Implementation State

As of 2026-06-25, the production binding transport is settled:

1. Native app writes use `nmp_app_dispatch_action_bytes`; wasm app writes use
   `dispatch_bytes` / `handle_dispatch_bytes`. Both carry the same
   `DispatchEnvelope` bytes and decode through the same Rust envelope path.
2. Release builds expose the byte dispatch transport as the binding write path.
3. Generated host builders are the app-facing API. App-owned helper seams may
   still use canonical Rust serde bodies as an in-process construction detail
   before encoding the namespace's typed `ActionPayload`; that JSON must not
   cross the FFI/worker boundary as a runtime dispatch transport.
4. Per-module payload coverage can continue to expand, but that is schema
   coverage work inside the byte doorway, not a reason to split native and web
   transports.

Wallet **session lifecycle** (`WalletConnect` / `WalletDisconnect` /
`WalletPayInvoice`) is explicitly **out of scope**: it does not produce a signed
Nostr event and stays on its dedicated capability/session symbols (Theme A
discriminator).
