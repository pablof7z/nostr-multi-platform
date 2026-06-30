# 02 — Mental model: kernel + extension seams

*Status: SHIPS · Audience: both · Read after [01](01-what-nmp-is.md).*

If you remember one thing: **NMP is a Nostr-native app kernel with first-class
extension modules — not a framework with closed built-ins.** The kernel knows
*how* to run a reactive Nostr client. It does not know *what* a Profile, an
Episode, a Highlight, or a TODO is. Those nouns live in modules you write.

This section gives you the four-layer stack, the production app surfaces, the
internal extension machinery, the
no-app-nouns-in-kernel rule, what crosses FFI, and a concrete "where does X
live?" map. It is the map for the whole guide.

## The 4-layer stack

Four layers, strict ownership. Built from the bottom up:

```
┌──────────────────────────────────────────────────────────────────────┐
│ PLATFORM SHELL          apps/chirp/ios + Android Chirp/gallery shells       │
│  owns: rendering, OS handle execution, generated binding wrappers      │
│  D5 ► consumes ONE bounded FlatBuffers update frame; no policy nouns     │
└────────────────────────────────▲───────────────────────────────────────┘
                                  │ FlatBuffers payload; UniFFI native / wasm-bindgen browser bindings
┌────────────────────────────────┴───────────────────────────────────────┐
│ RUNTIME + COMPOSITION   nmp-native-runtime + binding adapters            │
│                         app core + explicit NMP installers               │
│  owns: lifecycle/action/capability binding + explicit Rust composition   │
│  D6 ► no Result<T,E> crosses here; envelopes only                        │
└────────────────────────────────▲───────────────────────────────────────┘
                                  │ `NmpApp` seams + `AppHost` traits
        ┌─────────────────────────┼──────────────────────────┐
┌───────┴──────────┐  ┌───────────┴───────────┐  ┌────────────┴─────────┐
│ APP CORE CRATES   │  │ NMP PROTOCOL MODULES   │  │  (more app cores)    │
│ apps/chirp/        │  │ nmp-nip29 (groups)     │  │ microblog-core       │
│  microblog-core    │  │ nmp-nip42 (auth)       │  │  (walkthrough app)   │
│  nmp-app-chirp     │  │ nmp-nip77 (sync)       │  │                      │
│ D0 ► MAY hold app  │  │ nmp-signers (identity) │  │ D0 ► app nouns OK    │
│      nouns         │  │ D0 ► protocol nouns ONLY│  │                     │
└───────┬──────────┘  └───────────┬───────────┘  └────────────┬─────────┘
        └─────────────────────────┼──────────────────────────┘
┌────────────────────────────────┴───────────────────────────────────────┐
│ nmp-core KERNEL    actor · EventStore · planner · subs · publish        │
│                    + 2 extension traits + 3 registration seams          │
│  D0 ► ZERO app nouns. ZERO protocol nouns. Generic infrastructure only. │
│  D4 ► one writer per fact (the actor) — never the platform              │
└──────────────────────────────────────────────────────────────────────────┘
```

Representative crates are labelled in their layer above:
`nmp-core` (kernel), `nmp-nip29` / `nmp-nip42` / `nmp-nip77` / `nmp-signers`
(protocol modules), `apps/chirp/crates/nmp-app-chirp` + `microblog-core` (app cores),
`nmp-substrate` (shared substrate floor), `nmp-native-runtime` (native
runtime owner), and binding crates/adapters (UniFFI for native,
wasm-bindgen for browser).
`nmp-codegen` still emits host bindings (`gen swift`, `gen typed-decoders`);
it no longer generates per-app composition crates (ADR-0046). Chirp is the
active product shell.

### Doctrine callouts on the diagram

- **D0 (kernel/extension boundary).** The dividing line *is* this section.
  `nmp-core` provides generic infrastructure only — actor runtime, verified
  event store, planner, publish pipeline, signer plumbing, the extension
  seams. It contains **no** `Profile`/`Timeline`/`Episode`/`Highlight`/
  `Project` types. The rule: *if shipping your app requires adding a domain
  noun to `nmp-core`, the boundary is wrong and the kernel changes — never
  the app.*
- **D4 (single writer per fact).** Exactly one component owns each fact. The
  actor inside the kernel is that writer. The platform shell never mutates
  state; it renders snapshots and dispatches actions.
