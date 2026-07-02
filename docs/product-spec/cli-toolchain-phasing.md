# Product Spec: CLI And Toolchain

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

## CLI Commands

```text
nmp init [<path>]                    Scaffold a thin NMP app.
nmp add ios | android | desktop      Add a native platform shell.
nmp add web                          Add a browser-runtime web shell.
nmp add module <crate>               Add an app/protocol module dependency.
nmp gen swift                        Regenerate Swift host bindings.
nmp gen typed-decoders               Regenerate typed decoder bindings.
nmp gen bindings [target]            Regenerate supported platform bindings.
nmp gen projection <name>            Scaffold an app-core projection seam.
nmp gen feed <name>                  Scaffold source/reducer wiring.
nmp gen action <name>                Scaffold an ActionModule.
nmp gen screen <name>                Scaffold a platform screen.
nmp upgrade                          Bump NMP dependencies and run migrations.
```

Composition is an app-owned Rust root that calls `nmp-substrate` plus explicit
named installers from protocol crates and app modules, then hands the composed
app to the selected platform runtime builder. Native uses
`nmp-native-runtime::NmpAppBuilder` surfaced through UniFFI; web uses
`nmp-browser-runtime::BrowserAppBuilder` surfaced through wasm-bindgen.

## `nmp init`

The scaffold creates a small app workspace:

- app Rust core with an explicit composition root for substrate, protocol, app,
  publish/signing, and capability features,
- native shells for selected platforms,
- generated UniFFI bindings and typed decoder targets,
- build orchestration,
- CI wiring.

The scaffold does not generate a second framework runtime crate. App-specific
logic is added through Rust modules, actions, typed sessions, projections, and
capabilities.

## Build Pipeline

- iOS links the Rust static library/XCFramework into a SwiftUI shell.
- Android links per-ABI Rust libraries into a Compose shell.
- Desktop may link Rust directly.
- Web uses the browser runtime (`nmp-browser-runtime`), which also owns the
  wasm-bindgen ABI glue (`nmp-browser-runtime::wasm`).
- `just` remains the primary local build entrypoint.

CI should verify generated bindings, Rust tests, doctrine lint, and the platform
builds relevant to the touched code.

## Release Phasing

The tactical release queue lives in GitHub Issues. This file owns only durable
CLI/toolchain behavior, not milestone queue state.
