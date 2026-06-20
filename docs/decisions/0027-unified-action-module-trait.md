# ADR-0027 — Unify the `ActionModule` trait: collapse `register_action_module` + `register_action_executor`

- Status: Implemented. The dual seam is gone: `register_action_executor`
  (Rust and C-ABI) was deleted and `ActionModule::execute()`
  (`crates/nmp-core/src/substrate/action.rs:126`) is the sole dispatch path.
  Shipped piecemeal via PRs #227–#247; the action FFI now lives in
  `crates/nmp-ffi/src/action.rs`.
- Date: 2026-05-21
- Related: ADR-0010 (generated app enum vs type-erased registry),
  ADR-0025 (Marmot bespoke FFI cluster — named exception),
  ADR-0026 (signer NIP-44 seal seam),
  memory: `dual_action_seam_footgun.md`
- Doctrine: aim.md §6 doctrine #3 ("All writes through actions — no manually
  assembled build/sign/publish sequence") and #6 (auto-wired subscriptions —
  by analogy, auto-wired action registrations).

## Context — the dual seam

Before this ADR a kernel action namespace was wired through **two independent
registration calls**:

1. `NmpApp::register_action_module(namespace, validator)` — stored a closure
   that ran `ActionModule::start` (shape validation, no side effects).
2. `NmpApp::register_action_executor(namespace, executor)` — stored a *second*
   closure that re-parsed `action_json` and emitted an `ActorCommand` via the
   `send` callback.

The two closures shared nothing but a string. A host that wired only one half
got a runtime error (`"unknown action namespace"` from `start`, or `"no
executor registered for namespace 'X'"` from `execute`). The compile gate could
not catch the mismatch; the protection was documentation plus a runtime error
message.

The same shape was mirrored at the C-ABI boundary
(`nmp_app_register_action_executor` and `nmp_app_register_action_module`).

To paper over the foot-gun, the Chirp host crate carried a `wire_action!`
macro that took a single `$Action` type and a `$command` builder and emitted
both halves in lock-step. The macro worked — but only for code that used it. A
new NIP-crate written from scratch could still register one half and forget the
other; the macro was a convention, not an invariant.

This violates aim.md doctrine #3 at the *registration* seam: a developer
following the documented API can ship a broken Nostr application. "Impossible
to fuckup" requires the type system to refuse a partial registration.

## Decision

Extend the `ActionModule` trait (`crates/nmp-core/src/substrate/action.rs`)
with a single new required method that turns a validated action into an
`ActorCommand`:

```rust
pub trait ActionModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Action: Clone + Serialize + DeserializeOwned + Send + 'static;

    fn start(
        ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection>;

    /// Build and dispatch the `ActorCommand` for a validated action.
    ///
    /// Called by the registry after `start` accepts the action. `correlation_id`
    /// is the registry-minted handle the host received from `dispatch_action`;
    /// threading it onto an `ActorCommand` whose terminal verdict must report
    /// that id (e.g. `PublishNote` — the actor signs the event) keeps the
    /// host's spinner key consistent with `action_results`.
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String>;

    fn preferred_action_id(_action: &Self::Action) -> Option<ActionId> { None }
}
```

Then `NmpApp::register_action_module<M: ActionModule>()` (renamed and
generic) wires **both halves from one typed impl** — the `ActionModuleAdapter<M>`
gains an `execute` arm that decodes once into `M::Action` and forwards to
`M::execute`. `register_action_executor` is deleted.

### Why typed `Self::Action`, not raw `&str`

The trait could have taken `action_json: &str` (matching the old `ExecutorFn`
shape). It does not, for two reasons:

1. The adapter already parses `action_json` into `M::Action` once during
   `start`. Parsing it again in
   `execute` is wasted work — and, more importantly, lets the executor see a
   *different* `M::Action` than the validator did if a (hypothetical) future
   refactor splits the codepaths. A single decode at the adapter boundary
   makes validator-executor symmetry a type-level fact.
2. Doctrine #3's "impossible to fuckup" is stronger when the trait impl never
   touches raw JSON inside `execute`. The current `<verb>_command(&str)`
   helpers (`post_chat_message_command`, `send_dm_command`, …) all start with
   the same `serde_json::from_str` boilerplate; collapsing that into the
   adapter is a real simplification, not a re-shuffle.

