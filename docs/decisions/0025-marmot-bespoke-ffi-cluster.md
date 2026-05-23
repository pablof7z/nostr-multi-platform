# ADR-0025 — Marmot Bespoke FFI Cluster: Named Exception

Date: 2026-05-21  
Status: Accepted  
Deciders: NMP team

## Context

`apps/chirp/nmp-app-chirp/src/marmot/` exposes a second C-ABI cluster —
`nmp_marmot_{register,snapshot,group_messages,dispatch,...}` — that
operates in parallel with the generic `nmp_app_dispatch_action` /
`nmp_app_register_snapshot_projection` seam. One symbol in particular,
`nmp_marmot_dispatch`, is a second action-dispatch envelope: it parses
an "op envelope" JSON and routes it through `nmp_marmot::projection::ops::dispatch`
rather than through the `ActionRegistry`.

This is a deviation from the kernel's single-entry-point design principle.
Direction reviews #36, #37, and #38 identified it as "silent seam erosion" and
requested this ADR to name the exception explicitly.

## Decision

The Marmot FFI cluster is a **permanent, bounded exception** to the generic
`dispatch_action` seam, for the following reasons:

1. **Stateful group handle.** MLS groups have handle-scoped cryptographic state
   (`nmp_marmot::mls::GroupHandle`) that must live in a typed Rust handle, not
   survive serialization through a `dispatch_action` JSON payload. The
   generic seam is stateless-by-design; MLS is not.
2. **Pre-existing; not growing.** The cluster existed before the `dispatch_action`
   seam was formalized. It is not new debt; it is bounded legacy.

## Constraints (hard limits on this exception)

- The Marmot FFI cluster **must not grow** with new feature symbols. Any new
  Marmot capability that does not need handle-scoped crypto state MUST be routed
  through `dispatch_action`.
- **NIP-17 DMs must NOT copy this pattern.** The DM-send executor needs signer
  access (gift-wrap requires `nostr::Keys`), but that access must be mediated by
  a new `ActorCommand::SendGiftWrappedDm` kernel command, not by a bespoke
  `nmp_app_chirp_dm_*` cluster.
- This ADR does not cover `nmp_app_chirp_identity_*` symbols; those are a
  separate surface and should be audited independently.

## Bounded exception — the raw-nsec slot

Marmot's MLS layer needs the active account's raw secret key (`nostr::Keys`)
to drive the OpenMLS credential. To keep that key Rust-owned (D0 — Swift never
sees it on the `createAccount` path), `NmpApp` carries a dedicated slot:

- **`NmpApp::mls_local_nsec: Arc<Mutex<Option<Zeroizing<String>>>>`** — the
  active local account's `nsec1…` in bech32 form, written by the actor after
  every identity mutation, read by `nmp_marmot_register` via the
  `NmpApp::mls_local_nsec()` accessor.

This slot is part of the bounded exception. Hard limits:

- The slot is named `mls_local_nsec` (describing the MLS protocol purpose, not
  the Marmot consumer — D0 forbids app nouns at the substrate level). The D13
  doctrine-lint enforces that only `crates/nmp-marmot/` may call `mls_local_nsec()`.
- **NIP-17 DMs must NOT read this slot.** DM gift-wrapping also needs signer
  access, but per the Constraints above it must go through a dedicated
  `ActorCommand::SendGiftWrappedDm` kernel command — never by reading
  `mls_local_nsec` directly.

## Consequences

- The Marmot cluster remains as-is; no migration onto `dispatch_action` is
  planned.
- Future feature work touching Marmot must justify any new `nmp_marmot_*`
  symbol against this ADR before landing.
- The `dispatch_action` namespace census (used by Opus direction reviews) excludes
  Marmot op types by design; that exclusion is now named.
