# ADR-0025 - Marmot credential-slot exception

- **Status:** Amended 2026-06-30; bespoke FFI lifecycle retired
- **Date:** 2026-05-21
- **Relates to:** ADR-0039, ADR-0069

## Context

Marmot MLS groups carry handle-scoped cryptographic state that must remain in a
typed Rust handle. That state cannot be modeled as stateless generic action JSON
without losing the typed lifetime and credential boundary.

The previous decision allowed a Marmot-specific native lifecycle surface for
active registration. That lifecycle surface is now retired. Marmot is installed
like any other reusable protocol crate through explicit Rust composition.

## Decision

Marmot active support is installed only through:

```rust
nmp_marmot::install(app, marmot_config)?;
```

The installer registers the action module, ingest parsers, identity observer,
and typed projections owned by `nmp-marmot`. There is no `ffi` feature, no
`nmp_marmot_*` native symbol family, no app-owned lifecycle callback, and no
hidden key-package autopublish flag.

Marmot writes route through the generic `nmp.marmot` action namespace. Marmot
read state is delivered through pushed typed projections:

- `nmp.marmot.snapshot`;
- `nmp.marmot.messages`.

## Credential Slot

`NmpApp` carries `mls_local_nsec`, a Rust-owned slot for the active local
account's nsec needed by the MLS credential path. The slot is scoped to MLS and
guarded so only `nmp-marmot` may read it.

Native runtime may hand `nmp-marmot` a live slot handle wrapped as
`MarmotLocalCredentialSlot`, but it must not parse or inspect the key material.
The raw-key accessor does not belong on `AppHost` or `HostCapabilities`.

NIP-17 DMs must not read this slot. DM signing/encryption uses the signer-session
and NIP-44 signer seams.

## Limits

- Do not add new Marmot feature symbols when generic dispatch or pushed
  projections can represent the operation.
- Do not pass raw secret key material through native-facing Marmot APIs.
- Do not use Marmot's exception as precedent for app-specific write APIs.
- Do not restore active-register/unregister lifecycle methods.

## Consequences

- Marmot keeps the Rust handle and MDK SQLite store it needs for MLS
  correctness.
- The public write path remains unified.
- Native shells consume Marmot state through the same pushed projection model as
  other app state.
- Account switches rebind the active Marmot projection through the shared
  identity-change registrar rather than capturing a one-time projection handle.