The adapter's `ErasedActionModule::execute` owns the parse + dispatch, and the
`ActionRegistry::execute` callsite lost its `executors: HashMap` dependency
entirely.

## FFI implications

`nmp_app_register_action_executor` (now in `crates/nmp-ffi/src/action.rs`)
is deleted, along with its companion typedef `NmpActionExecutor`.
`register_action_module<M>` remains — but **only as a Rust-callable seam for
typed `ActionModule` impls**. There is no useful C-ABI shape for the unified
trait, because `Self::Action` and `ActorCommand` are Rust types with no stable
C representation.

The decision: **the unified `register_action_module<M>` is Rust-only.** A
non-Rust host that wants a custom action namespace registers a typed
`ActionModule` impl from a Rust shim crate it controls, or stays on the
existing built-in namespaces. This is consistent with how the Marmot cluster
is structured today (Rust-side composition root in `apps/chirp/nmp-app-chirp`)
and with ADR-0010's generated-app-enum direction.

The two C-ABI symbols (`nmp_app_register_action_executor`,
`nmp_app_register_action_module`) become un-needed at the same time. Both are
deleted; the `extern "C"` surface shrinks. Cross-reference to the D8 constraint
("no high-frequency FFI loops"): this refactor *reduces* FFI surface — the
dispatch path itself (`nmp_app_dispatch_action`) is untouched.

A grep for external consumers of `nmp_app_register_action_executor` found
zero Swift / Kotlin / Objective-C callers, so deletion was empirically safe.

## Alternatives considered

- **Keep the dual seam, add a lint check.** A `cargo`-level lint that asserts
  every namespace registered as a module has a matching executor (and vice
  versa). Rejected: a lint catches the mistake post-hoc; the type system
  refuses it up-front. The ADR's purpose is the upgrade from documentation
  to invariant.
- **One-method shape: `fn handle(...) -> Result<ActorCommand>`.** Collapse
  `start` and `execute` into a single function that both validates and
  returns the command. Rejected: it conflates validation (a pure function,
  used by `start_publish_*` test cases that don't drive the actor) with
  dispatch (a side-effecting send into the actor mailbox). Two methods, one
  registration is the right granularity — the type-erasing adapter is the
  *only* code that needs to know both exist.
- **Free-function executor + `ActionModule::EXECUTOR` const.** Rejected: a
  `const EXECUTOR: fn(...)` field can't reference `Self::Action` (Rust's
  associated-const ergonomics around generic types), and it's strictly less
  ergonomic than a method.

## Doctrine alignment

- **aim.md §6 doctrine #3** — "All writes through actions. No 'build event,
  sign, publish' sequence the developer assembles manually." Today's dual
  seam lets a developer assemble *half* of a write path and ship it. After
  this ADR, registering an action means implementing one trait — the
  framework provides the dispatch side automatically.
- **aim.md §6 doctrine #6** — auto-grouped, auto-closed subscriptions. The
  analogue at the action seam is auto-wired registration: one call, both
  halves. The doctrine's spirit (the developer never assembles the
  framework's plumbing) carries over.
- **No high-frequency FFI loops** — the change *removes* one C-ABI symbol
  pair (`nmp_app_register_action_executor`, `nmp_app_register_action_module`).
  Net FFI surface decreases; no new ABI is added.

## Out of scope

- The broader "consolidate all write paths" question — `publish_signed_event`
  vs. `dispatch_action` vs. the 36 `ActorCommand` variants that bypass the
  action seam (see explorer's Finding #1 / opus review #31). That is a
  separate, larger architectural conversation. **This ADR is scoped to the
  *registration* seam only**: validator + executor become one trait impl,
  one call.
- The Marmot bespoke FFI cluster (ADR-0025) is unchanged. Marmot's dormant
  `ActionModule` impls are out of scope; if they should be deleted, that is
  a follow-on ADR.
- The C-ABI surface for non-Rust hosts that want custom action namespaces.
  This ADR chooses Rust-only registration; if a future host needs a C-ABI
  path, that requires its own ADR specifying a stable serialization for
  `ActorCommand` (today there is none).
