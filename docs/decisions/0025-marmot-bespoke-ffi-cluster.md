# ADR-0025 — Marmot read/lifecycle FFI exception

- **Status:** Accepted / narrowed
- **Date:** 2026-05-21
- **Relates to:** ADR-0039

## Context

Marmot MLS groups carry handle-scoped cryptographic state that must remain in a
typed Rust handle. That state cannot be modeled as stateless generic action JSON
without losing the typed lifetime and credential boundary.

Mutating Marmot operations still use the generic action dispatch path. The
exception is only for handle lifecycle, active registration, and the Rust-owned
credential slot needed by MLS.

## Decision

The Marmot-specific native FFI surface is limited to read/lifecycle registration
for the typed MLS handle:

- `nmp_marmot_register_active`;
- `nmp_marmot_unregister`;
- Rust-only helpers that reuse already-in-hand secret material during local nsec
  sign-in without exposing that material as native ABI.

Marmot read state is delivered through registered pushed projections, not host
snapshot pull functions. Marmot writes route through the generic action dispatch
namespace.

## Credential Slot

`NmpApp` carries `mls_local_nsec`, a Rust-owned slot for the active local
account's nsec needed by the MLS credential path. The slot is scoped to MLS and
guarded so only `nmp-marmot` may read it.

NIP-17 DMs must not read this slot. DM signing/encryption uses the signer-session
and NIP-44 signer seams.

## Limits

- Do not add new Marmot feature symbols when generic dispatch or pushed
  projections can represent the operation.
- Do not pass raw secret key material through native-facing Marmot symbols.
- Do not use Marmot's exception as precedent for app-specific write APIs.

## Consequences

- Marmot keeps the Rust handle it needs for MLS correctness.
- The public write path remains unified.
- Native shells consume Marmot state through the same pushed projection model as
  other app state.
