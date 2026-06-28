# ADR-0068 — Native runtime ownership split (nmp-ffi is C ABI glue)

- **Status:** Accepted; amended by ADR-0069 and ADR-0072
- **Date:** 2026-06-28
- **Relates to:** ADR-0030 (UniFFI vs C-ABI), ADR-0046 (composition is a library), ADR-0067 (browser runtime split), #2205, #2209

**Current disposition:** the native runtime ownership split survives. ADR-0069
narrows composition: `nmp-defaults` is reusable explicit composition, not hidden
production app policy. ADR-0072 keeps `nmp-ffi` as ABI glue only; native shells
render, execute capabilities, and hold ephemeral presentation state.

## Context

Before #2205 landed, `nmp-ffi` looked like the durable native composition root
because it owned the `NmpApp` handle, actor-thread lifecycle, native session
registries, and some higher-order runtime orchestration. That made the wrong
work look local: new native sessions, builder state, and platform lifecycle
logic tended to land in the C ABI crate instead of a typed native runtime owner.

At the same time, `nmp-defaults` must remain the reusable Layer-5 composition
library. Its job is to register generic NMP mechanisms through `AppHost`; it must
not become a native runtime crate and must not depend on `nmp-ffi`.

## Decision

Native apps use the same three-layer ownership split that ADR-0067 established
for browser apps:

1. **`nmp-defaults` is pure Layer-5 composition.** It registers generic NMP
   defaults through `nmp_core::substrate::AppHost` and narrow registrar traits.
   It owns no platform runtime handle, no C ABI, no native actor-thread
   lifecycle, and no app/operator policy.

2. **`nmp-native-runtime` is the native platform runtime adapter.** It owns the
   native `NmpApp`/handle type, actor-thread lifecycle, native runtime slots,
   native session registries, native Rust APIs, and the native typestate builder
   (`NmpAppBuilder` / `RunConfig`). It composes `nmp-defaults` like a leaf app
   runtime.

3. **`nmp-ffi` is the C ABI delivery shell only.** It owns `extern "C"` symbols,
   opaque pointer conversion, C strings, panic guards, callback registration
   glue, and C-compatible allocation/freeing. It calls into
   `nmp-native-runtime`; it does not own routing, signing policy, NIP modules,
   protocol defaults, app defaults, projection policy, persistence policy,
   retry policy, session semantics, or account state.

`nmp-browser-runtime` remains the browser analogue: it owns the browser runtime
adapter, wasm-bindgen Worker export, and the wasm-bindgen ABI glue
(`nmp-browser-runtime::wasm`). The `nmp-wasm` crate was deleted in #2202;
`nmp-browser-runtime::wasm` is the sole browser WASM ABI surface.

## Landed State

#2210-#2214 landed the split described here. The live tree has
`nmp-native-runtime` as the native runtime owner, `nmp-ffi` as the C ABI shell,
and `nmp-defaults` as pure `AppHost` composition. This landed state is the
precedent; the old `nmp-ffi` runtime ownership must not be recreated.

Release sequencing and v1 gating are tactical queue state and stay in GitHub
Issues (#2205 and #2121). This ADR owns the durable crate-boundary rule.

## Consequences

- Builder/scaffold docs teach `nmp-native-runtime::NmpAppBuilder` as the native
  runtime entry point.
- `nmp-defaults` must be usable by native and browser runtimes without depending
  on either runtime.
- `nmp-ffi` can keep C ABI compatibility only by delegating to runtime APIs; it
  cannot preserve old crate paths as a reason to retain runtime ownership.
- Boundary gates catch restored `nmp-defaults -> nmp-ffi` dependencies and
  restored `nmp-ffi` runtime/session ownership.
