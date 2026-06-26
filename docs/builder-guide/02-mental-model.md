# 02 — Mental model: kernel + extension seams

*Status: SHIPS · Audience: both · Read after [01](01-what-nmp-is.md).*

If you remember one thing: **NMP is a Nostr-native app kernel with first-class
extension modules — not a framework with closed built-ins.** The kernel knows
*how* to run a reactive Nostr client. It does not know *what* a Profile, an
Episode, a Highlight, or a TODO is. Those nouns live in modules you write.

This section gives you the four-layer stack, the three extension seams, the
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
                                  │ FlatBuffers payload; UniFFI = lifecycle/bindings
┌────────────────────────────────┴───────────────────────────────────────┐
│ C-ABI + COMPOSITION     nmp-ffi (shared `nmp_app_*` C-ABI surface)        │
│                         nmp-defaults (canonical composition: one call)  │
│  owns: lifecycle / action / capability FFI + `register_defaults` glue    │
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

Representative shipped crates are labelled in their layer above:
`nmp-core` (kernel), `nmp-nip29` / `nmp-nip42` / `nmp-nip77` / `nmp-signers`
(protocol modules), `apps/chirp/crates/nmp-app-chirp` + `microblog-core` (app cores),
`nmp-defaults` (canonical composition root), and `nmp-ffi` (shared C-ABI).
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

## The 3 extension seams

Extension crates plug into a vanilla `NmpApp` through exactly three seams
(`crates/nmp-ffi/src/lib.rs:1087-1599`). A crate uses one, two, or all three;
it never reaches into kernel internals.

### Seam 1 — `register_action(module)`

```rust
app.register_action(MyActionModule);
```

Registers an `ActionModule`: its `start()` validates dispatched actions;
its `execute()` enqueues `ActorCommand`s into the actor. The registered module
receives every typed dispatch envelope whose namespace matches
`MyActionModule::NAMESPACE`.

### Seam 2 — `register_typed_snapshot_projection(key, closure)`

```rust
app.register_typed_snapshot_projection("nmp.myapp.items", move || {
    store.lock().ok().map(|g| encode_items_projection(&g))
});
```

Registers a typed sidecar pushed under `typed_projections["nmp.myapp.items"]` on
every snapshot tick. The closure runs on the **actor thread**; it must be
cheap and non-blocking (D8). Registered under dotted `nmp.*` namespaces.

### Seam 3 — `open_observed_projection(decl)`

```rust
app.open_observed_projection(ObservedProjection::from_kinds(
    Arc::new(MyObserver { store: Arc::clone(&store) }),
    "nmp.myapp.items",
    0,
    [KIND_NOTE],
    128,
));
```

Registers a `ObservedProjectionSink` behind a declared shape. The kernel opens
the matching interest, replays cached/store-backed rows to the muted sink, then
activates future delivery scoped to that shape. App/product read models do not
subscribe to a public filterless accepted-event observer.

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
function expects from `nmp-defaults` and what the shell expects back:

| Export | Type | Purpose |
|---|---|---|
| `ACTION_NAMESPACE` | `&'static str` | must equal `MyActionModule::NAMESPACE` |
| `Store` | type alias | app-owned state (`Arc<Mutex<T>>`) |
| `register(app: &mut impl AppHost) -> Store` | fn | wires seams, returns store |
| `accepted() -> Update` | fn | success variant for dispatch result |
| `Update` | enum | update variants (at minimum `ActionAccepted`) |

`register()` is the composition root. From the microblog walkthrough
([19a](19a-walkthrough-microblog.md)):

```rust
pub fn register(app: &mut impl AppHost) -> FeedStore {
    let store = FEED_STORE.get_or_init(|| Arc::new(Mutex::new(Vec::new()))).clone();
    // Inherit canonical NMP composition (routing, outbox, DMs, zaps, WOT).
    nmp_defaults::register_defaults(app);
    // App-specific seams.
    app.register_action(NoteActionModule);
    app.open_observed_projection(ObservedProjection::from_kinds(
        Arc::new(FeedObserver { store: Arc::clone(&store) }),
        FEED_SNAPSHOT_KEY,
        0,
        [KIND_NOTE],
        128,
    ));
    let projector = Arc::clone(&store);
    app.register_typed_snapshot_projection(FEED_SNAPSHOT_KEY, move || {
        match projector.lock() {
            Ok(g) => project_feed(&g),
            Err(_) => None,   // D6: no panic on poison
        }
    });
    store
}
```

A thin staticlib shell (`nmp-app-<name>`) or an `examples/shell.rs` then calls
only `<app>_core::register(app)`. The `register_defaults` call lives inside the
app-core composition root, so the canonical defaults are installed exactly once.
`nmp-codegen` still exists for host bindings (`gen swift`, `gen typed-decoders`),
but it does not generate composition wiring.

## The no-app-nouns-in-kernel rule

This is D0 restated operationally. Before adding a type to `nmp-core`, ask:
*is this generic Nostr-client infrastructure, or is it a noun some specific
app cares about?* `VerifiedEvent`, `CompiledPlan`, `InsertOutcome` are
infrastructure. `Episode`, `Highlight`, `Project`, `Group` are nouns —
protocol nouns go in `nmp-nip*` crates, app nouns in app-core crates. The live
proof that the boundary holds: `microblog-core` exercises all three seams
while remaining a Nostr-shaped app crate, and `nmp-nip29` adds actions +
projections for group machinery while `nmp-core` gains exactly *one* generic
seam (the relay-pin routing lane) and zero group nouns.

## What crosses FFI (and what does not)

| Crosses FFI | Stays in Rust |
|---|---|
| One FlatBuffers update frame per emit (D5) | The EventStore + every `VerifiedEvent` |
| Dispatched action namespaces | Action ledger, ActorCommand queue |
| `CapabilityRequest` / `CapabilityEnvelope` | Planner, subscription pool, signer keys |
| `rev: u64` monotonic guard | All policy / retry / routing decisions |
| Typed projection sidecars | Kernel-internal view state |

No `Result<T,E>` crosses the boundary (D6) — failures arrive as data inside
the snapshot or as capability envelopes. The hot update transport is a single
canonical FlatBuffers schema: `UpdateFrame` carries snapshot envelopes and typed
projection sidecars. The raw C/JNI ABI remains live for lifecycle/actions/
capabilities, but it no longer carries JSON runtime snapshots (see
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
2. **Bypassing the shipped seams.** Use `register_typed_snapshot_projection` for
   named read output, `open_observed_projection` for declared event-driven
   read models, and `register_action` for the write path.
3. **Bypassing `register_typed_snapshot_projection` to render raw events in
   SwiftUI.** Decoding `kind:1` JSON in Swift re-implements the kernel's
   reactive contract in the shell, duplicates state ownership (D4 violation),
   and breaks D5 bounding. Every read goes through a registered projection or
   an `ObservedProjectionSink`-driven view with a declared shape.
4. **Adding a 4th registration seam without an ADR.** The three seams are the
   extension contract. A new seam is a kernel change that requires its own ADR.

Paste the **"Where does X live?" map** next to any PR that adds a new type and
answer the "why" column before merging.

See also: [03 — Doctrine D0–D10 end-to-end](03-doctrine-d0-d8.md) ·
[05a — Kernel substrate — the 2 traits + 3 seams](05a-substrate-traits.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[20 — Adding a new protocol module (`nmp-nip29` as reference)](20-new-protocol-module.md)