- **D5 (snapshots bounded by what's open).** What crosses up to the shell is
  one bounded update payload scoped to currently-open views — not the whole
  store. The runtime payload format is FlatBuffers; the shell holds no
  source-of-truth state.

## Production app surfaces and internal seams

Extension crates plug into a vanilla `NmpApp` through a small set of Rust seams,
but not every seam is app-facing vocabulary. A crate uses only the seams it owns
and never reaches into kernel internals. Production app screens should see typed
read sessions and typed action builders; raw observer/projection plumbing is
executor machinery behind those sessions.

### Seam 1 — `register_action(module)`

```rust
app.register_action(MyActionModule);
```

Registers an `ActionModule`: its `start()` validates dispatched actions;
its `execute()` enqueues `ActorCommand`s into the actor. The registered module
receives every typed dispatch envelope whose namespace matches
`MyActionModule::NAMESPACE`.

### Production read surface — typed sessions/helpers

```rust
register_microblog_read_session(app, Arc::clone(&store));
```

A typed read session, or a generated helper over one, is the production read
model. It owns demand, replay-before-live, admission, typed output, status,
dynamic source wakeups, and teardown. The shell renders the session's typed
output; it does not hand-author filters or observed projections.

Exact helper names are owned by the live crates. The important shape is that the
composition root installs one named read owner instead of exposing the executor
pieces to app screens.

### Internal seam 2 — typed output registration

Registers a typed output row pushed under `typed_projections["nmp.myapp.items"]`.
This is output transport machinery. A production read session may use it
internally, but an app screen should open the session/helper rather than wire
projection keys directly. The producer runs on the actor update path and must be
cheap and non-blocking (D8).

### Internal seam 3 — observed delivery

Registers scoped observed delivery behind a declared shape. The kernel opens
the matching demand, replays cached/store-backed rows to the muted sink, then
activates future delivery scoped to that shape. Under ADR-0070 this is private
read-session executor machinery, not the normal production app API. Protocol
modules and runtime internals may use it to implement sessions; product screens
should not assemble it.

### The two kernel-defined extension traits

**`ActionModule`** (`substrate/action.rs:56`) — the write seam.

```rust
pub trait ActionModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Action: Clone + Serialize + DeserializeOwned + Send + 'static;

    // Validate `action` upfront. Default: always accept.
    fn start(ctx: &mut ActionContext, action: Self::Action)
        -> Result<(), ActionRejection> { Ok(()) }

    // (The registry always mints the correlation_id — the operation's identity,
    // never a substituted event id. See #1748.)

    // True when the terminal outcome arrives async through
    // projections["action_stages"] rather than the dispatch return value.
    fn is_async_completing() -> bool { false }

    // Enqueue the ActorCommand(s) that carry out the validated action.
    fn execute(
        &self,
        ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String>;
}
```

**`CapabilityModule`** (`substrate/capability.rs:11`) — the native bridge
shape. Defines typed request/result envelopes; native code implements the
callback and reports raw facts; the kernel decides policy (D7).

```rust
pub trait CapabilityModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    type Request: Clone + Serialize + DeserializeOwned + Send + 'static;
    type Result:  Clone + Serialize + DeserializeOwned + Send + 'static;
    fn callback_interface_name() -> &'static str;
}
```

### The app-core convention

Every app-core crate follows a small naming convention so the thin staticlib
shell can call it uniformly. The convention is simply what your `register()`
function installs through substrate/protocol owner crates and what the shell
expects back:

| Export | Type | Purpose |
|---|---|---|
| `ACTION_NAMESPACE` | `&'static str` | must equal `MyActionModule::NAMESPACE` |
| `Store` | type alias | app-owned state (`Arc<Mutex<T>>`) |
| `register(app: &mut impl AppHost) -> Store` | fn | wires seams, returns store |
| `accepted() -> Update` | fn | success variant for dispatch result |
| `Update` | enum | update variants (at minimum `ActionAccepted`) |

`register()` is the app composition root. Its job is to make installed features
visible, not to hide them behind a broad preset:

```rust
pub fn register(app: &mut impl AppHost) -> FeedStore {
    let store = FEED_STORE.get_or_init(|| Arc::new(Mutex::new(Vec::new()))).clone();

    // Shape only: exact installer names are owned by the live crates.
    install_substrate(app);
    install_protocol_features(app, [follows, routing, publish]);
    install_app_features(app);

    app.register_action(NoteActionModule);
    register_microblog_read_session(app, Arc::clone(&store));

    store
}
```

A thin staticlib shell (`nmp-app-<name>`) or an `examples/shell.rs` calls only
`<app>_core::register(app)`. Broad compatibility presets, test helpers, and
replacement defaults bundles are not current composition architecture.
`nmp-codegen` still exists for host bindings (`gen swift`, `gen typed-decoders`),
but it does not generate composition wiring.

## The no-app-nouns-in-kernel rule

This is D0 restated operationally. Before adding a type to `nmp-core`, ask:
*is this generic Nostr-client infrastructure, or is it a noun some specific
app cares about?* `VerifiedEvent`, `CompiledPlan`, `InsertOutcome` are
infrastructure. `Episode`, `Highlight`, `Project`, `Group` are nouns —
protocol nouns go in `nmp-nip*` crates, app nouns in app-core crates. The live
proof that the boundary holds: `microblog-core` uses typed actions and a named
read owner while remaining a Nostr-shaped app crate, and `nmp-nip29` adds
actions + projections for group machinery while `nmp-core` gains exactly *one*
generic seam (the relay-pin routing lane) and zero group nouns.

## What crosses FFI (and what does not)

| Crosses FFI | Stays in Rust |
|---|---|
| One FlatBuffers update frame per emit (D5) | The EventStore + every `VerifiedEvent` |
| Dispatched action namespaces | Action ledger, ActorCommand queue |
| `CapabilityRequest` / `CapabilityEnvelope` | Planner, subscription pool, signer keys |
| `rev: u64` monotonic guard | All policy / retry / routing decisions |
| Typed projection rows | Kernel-internal view state |

No `Result<T,E>` crosses the boundary (D6) — failures arrive as data inside
the snapshot or as capability envelopes. The hot update transport is a single
canonical FlatBuffers schema: `UpdateFrame` carries snapshot envelopes and typed
projection rows. UniFFI is the public native binding for lifecycle, actions,
callbacks, and capability objects; wasm-bindgen is the browser binding. App-owned
raw glue is delivery-specific and is not starter-app API (see
[15](15-codegen-and-ffi.md)).

## "Where does X live?" — concrete map

| Noun | Lives in | Why |
|---|---|---|
| `VerifiedEvent`, `CompiledPlan` | `nmp-core` | generic Nostr infra |
| `Signer`, keyring access | `nmp-signers` | identity is a protocol module (D0) |
| NIP-29 `GroupId`, group actions | `nmp-nip29` | protocol noun |
| NIP-77 sync reconciler | `nmp-nip77` | protocol noun |
| `NoteRecord`, feed store | `microblog-core` | app noun (walkthrough app) |
| App-owned store (`Arc<Mutex<T>>`) | app-core crate | D4: app owns its state |
| SwiftUI list cell, OS audio handle | `apps/chirp/ios` / shell | rendering / OS execution |

The single test of correctness: a future app module can be added with **zero
changes to `nmp-core`**.

## Anti-patterns

1. **Putting `Highlight` / `Episode` / `Project` in `nmp-core`.** This is the
   exact abstraction error ADR-0009 exists to forbid — it turns the kernel
   into a junk drawer of every consumer's domain concepts. App nouns go in
   app-core crates; protocol nouns in `nmp-nip*` crates.
2. **Bypassing typed sessions and actions.** Use typed read sessions or
   generated helpers for product reads, and `register_action` / typed action
   builders for writes.
3. **Rendering raw events in SwiftUI/Kotlin/TypeScript.** Decoding `kind:1`
   JSON in the shell re-implements the kernel's reactive contract, duplicates
   state ownership (D4 violation), and breaks D5 bounding. Product reads render
   Rust-owned typed output.
4. **Adding a new registration seam without an ADR.** The existing action,
   capability, typed-output, and internal observed-delivery seams are the
   extension contract. A new seam is a kernel change that requires its own ADR.

Paste the **"Where does X live?" map** next to any PR that adds a new type and
answer the "why" column before merging.

See also: [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md) ·
[05a — Kernel substrate — the 2 traits + 3 seams](05a-substrate-traits.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
