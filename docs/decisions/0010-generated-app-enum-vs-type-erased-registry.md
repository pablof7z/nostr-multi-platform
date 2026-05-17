# ADR 0010: Generated per-app concrete enums at the FFI boundary

**Date:** 2026-05-17
**Status:** accepted
**Resolves:** `docs/design/app-extension-kernel.md` open question 1
**Depends on:** ADR-0009 (kernel boundary)

## Context

ADR-0009 establishes that NMP is a kernel with extension modules. Apps assemble themselves from a kernel (`nmp-core`), a set of protocol modules (`nmp-nip01`, `nmp-nip17`, ...), and an app-specific core crate (`twitter-core`, `highlighter-core`, ...). Each layer can define typed `AppAction` variants, `AppUpdate` variants, `ViewSpec` variants, and so on.

The framework's existing FFI surface (UniFFI on iOS/Android, wasm-bindgen on web) expects **concrete, closed types**. Open enums don't exist in UniFFI. Two paths are possible:

1. **Generated app enum.** Each app's build runs a codegen step that produces concrete per-app `AppAction = KernelAction | Nip01Action | Nip17Action | TwitterAction | ...` enums by composing variants from the kernel, the chosen protocol modules, and the app's own core crate. The FFI exposes the per-app concrete types. Compile-time type safety end-to-end.
2. **Type-erased registry.** Modules register handlers keyed by string namespace; payloads cross FFI as `Vec<u8>` (or `serde_json::Value`). The kernel doesn't know module types statically; it dispatches via the registry. Plug-and-play module loading; loses type safety at the FFI boundary.

This decision affects every layer of the framework.

## Decision

**Generated app enum.** Each app's build produces concrete per-app `AppAction`, `AppUpdate`, `ViewSpec`, `Capability*` enums via the `nmp gen modules` codegen step. UniFFI exposes the per-app concrete types via a thin per-app FFI crate (`nmp-app-<name>`).

## How it works

### App declaration

An app declares its modules in `nmp.toml`:

```toml
[app]
name = "twitter"
display_name = "Twitter (nmp demo)"
bundle_id = "com.example.nmp.twitter"

[modules]
kernel = "nmp-core"
protocol = ["nmp-nip01", "nmp-nip02", "nmp-nip25", "nmp-nip10"]
app = ["twitter-core"]

[platforms]
ios = true
desktop = true
```

### Codegen output

`nmp gen modules` produces a per-app crate `nmp-app-twitter/`:

```
nmp-app-twitter/
├── Cargo.toml             # depends on nmp-core, the chosen protocol modules, twitter-core
├── src/
│   ├── lib.rs             # uniffi::setup_scaffolding!()
│   ├── action.rs          # generated AppAction enum
│   ├── update.rs          # generated AppUpdate enum
│   ├── view_spec.rs       # generated ViewSpec enum
│   ├── capability.rs      # generated capability traits
│   ├── domain.rs          # generated domain registrations
│   └── ffi.rs             # FfiApp wrapper exposing the concrete types
├── uniffi.toml
└── bindings/
    ├── swift/             # generated Swift bindings checked in
    ├── kotlin/            # generated Kotlin bindings checked in
    └── typescript/        # generated TS bindings checked in
```

The generated `action.rs` looks like:

```rust
// AUTO-GENERATED — do not edit by hand. Regenerate with `nmp gen modules`.

#[derive(Clone, uniffi::Enum)]
pub enum AppAction {
    // Kernel variants (from nmp-core)
    Kernel(nmp_core::KernelAction),

    // Protocol module variants
    Nip01(nmp_nip01::Action),
    Nip02(nmp_nip02::Action),
    Nip25(nmp_nip25::Action),
    Nip10(nmp_nip10::Action),

    // App module variants
    Twitter(twitter_core::Action),
}

impl AppAction {
    pub fn dispatch(self, app: &Arc<FfiApp>) {
        match self {
            AppAction::Kernel(a) => app.core_tx.send(CoreMsg::Kernel(a)).ok(),
            AppAction::Nip01(a) => app.core_tx.send(CoreMsg::Module("nip01", a.into_bytes())).ok(),
            AppAction::Nip02(a) => app.core_tx.send(CoreMsg::Module("nip02", a.into_bytes())).ok(),
            // ...
        };
    }
}
```

`AppUpdate`, `ViewSpec`, and the capability traits are generated analogously.

### How the actor dispatches

Inside `nmp-core`'s actor, message handling is generic over module identifier:

```rust
enum CoreMsg {
    Kernel(KernelAction),
    Module(&'static str, Vec<u8>),    // module namespace + serialized action
    Internal(InternalEvent),
}

fn handle_message(&mut self, msg: CoreMsg) {
    match msg {
        CoreMsg::Kernel(a) => self.handle_kernel_action(a),
        CoreMsg::Module(ns, bytes) => {
            let handler = self.module_registry.get(ns).expect("module not registered");
            handler.dispatch(self, &bytes);
        }
        CoreMsg::Internal(e) => self.handle_internal(e),
    }
}
```

