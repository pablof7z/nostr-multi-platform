# ADR-0064 — Unified write/command boundary: one typed byte transport, open per-crate FlatBuffers payloads, signing as a capability round-trip

- **Status:** Accepted pending implementation (owner-decided 2026-06-21; staged migration).
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
    the seam ADR-0027 left "structurally vestigial" (≈70 event-producing C symbols,
    only 3 routed through `dispatch_action`).
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

---

## Context

### The write path has three divergent shapes for one concept

1. **Native FFI** reaches the `ActionModule` registry through a generic JSON
   doorway: `nmp_app_dispatch_action(namespace, action_json)`, where `namespace`
   is a string key (`"nmp.publish"`, `"nmp.nip25.react"`, `"nmp.follow"`) and
   `action_json` is the serde-JSON of that action's Rust enum. This is the
   doctrinally-correct *open registry*: a NIP crate self-registers an
   `ActionModule` (`const NAMESPACE`, typed `Action`, `start()` validator,
   `execute()`), and adding an action needs zero new C symbols (no doorway bypass).

2. **The wasm worker** carries that same generic envelope
   (`WorkerRequest::Dispatch { action_type, payload, correlation_id }`) **plus** a
   redundant, hand-rolled second vocabulary `WorkerRequest::AppAction(AppAction)`
   (`{ PublishNote | React | Follow | Unfollow }`) whose only job is
   `into_dispatch_parts()` — converting back into the same `(namespace, payload)`
   envelope. This is the **only** place in the framework that hand-rolls a
   second, platform-specific typed write vocabulary, and it is the per-platform
   hand-decoder divergence the shell-layer audits repeatedly flag.

3. **Signing is modeled twice.** Native signing is the ADR-0050 capability port
   (`sign` parks a `PendingSign`, the completion is mailbox-delivered, local keys
   resolve inline). The wasm path instead installs a persistent `Arc<dyn Signer>`
   (`SetSigner`) whose `.sign(unsigned).await` is called *from inside* the publish
   flow via a Promise entrypoint (`dispatch_app_action_async`). That is a
   non-replayable side-effect awaited inside the actor — at odds with D7/D8 and a
   second implementation of a problem ADR-0050 already solved.

### Why the string namespace is not the problem (and where the real fix is)

The owner's discomfort — *"why is the action's identity a string literal? why not
a typed `nmp_publish()`?"* — is correct only insofar as **a human should never
write that string**. The string is the extension key of an *open* registry over a
*frozen* C-ABI; that openness is load-bearing (D0: the kernel never learns the noun
"react"). The defect is that app and host code today *do* hand-assemble
`{action_type, payload}` JSON. The fix is not to close the command set into a
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

There is exactly one event-producing write doorway on each boundary. The JSON
`dispatch_action(namespace, action_json)` envelope and the wasm
`WorkerRequest::Dispatch { action_type, payload }` envelope are migrated onto it
(staged, §Migration). `WorkerRequest::AppAction` / the `"app_action"` wire tag /
`AppAction::into_dispatch_parts` are **deleted outright** — no alias, no
compatibility shim.

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
- no hand-written wasm action enum (the `AppAction` regression backstop);
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
- **Signing is solved once.** The wasm `Arc<dyn Signer>.await` path,
  `dispatch_app_action_async`, and `SetSigner`-as-install are retired; NIP-07 joins
  the ADR-0050 port as a fulfiller. Web and native share one replayable flow.
- **Known defects are fixed by construction:** the `PublishSignedEvent`
  validation-bypass, the pre-signed-publish event-id-as-correlation-id spinner bug,
  and the `app_action` divergence all close in the migration rather than persisting.
- **The write wire is binary FlatBuffers**, matching the read direction; the JSON
  control/action envelopes on wasm shrink to the lifecycle/handshake events that are
  genuinely control-plane.

## Migration (staged — per ADR-0027, not big-bang)

1. Add the FlatBuffers `DispatchEnvelope` + byte doorway
   (`nmp_app_dispatch_action_bytes` / `DispatchBytes`) **alongside** the current
   JSON dispatch.
2. Teach the `ActionModule` registry to decode typed FlatBuffers payloads; give the
   already-registered modules (`nmp.publish`, `nmp.nip25.*`, `nmp.follow/unfollow`)
   a FlatBuffers payload schema.
3. **Cut A** (issue #1743, in-repo only): Generate typed host builders for those
   modules; move wasm callers onto them and **delete `AppAction` / `"app_action"` /
   `into_dispatch_parts`**. No external consumers depend on this tag; it can land
   without lockstep coordination.
4. Route signing through the ADR-0050 capability port on wasm: introduce the
   typed Sign request/response across the worker boundary, route **local-key**
   signing through it first (the inline fulfiller), then retire
   `dispatch_app_action_async` + `SetSigner`-as-install.
5. Move NIP-07 (and confirm NIP-46/NIP-55) onto the same capability completion path.
6. Migrate the remaining action payloads JSON → FlatBuffers per crate; fix the
   `correlation_id`/event-id binding and the `PublishSignedEvent` validation bypass
   in the same step.
7. **Cut B** (issue #1756, lockstep with external consumers): Collapse the
   event-producing C-symbol bypasses (`nmp_app_publish_signed_event`,
   `nmp_app_publish_unsigned_event`, the direct `send_cmd` writers) into the one
   doorway and remove them. Requires coordinated update of all external native
   callers before deletion.
8. Land the §6 drift gates.

Wallet **session lifecycle** (`WalletConnect` / `WalletDisconnect` /
`WalletPayInvoice`) is explicitly **out of scope**: it does not produce a signed
Nostr event and stays on its dedicated capability/session symbols (Theme A
discriminator).
