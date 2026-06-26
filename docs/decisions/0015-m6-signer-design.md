# ADR-0015 — Signer Crate Boundary And Session Ownership

**Date:** 2026-05-18
**Status:** accepted
**Doctrines invoked:** D0, D4, D6, D7, D8

## Context

NMP needs local-key, remote-signer, and platform-signer support without making
`nmp-core` own signer policy or secret storage. The kernel must be able to
request signatures, route operations to the active account, and keep native
shells away from secret material.

## Decision

Signer implementations live outside `nmp-core`.

- `nmp-nip46` owns transport-agnostic NIP-46 protocol parsing, RPC, bunker URI,
  and progress-code primitives.
- `nmp-signers` owns signer traits, local signer implementation, account
  management, and signer payload shapes.
- `nmp-signer-iface` holds the cross-crate operation types that `nmp-core` may
  name without depending on `nmp-signers`.
- `nmp-core` sees signer work as typed actor/capability operations and active
  account state, not as concrete signer implementations.
- Native shells execute OS capabilities such as Keychain/Keystore or external
  signer launch/return, then report raw results to Rust.

The active account switch is actor-owned. A switch installs the new signer
state synchronously before operations routed after the switch can sign. Interest
rewiring is driven by kernel account/session commands rather than by a separate
observer scaffold.

## Signer Contract

The signer surface keeps these invariants:

- `pubkey()` is synchronous once a signer is constructed or restored.
- signing returns an operation handle that can resolve without blocking the
  actor;
- NIP-04 and NIP-44 support is optional per signer kind;
- signer mismatch postconditions are enforced before accepting an account or a
  returned signed event;
- secret-bearing payloads stay out of host-rendered state.

## FFI Boundary

Signers are not directly FFI-exposed. Hosts dispatch account/signing actions or
complete capability requests. Errors become typed action results, diagnostics,
or user-visible state; they do not cross FFI as exceptions.

## Consequences

- `nmp-core` does not depend on concrete signer crates or app identity nouns.
- Native code never decides account policy.
- External signer and keychain integrations can vary per platform while Rust
  keeps the session/account rules authoritative.
- Signer persistence and the keyring/LMDB split are owned by the later signer
  capability ADRs, not by this boundary ADR.
