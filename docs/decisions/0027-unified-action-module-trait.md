# ADR-0027 — Unify the `ActionModule` trait: collapse `register_action_module` + `register_action_executor`

- Status: Proposed
- Date: 2026-05-21
- Related: ADR-0010 (generated app enum vs type-erased registry),
  ADR-0025 (Marmot bespoke FFI cluster — named exception),
  ADR-0026 (signer NIP-44 seal seam),
  memory: `dual_action_seam_footgun.md`
- Doctrine: aim.md §6 doctrine #3 ("All writes through actions — no manually
  assembled build/sign/publish sequence") and #6 (auto-wired subscriptions —
  by analogy, auto-wired action registrations).
- Scope: this ADR is a **design proposal**, not an implementation. No code
  changes ship with it.

## Context — the dual seam

A kernel action namespace today is wired through **two independent
registration calls**:

1. `NmpApp::register_action_module(namespace, validator)` —
   `crates/nmp-core/src/ffi/mod.rs:634`. Stores a closure that runs
   `ActionModule::start` (shape validation, no side effects).
2. `NmpApp::register_action_executor(namespace, executor)` —
   `crates/nmp-core/src/ffi/mod.rs:607`. Stores a *second* closure that
   re-parses `action_json` and emits an `ActorCommand` via the `send`
   callback.

The two closures share nothing but a string. A host that wires only one half
gets a runtime error (`"unknown action namespace"` from `start`, or `"no
executor registered for namespace 'X'"` from `execute` —
`crates/nmp-core/src/kernel/action_registry.rs:285`). The compile gate cannot
catch the mismatch; the protection is documentation plus a runtime error
message.

The same shape is mirrored at the C-ABI boundary:
`nmp_app_register_action_executor` (`ffi/action.rs:139`) and
`nmp_app_register_action_module` (`ffi/action.rs:228`). Both declared in
`ios/Chirp/Chirp/Bridge/NmpCore.h:233` and `:235`.

To paper over the foot-gun, the Chirp host crate carries the `wire_action!`
macro (`apps/chirp/nmp-app-chirp/src/ffi.rs:67-88`) that takes a single
`$Action` type and a `$command` builder and emits both halves in lock-step.
The macro works — but only for code that uses it. A new NIP-crate written from
scratch can still register one half and forget the other; the macro is a
convention, not an invariant.

This violates aim.md doctrine #3 at the *registration* seam: a developer
following the documented API can ship a broken Nostr application. "Impossible
to fuckup" requires the type system to refuse a partial registration.

## Decision

Extend the `ActionModule` trait (`crates/nmp-core/src/substrate/action.rs:10`)
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
generic) wires **both halves from one typed impl** — the adapter
(`ActionModuleAdapter<M>` at `kernel/action_registry.rs:73`) gains an
`execute` arm that decodes once into `M::Action` and forwards to `M::execute`.
`register_action_executor` is deleted.

### Why typed `Self::Action`, not raw `&str`

The trait could have taken `action_json: &str` (matching the current
`ExecutorFn` shape at `kernel/action_registry.rs:108`). It does not, for two
reasons:

1. The adapter already parses `action_json` into `M::Action` once during
   `start` (`ActionModuleAdapter::start` at `:87`). Parsing it again in
   `execute` is wasted work — and, more importantly, lets the executor see a
   *different* `M::Action` than the validator did if a (hypothetical) future
   refactor splits the codepaths. A single decode at the adapter boundary
   makes validator-executor symmetry a type-level fact.
2. Doctrine #3's "impossible to fuckup" is stronger when the trait impl never
   touches raw JSON inside `execute`. The current `<verb>_command(&str)`
   helpers (`post_chat_message_command`, `send_dm_command`, …) all start with
   the same `serde_json::from_str` boilerplate; collapsing that into the
   adapter is a real simplification, not a re-shuffle.

The adapter's `ErasedActionModule::start` returns the preferred-id today; the
new `ErasedActionModule::execute` will own the parse + dispatch. The
`ActionRegistry::execute` callsite (`kernel/action_registry.rs:297`) loses its
`executors: HashMap` dependency entirely.

## Migration plan

Every live `ActionModule` impl gets an `execute` method. The body is mostly a
move of the corresponding `<verb>_command` function — already a typed
`(action_json: &str) -> Result<ActorCommand, String>` — but reading
`action: Self::Action` directly.

Files, verified against the code at commit `a1dcc568`:

- **`crates/nmp-core/src/publish/action.rs:108`** — `PublishModule`. There is
  **no** `publish_command` helper; the executor is an inline closure inside
  `default_registry` at `crates/nmp-core/src/kernel/action_registry.rs:401-471`
  that fans out on `PublishAction::{Publish, PublishNote, PublishProfile}` to
  three `ActorCommand` variants (`PublishSignedEvent`, `PublishNote`,
  `PublishProfile`). Move that match into `PublishModule::execute`; the closure
  registration disappears.
- **`crates/nmp-nip17/src/action.rs:50`** — `SendDmAction`. Move the body of
  `send_dm_command` (`:81`) into `execute`; delete the free function.
