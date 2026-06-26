# Product Spec: CLI And Toolchain

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

## CLI Commands

```text
nmp init [<path>]                    Scaffold a thin NMP app.
nmp add ios | android | desktop      Add a native platform shell.
nmp add web                          Add the post-v1 wasm/web shell.
nmp add module <crate>               Add an app/protocol module dependency.
nmp gen swift                        Regenerate Swift host bindings.
nmp gen typed-decoders               Regenerate typed decoder bindings.
nmp gen bindings [target]            Regenerate supported platform bindings.
nmp gen projection <name>            Scaffold an app-core projection seam.
nmp gen feed <name>                  Scaffold source/reducer wiring.
nmp gen action <name>                Scaffold an ActionModule.
nmp gen screen <name>                Scaffold a platform screen.
nmp doctor                           Diagnose toolchain and build environment.
nmp upgrade                          Bump NMP dependencies and run migrations.
```

Composition is a library call through `nmp-defaults` and `NmpAppBuilder`.

## `nmp init`

The scaffold creates a small app workspace:

- app Rust core that calls `nmp_defaults::register_defaults` or the narrower
  substrate tier,
- native shells for selected platforms,
- generated bindings/decoder targets,
- build orchestration,
- CI wiring.

The scaffold does not generate a framework-owned FFI crate. App-specific logic
is added through Rust modules, actions, observers, projections, and capabilities.

## Build Pipeline

- iOS links the Rust static library/XCFramework into a SwiftUI shell.
- Android links per-ABI Rust libraries into a Compose shell.
- Desktop may link Rust directly.
- Web uses the post-v1 wasm/browser runtime.
- `just` remains the primary local build entrypoint.

CI should verify generated bindings, Rust tests, doctrine lint, and the platform
builds relevant to the touched code.

## Release Phasing

The tactical release queue lives in GitHub Issues. This file owns only durable
CLI/toolchain behavior, not milestone queue state.
