# Runtime, Capability, and Shell Boundary

> Doctrine home: ADR-0072, with ADR-0040, ADR-0048, ADR-0050, ADR-0066, and the runtime
> ownership split of ADR-0067/0068. For the binding-surface story (UniFFI vs wasm-bindgen,
> facades), see `ffi-and-native-surface.md`.

## The Three-Tier Runtime Stack

| Crate | Role | Owns |
|---|---|---|
| `nmp-native-runtime` | Native platform runtime adapter | `NmpApp`, `NmpAppBuilder` typestate, actor thread, native session registries |
| `nmp-uniffi` | Public native binding surface | UniFFI export of `nmp-native-runtime`; no policy, routing, or signing |
| `nmp-browser-runtime` | Browser runtime adapter | Worker, OPFS init, `wasm-bindgen` ABI (`::wasm`) |
| Owner-crate installers | Explicit composition surface | `AppHost` registration; NOT a runtime; no lifecycle handle |

`nmp-wasm` is deleted; `nmp-browser-runtime::wasm` is the sole browser ABI. Any composition
installer acquiring an actor-thread lifecycle or runtime handle is a violation.

## A Shell Does Exactly Three Things

1. Start or attach to the Rust-owned app runtime.
2. Open typed read sessions and dispatch typed actions.
3. Render emitted state and answer capability requests.

Anything a second platform would need to reimplement to stay correct belongs in Rust:
read-source expansion, relay choice, signer choice, tag/envelope mutation, publish retry,
cache truth, admission policy, privacy checks, product queue state, durable navigation
meaning, user-visible operation status.

## The Capability Contract

### Types
```rust
// crates/nmp-core/src/substrate/capability.rs
pub trait CapabilityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Request:  Clone + Serialize + DeserializeOwned + Send + 'static;
    type Result:   Clone + Serialize + DeserializeOwned + Send + 'static;
    fn callback_interface_name() -> &'static str;
}
pub struct CapabilityRequest  { namespace, correlation_id, payload_json }
pub struct CapabilityEnvelope { namespace, correlation_id, result_json }
```

### Registration
The UniFFI path (`CapabilitySink::on_capability_request(String) -> String`,
`crates/nmp-uniffi/src/capability/mod.rs`) is preferred; a transitional C-ABI path
(`CapabilityCallbackRegistration`, `crates/nmp-core/src/capability_socket.rs`) exists. Both
route through one `CapabilityCallbackGate` with `in_flight + Condvar` quiescence: after
`set_capability_callback` returns, the previous sink is neither registered nor mid-invocation.

### Execution (ADR-0040) — off-actor, serialized, FIFO
The capability worker (`crates/nmp-core/src/actor/capability_worker.rs`) drains a FIFO queue
with blocking `recv`, executes the native callback on its own thread, and re-enters the actor
via `ActorCommand::Identity(IdentityCommand::CapabilityResultReady)`. The actor never blocks
on capability work. A wedged operation reports timeout/error as data; it must not mutate kernel
state from the worker thread. `CapabilityResultReady` is a **genuine actor wake** (ADR-0050 D3a,
`actor/inbox.rs`): completion latency = mailbox latency. The 250 ms idle sweep is solely a
deadline-expiry gate, not how completions are noticed. Cold-start local Keychain reads known
not to involve biometric/UI waits may stay synchronous; blocking paths use the worker.

### Rules
- Native executes the OS primitive; Rust decides what it means.
- Native reports raw facts (`Loaded`, `Registered`, `Error { reason }`, `Rejected`); never
  retry, relay, cipher, or recoverability decisions.
- `Request`/`Result` carry **no app/protocol nouns** (no episode IDs, group IDs, event kinds,
  relay policy) — only OS-level primitives: URLs, paths, tokens, ciphertexts, pubkeys. This
  keeps capabilities reusable and keeps app nouns out of `nmp-core` (D0).
