# ADR-0016 — F-TTL force refresh on claim functions

- **Status:** Accepted
- **Date:** 2026-05-18

## Context

Replaceable Nostr records such as profiles and addressable events need lazy TTL
re-verification plus an explicit user-triggered refresh path.

The refresh path should not be a separate public FFI verb. A separate verb would
create another way to mutate freshness state and would widen the native surface.

## Decision

Force refresh is a parameter on existing claim functions.

- Profile claims pass `force` to request immediate kind:0 re-verification.
- Addressable event/reference claims use the same TTL machinery when the
  resolved identity has a replaceable freshness record.
- Immutable event ids do not need TTL refresh because the event id names a fixed
  event.

`force != 0` treats the stored `check_again_after` as due now. `force == 0`
uses the normal lazy TTL gate.

## Consequences

- Refresh and ordinary claims share one refcount/freshness path.
- Hosts get pull-to-refresh behavior without another native symbol.
- The in-flight guard and EOSE restamp behavior remain owned by Rust.
