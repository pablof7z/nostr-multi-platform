# ADR 0010: Generated per-app concrete enums at the FFI boundary

**Date:** 2026-05-17
**Status:** superseded by ADR-0046 (2026-06-12)
**Resolves:** `docs/design/app-extension-kernel.md` open question 1
**Depends on:** ADR-0009 (kernel boundary)

> **Superseded by ADR-0046 (composition is a library, not a generator).** This ADR
> originally chose a generated per-app FFI crate (`nmp gen modules` producing
> `nmp-app-<name>` with composed `AppAction`/`AppUpdate`/`ViewSpec` enums). That
> generator and the `apps/fixture` crate were deleted. Apps now depend on
> `nmp-defaults` and call `register_defaults`; `nmp init` scaffolds a thin
> `<name>-core` crate that calls it — no generated FFI crate. See ADR-0046 for the
> current model and git history for the original generated-enum design.