- Idempotent: start-after-start and stop-after-stop are no-ops; a 1000-cycle start/stop/restart
  leaves zero retained-by-cycle leaks; no straggler events after `Cancelled`.
- Failures cross as result *variants*, never thrown exceptions or `Result<T,E>` (D6).
- Native holds only OS handles — no queue, history, preferences, derived state, or policy
  beyond the OS mechanism selector.

**OS mechanism choice is execution, not policy.** When a bridge picks between OS transport
paths (e.g. NIP-55 Intent vs ContentResolver), the rule is a mechanical consequence of a field
in the Rust-built request (`granted_permissions`); the host checks the field and selects the
primitive — it does not decide retry, routing, or ciphertext interpretation (ADR-0048 D2).

## Signer Capability (special case)

An external signer implements `RemoteSignerHandle` (`crates/nmp-core/src/remote_signer.rs`),
not a bare capability, so it is backend-transparent at the sign port (V-78). Three verbs
(ADR-0050 D1): `sign`, `nip44_encrypt`, `nip44_decrypt`. Per-op deadline is
`RemoteSignerHandle::op_timeout() -> Duration` (NIP-46 = 5 s, NIP-55 = 90 s), computed from the
**named signing account**, not the active account (ADR-0050 D4). Signer backend identity is
invisible at the port; `nmp-core` never imports signer implementations (D0). Signer health is
one `signer_state` projection (`is_ready`, `is_awaiting_approval`, `is_unavailable`,
`is_failed`, `signer_kind`) — not per-backend projections. ADR-0066 reserves an optional NIP-46
batch decrypt session (`begin/batch/end`) that returns plaintext only; no key material crosses
the boundary.

## Headless and OS-Owned Surfaces (ADR-0072)

AppIntents, CarPlay, Live Activities, widgets, share extensions, and suspended-process resumes
must use typed actions, short-lived headless invocation, capability results, or last
Rust-emitted mirror frames. They must **not** own parallel playback queues, signer state, relay
policy, deep-link admission, or publish result models. App-lifetime typed sessions are allowed
only after a proof that resident state is required, using the same lifecycle contract as
visible screens.

## Browser Runtime Proof (ADR-0072)

Durable browser mode requires real Worker + OPFS init before product start. Missing Wasm,
missing Worker, OPFS failure, Web Locks contention, or unsupported signer capability must
produce typed degraded/failure state or fail the proof — silent in-memory fallback is not
product success. Gate: `crates/nmp-browser-runtime-conformance/src/lib.rs`.

## Blocking Violations

- Shell owns protocol parsing, relay policy, signer state, publish completion, retry loops,
  playback-queue truth, or deep-link admission.
- Capability bridge stores retry/relay/cipher/policy state beyond OS handles.
- Capability bridge returns `Result<T,E>` or throws across the callback boundary.
- Capability bridge is non-idempotent on repeated start/stop/restart.
- Capability `Request`/`Result` carries app/protocol nouns.
- Headless/OS surface owns product-queue state or signer handles.
- Browser durable mode claimed without real Worker/OPFS proof.
- Signer implementation bypasses `RemoteSignerHandle` for a sign verb (V-78).
- Per-op deadline uses the active account instead of the named signing account.
- A composition installer acquires actor-thread lifecycle or a runtime handle.

## Who Owns What

| Concern | Owner |
|---|---|
| Actor thread lifecycle | `nmp-native-runtime` |
| Native host binding | `nmp-uniffi` |
| Browser Worker + OPFS | `nmp-browser-runtime` |
| Generic NMP composition | App root calling `nmp_substrate::install(...)` plus owner-crate installers |
| OS capability execution | Native shell (capability bridge) |
| Capability result policy | Rust (actor dispatch arm) |
| Signer backend selection | Rust (`RemoteSignerHandle`) |
| OS transport mechanism (Intent vs ContentResolver) | Native shell (mechanical, not policy) |
| Retry, routing, cipher choice | Rust always |
| Headless surface product state | Rust (never the OS surface) |
