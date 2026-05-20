# Opus Direction Review #5 — The Actions Layer, Concretely

2026-05-20. Reviews 1-4 covered relay transport, per-app projection FFI, operational
liveness, and the FFI contract. This one designs the actions layer.

## Correction up front: the trait already exists; the wiring does not

Review #2 said the actions layer is "missing." That is half wrong. `crates/nmp-core/
src/substrate/action.rs` already defines `trait ActionModule` (a two-method
`start`/`reduce` state machine), `PublishModule` impls it, and `nmp-nip29` ships 15
impls. What is missing is the *runtime*: nothing dispatches into `ActionModule::start`.
`publish/action.rs` says so plainly — "Wiring of start/reduce into the actor mailbox
lands with the kernel action ledger (M6)." So the real gap is two seams: (a) the
actor-side action ledger that drives `start`/`reduce`, and (b) the FFI entry point.
Designing a *parallel* `dispatch_action` would be a mistake — extend `ActionModule`.

Second correction: "50+ bespoke C symbols" is loose. Chirp's `ffi.rs` is 4 symbols,
and they are a *projection* (read path), not actions. The write-path mass lives in
`crates/nmp-core/src/ffi/identity.rs` — `nmp_app_publish_note`, `nmp_app_react`,
`nmp_app_follow`, `nmp_app_create_new_account`, `nmp_app_add_relay`, etc. *Those* are
what an FFI action entry point collapses.

## 1. The FFI entry point

The capability analogy is structural only: `dispatch_capability` is Rust→Native
(sync round-trip). An action is Native→Rust (enqueue, async result). Signature:

```rust
#[no_mangle]
pub extern "C" fn nmp_app_dispatch_action(
    app: *mut NmpApp,
    namespace: *const c_char,   // "nmp.publish"
    action_json: *const c_char, // serialized ActionModule::Action
) -> *mut c_char;               // returns {"correlation_id":"..."} or error envelope
```

Payload schema is `ActionModule::Action` itself. For publish — wrapping today's
`publish_note` — the action is an `nmp-nip01` type, not `PublishAction` (which takes
a pre-signed event; nip01 owns Build→Sign):

```json
{"t":"PublishNote","content":"gm","reply_to":null}
{"t":"React","target_event_id":"<64hex>","reaction":"+"}
```

The FFI layer never names these types — it forwards `(namespace, json)` to a registry.

## 2. Registration — composition, not a central enum

A static enum in `nmp-core` would reintroduce the god-struct problem and break D0.
Use per-crate registration at `NmpApp` construction:

```rust
// nmp-core
pub struct ActionRegistry { table: HashMap<&'static str, Box<dyn ErasedActionModule>> }
trait ErasedActionModule: Send + Sync {                    // dyn-safe wrapper
    fn start(&self, ctx: &mut ActionContext, json: &str)
        -> Result<ActionPlan<serde_json::Value>, ActionRejection>;
    fn reduce(&self, ctx: &mut ActionContext, id: &str,
              input: ActionInput<serde_json::Value>) -> ErasedTransition;
}
impl<M: ActionModule> ErasedActionModule for ActionModuleAdapter<M> { /* serde at edge */ }

// host wiring crate (apps/chirp/nmp-app-chirp)
nmp_nip01::install_actions(&mut registry);   // PublishNote, React, Follow
nmp_nip29::install_actions(&mut registry);   // the 15 group actions
```

`ActionModule` stays statically typed; only the *registry* erases via serde at the
boundary. The dispatch table is built by composition — adding a NIP adds a crate, not
a match arm. D0 holds: `nmp-core` never names `nmp-nip01`.

## 3. Response correlation — the centerpiece

Review #4 flagged the absence of command↔update correlation. For actions this is
make-or-break. Three options: (a) fire-and-forget, observe snapshot delta — loses
per-action error attribution; (b) return a `correlation_id` synchronously, deliver
`{"t":"action_result","v":{"correlation_id","status","output"}}` later on the
existing update channel; (c) sync block — violates D8. **(b) is the only choice
consistent with the no-polling doctrine and review #4.** `nmp_app_dispatch_action`
returns the id immediately; `ActionStatus` transitions (`Pending`→`Running`→
`Completed`/`Failed`) ride the update channel as a new `UpdateEnvelope` variant.
This *is* the action ledger M6 already scopes — `ActionInput::RelayOk`/`Timeout` are
the inputs `reduce` consumes. The offline queue (aim.md OQ-6, PD-024) becomes a
`Vec<(ActionId, namespace, json)>` persisted next to `FsPublishStore` and replayed
via `ResumedAfterRestart` — the `ActionInput` variant for exactly this already exists.

## 4. Migration — incremental, not a flag day

The bespoke symbols and `dispatch_action` coexist. `nmp_app_publish_note` becomes a
3-line shim that builds the nip01 action and calls `dispatch_action` internally.
Migrate kind-by-kind (kind-wrappers.md §8 phasing): note/react/follow first, then
relay edits, then identity. Delete a bespoke symbol only when no shell links it.
Atomicity is *already delivered* by the single-actor model — `publish_note` signs +
publishes + mutates kernel state on one thread today. What `ActionModule` adds is
composition (nip29's `ReactInGroupAction` runs sub-actions), per-crate extensibility,
durable retry, and the FFI collapse — not atomicity.

## 5. Is the registry a second god-module?

Real risk, but bounded by three rules. (a) The registry is a `HashMap`, not an enum —
no file grows when a NIP is added. (b) `start`/`reduce` must be *pure* over
`ActionContext` (no kernel handle) — an action that needs kernel state requests it via
`AwaitCapability`, keeping logic in the owning crate. (c) Enforce a LOC budget per
`action/` module (nip29 already splits into `admin`/`membership`/`content`/
`composed`). The god-module failure mode is a central `match`; composition-registration
structurally prevents it. The residual risk is the *registry wiring crate* accumulating
`install_actions` calls — acceptable: that crate is meant to be the assembly point.

**Recommendation:** land the action ledger (M6) + `nmp_app_dispatch_action` + the
erased registry as one milestone; migrate `publish_note`/`react`/`follow` behind it;
keep bespoke symbols as shims until shells stop linking them.
