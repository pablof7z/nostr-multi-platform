# ADR-0072: Runtime, capability, and shell boundary

## Decision

Rust owns product state, protocol policy, relay routing, signing policy,
privacy, persistence, retry and recovery, time decisions, durable navigation
meaning, and cross-platform invariants.

Native, web, desktop, TUI, widgets, extensions, and other platform shells own
only rendering, platform UX affordances, execution of OS or browser
capabilities, and ephemeral presentation state that cannot become product truth.

Capability flow is:

```text
Rust requests capability
  -> shell executes the raw OS/browser/API operation
  -> shell reports raw success, failure, or data
  -> Rust decides durable state, retry, status, and user-visible meaning
```

Native hosts use UniFFI as the public binding surface. FlatBuffers bytes remain
the hot action/update payload transport where NMP needs compact typed frames.
Raw C/JNI framework APIs are not reusable native public architecture.

Browser hosts use the browser runtime and wasm-bindgen/Worker boundary owned by
the browser runtime. Native runtime lifecycle belongs to the native runtime.
Runtime crates own platform constraints; app Rust crates own product meaning.

The app-RPC/provider lane for LLM/STT/TTS-class calls is not a shell policy
escape. Until a separate issue or doc defines a generic NMP-owned lane, apps
expose app-owned UniFFI methods or typed capability contracts and Rust remains
the owner of durable product meaning.

## Context

NMP spans Swift, Kotlin, TypeScript, TUI, browser, and desktop surfaces. If a
second platform would need to reimplement a decision to stay correct, that
decision belongs in Rust.

The runtime boundary must let shells feel native without letting them own relay
policy, signer state, publish completion, cache truth, protocol parsing,
product queues, or retry loops.

## Consequences

Platform features often need more typed action/result plumbing than a direct
native shortcut. The payoff is one product behavior across platforms and
replaceable capability bridges.

Browser and native runtime proofs must exercise real platform constraints when
they are part of the claim, such as Worker/OPFS availability or native
capability lifecycles.

## Boundaries

Permitted:

- shell rendering and transient presentation state;
- shell execution of raw capabilities;
- UniFFI native bindings over app-owned Rust facades;
- wasm-bindgen/browser runtime glue for browser hosts;
- last-emitted Rust mirror frames for OS-owned surfaces.

Forbidden:

- shell-owned protocol parsing for product state;
- shell relay selection, signer selection, publish retry, or recovery policy;
- raw C/JNI framework APIs as reusable public native surface;
- polling or timer loops for correctness;
- OS/headless surfaces reporting success from dispatch acceptance alone;
- provider calls that bypass Rust-owned product state.

## Enforcement

Doctrine lint checks native/runtime boundaries, raw native ABI reintroduction,
polling, raw capability policy, and shell-owned product logic. Clean-room docs
gates route native developers to UniFFI and typed app facades, not raw binding
symbols.

Capability tests assert raw-result reporting and Rust-owned state decisions.
Runtime tests prove platform lifecycle and storage constraints where those
constraints define product behavior.

## Related

- [ADR-0069](0069-explicit-feature-composition.md) - explicit composition.
- [ADR-0070](0070-typed-read-sessions.md) - read-session boundary.
- [ADR-0071](0071-write-intents-and-route-provenance.md) - write boundary.
- [docs/ffi-surface.md](../ffi-surface.md) - binding and transport rules.
- [docs/architecture/crate-boundaries.md](../architecture/crate-boundaries.md)
  - crate and runtime ownership.
- #2726 - app-RPC/provider transport lane decision.
- #2746 - ADR current-only cleanup.
