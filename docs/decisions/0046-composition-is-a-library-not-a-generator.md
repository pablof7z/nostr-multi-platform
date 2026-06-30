# ADR-0046 — Composition is a library

- **Status:** Accepted / implemented; amended by ADR-0069
- **Date:** 2026-06-12
- **Relates to:** ADR-0010, ADR-0030

**Current disposition:** Composition remains live Rust code, not generated
framework scaffolding. ADR-0069 narrowed the production shape, and the
2026-06-30 defaults deletion completes it: apps install `nmp-substrate` plus
explicit protocol and app feature installers. Hidden default presets and
replacement presets are not production, tutorial, migration, or test surfaces.

## Context

Downstream apps need one obvious way to compose the NMP substrate. Real apps use
`nmp-substrate`, named protocol installers, and app-specific Rust modules.
Generated per-app framework wiring creates a second source of truth and makes
every app own framework internals.

## Decision

NMP composition is a library call.

A downstream app:

1. creates the platform runtime builder, such as
   `nmp-native-runtime::NmpAppBuilder` for native or `BrowserAppBuilder` for web;
2. installs the substrate tier it needs through `nmp-substrate`;
3. registers app/protocol-specific actions, projections, observers, and
   capabilities;
4. starts the app.

`nmp_substrate::install` is the correctness substrate for apps that need
lower-level assembly. Protocol crates own reusable feature installers. ADR-0069
rejects hidden defaults presets as production, tutorial, migration, or test app
roots. Substrate/protocol crates own reusable registration functions, not the
platform runtime builder or ABI surface.

## Boundaries

- Binding/codegen tools may generate platform bindings and typed decoder glue.
- Composition wiring remains live Rust code, not generated framework scaffolding.
- Apps own their product modules, not copied NMP framework wiring.

## Consequences

- There is one answer to "how do I compose NMP?": depend on the NMP crates and
  call `nmp-substrate` plus the named protocol/app installers.
- Framework wiring updates in one place.
- External consumers migrate by bumping crate dependencies, not by regenerating
  and editing a private copy of NMP wiring.
