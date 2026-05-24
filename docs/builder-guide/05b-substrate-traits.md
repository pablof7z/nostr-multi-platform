# 05b — Kernel substrate: fixture walkthrough + nip29 + composition

*Status: SHIPS · Audience: both · Read after [05a](05a-substrate-traits.md).*

[05a](05a-substrate-traits.md) gave you the five signatures and the "which
trait?" tree. This half is the proof the boundary works in both directions:
an annotated non-Nostr fixture that implements all five families, a sidebar
showing how a real Nostr protocol crate uses them, and how modules compose.

## Annotated walkthrough: `fixture-todo-core`

`apps/fixture/fixture-todo-core/src/lib.rs:13-265` is ADR-0009 acceptance criterion
1 made real: a module exercising **all five trait families with zero Nostr
concepts**. It is the canonical template — read it before writing any module.

```rust
// One app record. Not a Nostr event. (lib.rs:6-11)
pub struct TodoRecord { pub id: String, pub title: String, pub completed: bool }

// 1. DomainModule — durable store, app-local (no ingest_kinds). (lib.rs:13-37)
impl DomainModule for TodoDomainModule {
    const NAMESPACE: &'static str = "fixture.todo.domain";
    const SCHEMA_VERSION: u32 = 1;
    fn migrations() -> Vec<DomainMigration> { Vec::new() }   // v1: none yet
    fn indexes() -> Vec<DomainIndex> {                       // secondary index
        vec![DomainIndex { name: "by_completed",
            key_fn: |b| serde_json::from_slice::<TodoRecord>(b).ok()
                          .map(|t| t.completed.to_string().into_bytes()) }] }
    fn register(r: &mut DomainRegistry) { r.register_record::<TodoRecord>(); }
}
```

Note: no `ingest_kinds()` override → it inherits `&[]` → pure app-local
store, no relay traffic. The `by_completed` index lets the kernel answer
"open todos" without scanning every record.

```rust
// 2. ViewModule — reactive projection of the todo list. (lib.rs:62-127)
impl ViewModule for TodoViewModule {
    type Spec = TodoListSpec;     // { include_completed: bool }
    type Payload = TodoListView;  // { items, open_count }
    type Delta = TodoDelta;       // Replaced { payload }
    type Key = bool;              // dedupe by include_completed
    type State = TodoViewState;
    fn key(s: &Self::Spec) -> bool { s.include_completed }
    fn dependencies(_s) -> ViewDependencies { ViewDependencies::default() }
    fn open(_c, _s) -> (State, Payload) { /* empty list seed */ }
    // event callbacks return None (todos are domain-driven, not event-driven)
    fn on_projection_changed(_c, st, _ch) -> Option<TodoDelta> {
        Some(TodoDelta::Replaced { payload: st.payload.clone() }) }
    fn snapshot(_c, st) -> TodoListView { st.payload.clone() }
}
```

This is the key teaching point: the todo view reacts to
`on_projection_changed` (a `DomainModule` write), **not** to `on_event_*`. A
non-Nostr view legitimately returns `None` from every Nostr-event callback.
(Its empty `dependencies()` is acceptable *only* because it has no Nostr deps;
a Nostr view with empty deps is the anti-pattern below.)

```rust
// 3. ActionModule — local writes with synchronous validation. (lib.rs:146-178)
impl ActionModule for TodoActionModule {
    type Action = Action;   // Add{id,title} | Toggle{id} | ClearCompleted
    fn start(_c, a) -> Result<ActionPlan<TodoStep>, ActionRejection> {
        if matches!(&a, Action::Add{title,..} if title.trim().is_empty()) {
            return Err(ActionRejection::Invalid("todo title is empty".into())); }
        Ok(ActionPlan { initial_step: TodoStep::ApplyLocalWrite,
                         initial_status: ActionStatus::Running, deadline_ms: None }) }
    fn reduce(_c,_id,_in) -> ActionTransition<_,_> {
        ActionTransition::Complete { output: ActionOutput::Accepted } }   // 1-step
}
```

`start` rejects bad input *synchronously* (`ActionRejection::Invalid`) — that
is validation, not a workflow failure. `reduce` completes in one step because
a local write has no relay/capability round-trip.

```rust
// 4. CapabilityModule — typed request/result, no logic. (lib.rs:190-201)
impl CapabilityModule for TodoCapabilityModule {
    type Request = CapabilityCall;     // CountOpenTodos
    type Result  = CapabilityResult;   // { count }
    fn callback_interface_name() -> &'static str { "FixtureTodoCapability" }
}
// 5. IdentityModule — an app-local scope that cannot sign Nostr. (lib.rs:210-244)
impl IdentityModule for TodoIdentityModule {
    type Descriptor = TodoIdentityDescriptor;          // { label }
    fn scope_kind() -> IdentityScopeKind { IdentityScopeKind::AppLocal }
    fn create(ctx, d) -> Result<IdentityId, IdentityError> {
        if d.label.trim().is_empty() {
            return Err(IdentityError::InvalidDescriptor("empty label".into())); }
        let id = format!("fixture-todo:{}", d.label); ctx.remember(id.clone()); Ok(id) }
    fn sign(..) -> BoxFuture<Result<SignedEvent, SigningError>> {
        Box::pin(async { Err(SigningError::Unsupported(
            "fixture identity does not sign Nostr events".into())) }) }
    fn destroy(_c,_id) {}
}
```