- **`crates/nmp-nip17/src/dm_relay_list.rs:160`** — `PublishDmRelayListAction`.
  Move `publish_dm_relay_list_command` (`:200`) into `execute`.
- **`crates/nmp-nip29/src/action/content.rs:49`** — `PostChatMessageAction`.
  Move `post_chat_message_command` (`:42`) into `execute`.
- **`crates/nmp-nip29/src/action/composed.rs:46, :92`** — `ReactInGroupAction`,
  `CommentInGroupAction`. Move `react_in_group_command` (`:39`) and
  `comment_in_group_command` (`:85`) into the two `execute` methods.
- **`apps/chirp/nmp-app-chirp/src/ffi.rs:465-507`** — `nmp.nip25.react`,
  `nmp.follow`, `nmp.unfollow` are registered today as **inline
  anonymous closures**, not as typed `ActionModule` impls. Promote each to a
  typed impl: introduce `pub struct ReactModule;`, `FollowModule;`,
  `UnfollowModule;` wrapping the already-existing `ReactAction`
  (`ffi.rs:585`) and `PubkeyAction` (`:597`) input types. Drop the inline
  registration; call `register_action_module::<ReactModule>()` etc.
- **`apps/fixture/fixture-todo-core/src/lib.rs:199`** — `TodoActionModule`. The
  fixture's executor lives in the codegen-driven app shell; the trait's
  `execute` body is a non-trivial migration target (it constructs the
  fixture's action update enum). Implement `execute` with the existing logic;
  no separate `_command` helper exists today.
- **`apps/chirp/nmp-app-chirp/src/ffi.rs:67-88`** — `wire_action!` macro. After
  the trait change, three lines collapse to one: every `wire_action!(app,
  Action, Input, command)` call becomes
  `app.register_action_module::<Action>()`. The macro can be deleted entirely.

**Excluded from migration:**

- **`crates/nmp-marmot/src/action/actions.rs`** (historical) — `CreateGroupAction`,
  `InviteMemberAction`, `SendMessageAction`, `LeaveGroupAction`,
  `RemoveMemberAction`, `UpdateKeysAction`, `PublishKeyPackageAction`. These
  were never registered against any registry; the only references outside
  the crate were `tests.rs`. Per **ADR-0025** the Marmot path is a named,
  bounded exception that uses the bespoke `nmp_marmot_dispatch`
  envelope — not the generic `dispatch_action` seam. The 6 group-scoped
  impls were deleted in PR #200; `PublishKeyPackageAction` and the entire
  `crates/nmp-marmot/src/action/` directory were deleted shortly after under
  anti-dormant policy (zero `register_action_module` callers; the iOS user
  flow goes through `MarmotBridge.publishKeyPackage()` → bespoke dispatch).
  Re-add an `ActionModule` impl only when a non-bespoke caller needs to drive
  a stateless Marmot capability through `dispatch_action` per ADR-0025
  Constraint #1.
- **NIP-77 `RunSync`** (task brief, `crates/nmp-nip77/src/run_sync.rs:46`).
  The file does not exist; `crates/nmp-nip77/src/` has `reconciler.rs`,
  `planner_gate.rs`, etc., but no `run_sync.rs` and no `RunSync` symbol. The
  memory pointer is stale — excluded.

## FFI implications

`nmp_app_register_action_executor` (`crates/nmp-core/src/ffi/action.rs:139`)
is deleted. So is its companion typedef `NmpActionExecutor` (`:104`).
`nmp_app_register_action_module` (`:228`) remains — but **only as a
Rust-callable seam for typed `ActionModule` impls**. There is no useful
C-ABI shape for the unified trait, because `Self::Action` and `ActorCommand`
are Rust types with no stable C representation.

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

A grep for external consumers of `nmp_app_register_action_executor` finds
zero Swift / Kotlin / Objective-C callers — only the C declaration in
`ios/Chirp/Chirp/Bridge/NmpCore.h:233`. Deletion is empirically safe.

## Risk + rollback

**External consumers.** Grepping `*.swift`, `*.kt`, `*.h`, `*.c`, `*.m`
across the repo for `nmp_app_register_action_executor` returns three hits:
two documentation comments and one C declaration in `NmpCore.h`. No code
caller exists. An out-of-tree consumer would link-fail after the symbol is
deleted — but this repo has no such consumer, and the project's "single
composition root" doctrine (one binary, one app crate) makes one improbable.

**Symbol deletion strategy.** Single PR. A staged rollout (deprecate first,
delete next cycle) would carry the dual seam through one more release for no
gain — no production consumer is in flight, and the dormant-feature
moratorium named in opus direction reviews #33–#40 argues against keeping
shipped-but-inert registration paths. The PR lands the trait change, the
adapter update, the migrations listed above, and the FFI symbol deletion
together; CI proves no in-tree consumer regressed.

**What if the assumption is wrong.** If a downstream that this repo cannot
see does link the symbol, the rollback is a `git revert` of the migration
PR — the FFI symbol bodies are short, the trait change is mechanical, and no
data shape changes (the wire JSON for every namespace is identical before
and after).

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
