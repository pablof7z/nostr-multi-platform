# ADR 0010: Runtime Registration At The FFI Boundary

**Date:** 2026-05-17
**Status:** accepted
**Depends on:** ADR-0009

## Context

The extension boundary needs two properties at once:

- app and protocol crates can add actions/projections without changing
  `nmp-core`;
- host calls still cross a small stable boundary that does not require a
  generated per-app FFI crate.

## Decision

NMP uses runtime registration plus generated bindings for payload formats:

- Write intents register through `ActionModule` and `register_action`.
- Host dispatch routes by namespace through the registered action table.
- State output registers through snapshot and typed-projection registration.
- `nmp-defaults` is the default composition library; app crates call it and add
  their own registrations.
- Binding/codegen tools may generate Swift/Kotlin/FlatBuffers type helpers, but
  they do not generate an app-specific composition crate.

This keeps composition as library code and keeps extension ownership beside the
crate that owns the behavior.

## Consequences

- Adding a protocol action or app projection does not require editing
  `nmp-core`.
- Hosts depend on runtime registration and shared transport helpers, not
  generated per-app FFI crates.
- `nmp init` scaffolds a thin app shell; the framework wiring comes from
  `nmp-defaults`.