`scope_kind() == AppLocal` and `sign` deliberately returns
`SigningError::Unsupported` — proving an identity scope need not be a Nostr
key. The contract test `fixture_registers_all_extension_families`
(`lib.rs:271-286`) asserts exactly five descriptors register.

## Sidebar: how `nmp-nip29` uses all 5 families

`crates/nmp-nip29/src/lib.rs:1-57` is the Nostr-shaped counterpart — the
proof the same five traits scale to a real protocol with **zero new nouns in
`nmp-core`** (`crates/nmp-nip29/src/lib.rs:11-19`).

| Family | nip29 population | Notes |
|---|---|---|
| `DomainModule` | **13** (`domain/mod.rs:55-67`) | `GroupModule`, `GroupMembersModule`, … each overrides `ingest_kinds()` to claim a NIP-29 kind |
| `ViewModule` | **7** (`view/mod.rs:43-49`) | `JoinedGroupsView`, `GroupChatView`, … declare composite deps on `h`-tag refs |
| `ActionModule` | **15** (`action/mod.rs:47-61`) | `CreateGroupAction`, `PostChatMessageAction`, … typed `GroupId` flows to publish |
| `CapabilityModule` | 0 | groups need no OS handle |
| `IdentityModule` | 0 | groups reuse the active account |

The crate-boundary statement (`lib.rs:10-19`) is the doctrine in code:
*`nmp-nip29` does NOT import any other `nmp-nip*` crate; cross-protocol
composition happens at the app layer; the only generic surface added in
`nmp-core` is the third routing lane (`relay_pin` + lattice Rule 9).* Compare
the `register` fn at `lib.rs:50-54` with the fixture's `module_descriptors()`
— same `ModuleRegistry` API, 35 modules vs 5. (Full protocol-module recipe:
[20](20-new-protocol-module.md).) The app-shaped third case is any product
crate you add later: its D0 banner should say that app nouns live in app
modules, never in `nmp-core`.

## ModuleRegistry composition

Every module registers through one API
(`crates/nmp-core/src/substrate/mod.rs:19-79`). `ModuleDescriptor` carries
`{ namespace, family, rust_type }`; `ModuleRegistry::push` is idempotent per
`(namespace, family)` — double-registration is a silent no-op, not a panic.

```rust
// fixture-todo-core (lib.rs:257-265) — one app crate, 5 families:
pub fn module_descriptors() -> ModuleRegistry {
    let mut r = ModuleRegistry::default();
    r.register_domain::<TodoDomainModule>();      r.register_view::<TodoViewModule>();
    r.register_action::<TodoActionModule>();      r.register_capability::<TodoCapabilityModule>();
    r.register_identity::<TodoIdentityModule>();  r
}
// nmp-nip29 (lib.rs:50-54) — composed into the SAME registry by codegen:
pub fn register(registry: &mut ModuleRegistry) {
    domain::register_all(registry); view::register_all(registry);
    action::register_all(registry);
}
```

Per-app generated code (`nmp-codegen`, [15](15-codegen-and-ffi.md)) calls each
crate's register fn into one shared `ModuleRegistry` at startup. The kernel
then knows the trait-family populations without naming any module's concrete
types — D0 holds end to end.

## Anti-patterns

1. **Business policy in a `CapabilityModule` (D7 violation).** A capability
   reports a fact (`CountOpenTodos → { count }`). It must not decide retry,
   routing, or "should we publish." Policy lives in `ActionModule`.
2. **Long-lived state in `IdentityContext`.** It tracks created ids only
   (`identity.rs:34-47`). Stashing session/account state there duplicates the
   single writer (D4) and leaks across scopes.
3. **`ViewModule` with empty `dependencies()` for a Nostr view.** The fixture
   gets away with `ViewDependencies::default()` only because it is
   domain-driven. A Nostr view with empty deps forces a full table scan on
   every insert — declare tight composite keys ([06](06-reactivity-contract.md)).
4. **Skipping migrations in a `DomainModule`.** Bumping `SCHEMA_VERSION`
   without a `DomainMigration` from the prior version corrupts stored records
   on the next launch. Every version bump ships a migration.
5. **Putting Nostr nouns in `nmp-core` substrate.** `nmp-nip29` adds 35
   modules and *zero* group nouns to `nmp-core`. If your module needs a new
   `nmp-core` type, it is almost always wrong (the rare exception — a generic
   seam like `relay_pin` — needs its own ADR; see [20](20-new-protocol-module.md)).

## Deliverables (this half)

- **Annotated `fixture-todo-core` walkthrough** (above) — the copyable
  five-family template.
- **`nmp-nip29` sidebar table** (above) — how the same traits scale to a
  real protocol with zero kernel nouns; plus the `ModuleRegistry`
  composition pattern shared by app and protocol crates.

See also: [02 — Mental model](02-mental-model.md) ·
[05a — Substrate traits: signatures + decision tree](05a-substrate-traits.md) ·
[06 — Reactivity contract (D8)](06-reactivity-contract.md) ·
[16 — Capabilities (D7)](16-capabilities.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
