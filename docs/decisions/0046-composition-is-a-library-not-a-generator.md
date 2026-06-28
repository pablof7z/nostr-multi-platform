# ADR-0046 — Composition is a library

- **Status:** Accepted / implemented; amended by ADR-0069
- **Date:** 2026-06-12
- **Relates to:** ADR-0010, ADR-0030

**Current disposition:** Composition remains live Rust code, not generated
framework scaffolding. ADR-0069 narrows the production shape: apps install
explicit substrate, protocol, and app feature installers. `register_defaults()`
is no longer the standard production composition surface; any preset is
tutorial/test/migration compatibility with owner and removal/formalization gate.

## Context

Downstream apps need one obvious way to compose the NMP substrate. Real apps use
the shared defaults library and add app-specific Rust modules on top. Generated
per-app framework wiring creates a second source of truth and makes every app own
framework internals.

## Decision

NMP composition is a library call.

A downstream app:

1. creates the platform runtime builder, such as
   `nmp-native-runtime::NmpAppBuilder` for native or `BrowserAppBuilder` for web;
2. installs the substrate/default tier it needs through `nmp-defaults`;
3. registers app/protocol-specific actions, projections, observers, and
   capabilities;
4. starts the app.

`nmp-defaults::register_substrate` is the correctness substrate for apps that
need lower-level assembly. Other `nmp-defaults` installers may provide reusable
protocol composition. ADR-0069 rejects `register_defaults()` as the standard
production app root; if it remains, it is tutorial/test/migration compatibility
with owner and removal/formalization gate. `nmp-defaults` owns reusable
registration functions, not the platform runtime builder or ABI surface.

## Boundaries

- Binding/codegen tools may generate platform bindings and typed decoder glue.
- Composition wiring remains live Rust code, not generated framework scaffolding.
- Apps own their product modules, not copied NMP framework wiring.

## Consequences

- There is one answer to "how do I compose NMP?": depend on the NMP crates and
  call the defaults/substrate library.
- Framework wiring updates in one place.
- External consumers migrate by bumping crate dependencies, not by regenerating
  and editing a private copy of NMP wiring.
