# ADR-0046 — Composition is a library

- **Status:** Accepted / implemented
- **Date:** 2026-06-12
- **Relates to:** ADR-0010, ADR-0030

## Context

Downstream apps need one obvious way to compose the NMP substrate. Real apps use
the shared defaults library and add app-specific Rust modules on top. Generated
per-app framework wiring creates a second source of truth and makes every app own
framework internals.

## Decision

NMP composition is a library call.

A downstream app:

1. creates an `NmpAppBuilder`;
2. installs the substrate/default tier it needs through `nmp-defaults`;
3. registers app/protocol-specific actions, projections, observers, and
   capabilities;
4. starts the app.

`nmp-defaults::register_defaults` is the standard full Nostr composition.
`nmp-defaults::register_substrate` is the narrower correctness substrate for
apps that need lower-level assembly.

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