Each module registers itself with the kernel at startup, providing the `dispatch` closure that knows how to deserialize its own action type. The action bytes are produced by the generated FFI layer (which has typed access) and consumed by the module (which has typed access). The kernel in the middle is module-agnostic.

### What UniFFI sees

UniFFI sees only the generated per-app `nmp-app-twitter` crate. From its perspective, `AppAction` is a normal closed enum. Bindings regenerate deterministically.

### Per-platform consumers

A Swift app links `NmpAppTwitter.xcframework`. It sees Swift types `AppAction.kernel(...)`, `AppAction.nip01(...)`, `AppAction.twitter(...)`. The generated wrappers (`useTimeline`, `useProfile`, `@Twitter` property wrapper, etc.) come from the same codegen step.

A Kotlin app links the `.so` and the generated Kotlin bindings. Same shape.

A web app imports the generated wasm package. Same.

## Trade-offs

### What we get

- **Compile-time type safety end-to-end.** Swift code can't construct an `AppAction.nip17(...)` if the app doesn't include `nmp-nip17`. The action variants for unused modules don't exist in that app's binary.
- **Idiomatic enums per platform.** Swift `enum AppAction { case ... }`, Kotlin `sealed class AppAction`. Pattern-matchable.
- **Bindings drift catchable.** CI regenerates bindings and diffs. A module change that produces incompatible bindings fails the build.
- **Tree shaking.** Unused modules don't compile in; unused bindings don't ship.
- **Cleaner FFI surface.** No `Vec<u8>` blobs across FFI. No JSON parsing on the platform side. No string-keyed switches.

### What we lose / what costs

- **Codegen is critical-path.** The `nmp gen modules` step must run before each build. CI must regenerate. App developers need to re-run after adding/removing modules. Mitigation: cargo `build.rs` can run it; precommit hooks; CI gate.
- **Plug-and-play module loading at runtime is impossible.** Adding a module requires a rebuild. (This is fine — apps don't dynamically load modules at runtime.)
- **Per-app FFI crate adds a build step.** Each app has its own `nmp-app-<name>` crate. Slight build-graph complexity. Mitigation: scaffolded by `nmp init` and `nmp add module`; developers rarely touch it.
- **Binding regeneration churn.** Every module addition/removal regenerates bindings. Mitigation: CI handles it; checked-in bindings make diffs reviewable.

## Why not type-erased

The type-erased path (`Vec<u8>` actions, string-keyed registry, runtime dispatch) was seriously considered and rejected for v1:

- **Loses safety where it matters most.** The FFI boundary is exactly where strong typing has the highest payoff. Swift / Kotlin / TypeScript consumers benefit from idiomatic typed enums. Vec<u8> + opaque dispatch reproduces JavaScript-shaped fragility in three statically-typed languages.
- **Runtime errors replace compile-time errors.** Send the wrong-shape JSON action for a module → runtime failure. With generated enums → won't compile.
- **Doesn't actually buy plug-and-play.** Apps don't load modules dynamically. They build with a chosen module set. The "plug-and-play" benefit is theoretical.
- **Harder to debug.** Stringly-typed dispatch hides the actual shape of valid actions. Generated enums document themselves.

If a future need for runtime module loading emerges (e.g., shipping a Highlighter "extension" that adds functionality to an existing app), it can be added as a layer on top of the generated enum (e.g., a `KernelAction::Plugin { namespace, bytes }` variant that intentionally falls back to type-erased dispatch for plugin code). The default path stays typed.

## Consequences

- The `nmp` CLI ships in v1 with `nmp gen modules` as the central command.
- Every example app, the starter app, and the Twitter proof all run through `nmp gen modules` in their build pipeline.
- A per-app FFI crate becomes the canonical FFI surface; raw `nmp-ffi` is for the kernel only.
- Adding a new protocol module (`nmp-nip-XX`) becomes a standard pattern: create the crate, define the `Action`/`Update`/`ViewSpec`/`Capability` types, add to `nmp.toml`, run codegen.

## Alternatives considered

| Alternative | Why not |
|---|---|
| Type-erased registry (`Vec<u8>` actions) | See "Why not type-erased" above. |
| Trait object dispatch over `Box<dyn Action>` | UniFFI doesn't support trait objects across FFI cleanly. Same root issue. |
| Cargo features on a monolithic crate | Explosive feature matrix; binding regeneration on every feature combination; doesn't help with app-specific types. |
| Procedural macros generating per-module FFI | Tried in head. Inverts the dependency direction — modules would need to know about FFI exposure. The codegen tool generating an aggregating crate is cleaner. |

## Validation

- Phase 1a.1 (kernel substrate prototype) generates a tiny `nmp-app-fixture` crate with one DomainModule + one ViewModule + one ActionModule. UniFFI bindings compile and round-trip a dispatch on desktop.
- Phase 1a.2 (Profile module on desktop) generates `nmp-app-twitter` with `nmp-core` + `nmp-nip01` + `twitter-core`. UniFFI bindings compile.
- Phase 1a.3 (iOS port) consumes the same generated crate via xcframework. iOS Swift code sees typed enums.
- Bindings drift detection in CI: regenerate, diff, fail on mismatch.
