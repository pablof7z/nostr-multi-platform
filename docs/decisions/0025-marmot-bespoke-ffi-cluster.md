# ADR-0025 — Marmot Bespoke FFI Cluster: Named Exception

Date: 2026-05-21
Status: Superseded (write path) — read path retained, write path migrating to `dispatch_action`. See "Retirement plan" below.
Deciders: NMP team

## Update (2026-05-23) — retirement plan

The "permanent, bounded exception" framing has been replaced with a staged
retirement. The substrate-generic [`MlsOpHandler`](../../crates/nmp-core/src/substrate/mls_op_handler.rs)
seam landed in `nmp-core` and proves the architectural pre-condition the
original "stateful handle" argument missed: MLS state can live in the
app crate behind a shared `Arc` slot while the **wire envelope** (the
JSON action body, the same shape `nmp_marmot_dispatch` already speaks)
crosses through the kernel's generic `dispatch_action` seam. Stage
status:

* **PR 1 (landed — this commit).** `nmp_core::substrate::MlsOpHandler` +
  `NmpApp::set_mls_op_handler` + `ActorCommand::DispatchMlsOp` actor arm
  + `MarmotActionModule` registered under `"nmp.marmot"` +
  `MarmotMlsOpHandler` installed by `register_with_keys`. Both paths
  are live: `nmp_app_dispatch_action("nmp.marmot", action_json)`
  routes to the same `ops::dispatch` handler `nmp_marmot_dispatch`
  reaches; the same `MarmotProjection` instance backs both. iOS
  unchanged (still calls `nmp_marmot_dispatch`).
* **PR 2 (next).** iOS `MarmotBridge.swift` migrates each
  `dispatchAsync(...)`/`dispatchFireAndForget(...)` call from
  `nmp_marmot_dispatch(handle, json)` to
  `nmp_app_dispatch_action(app, "nmp.marmot", json)` (one line per
  call site, no payload change). Result handling moves from the
  synchronous `MarmotOpResult` return to the `correlation_id` +
  `register_action_result_observer` push (or a snapshot projection
  keyed by `correlation_id`) for the 5 ops whose returns iOS uses
  (`createGroup`, `invite`, `send`, `leave`, `remove` — the other 5
  are already fire-and-forget).
* **PR 3 (final).** Delete `nmp_marmot_dispatch` from
  `apps/marmot/nmp-app-marmot/src/ffi.rs`, delete the corresponding
  Swift `marmotDispatch` wrapper, retire this ADR. The kept Marmot
  C-ABI symbols (`nmp_marmot_register{,_active}`, `_snapshot`,
  `_group_messages`, `_string_free`, `_unregister`) are read-side /
  stateful-handle lifecycle — they are NOT a `dispatch_action`
  violation; they are kernel-shaped observer/projection registrations.
* **Step 12 (2026-05-25).** With PR 3 landed and the surviving FFI
  cluster sanctioned as kernel-shaped per-app FFI (the same pattern
  Chirp's `nmp_app_chirp_*` cluster uses), the `nmp-marmot` crate
  returned from `apps/marmot/nmp-app-marmot/` to `crates/nmp-marmot/`
  (`docs/architecture/crate-boundaries.md` step 12, Path B). The
  raw-nsec slot below is unchanged; the D13 part-B path-scope check
  in `crates/nmp-testing/bin/doctrine-lint/rules/d13.rs` already
  exempts `crates/nmp-marmot/`, so the per-line
  `// doctrine-allow: D13` opt-out at the `mls_local_nsec()` call
  site was removed in the relocation.

The original `mls_local_nsec` raw-nsec slot (the *secondary*
ADR-0025 exception) is unaffected by this retirement: it is a
read-only credential seam, not a write seam, and the "NIP-17 must
not copy this pattern" hard limit stays in force.

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
- This ADR does not cover Chirp's `nmp_app_chirp_identity_*` symbols. Those
  are app-owned wrappers in `nmp-app-chirp`, and reusable Marmot code receives
  a caller-supplied keyring account id instead of hardcoding Chirp policy.

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
