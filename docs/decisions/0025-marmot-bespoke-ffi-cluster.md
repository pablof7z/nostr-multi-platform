# ADR-0025 — Marmot Bespoke FFI Cluster: Named Exception

Date: 2026-05-21
Status: Superseded (write path retired; read/lifecycle FFI cluster retained).
Deciders: NMP team

## Outcome

The write-path retirement that earlier revisions of this ADR staged is **done**.
The bespoke write-dispatch envelope `nmp_marmot_dispatch` was deleted, the Swift
`marmotDispatch` wrapper was deleted, and Marmot mutating ops now flow through
the generic `nmp_app_dispatch_action("nmp.marmot", …)` seam (the `MlsOpHandler`
keeps handle-scoped MLS state in the crate behind a shared `Arc` slot while only
the JSON wire envelope crosses the kernel). The `nmp-marmot` crate was relocated
to `crates/nmp-marmot/`.

What this ADR now sanctions is the **surviving read/lifecycle FFI cluster** plus
one credential slot — the parts that are not a `dispatch_action` violation.

## Retained: the read/lifecycle FFI cluster

The kept native-facing Marmot C-ABI symbols (`nmp_marmot_register_active`,
`nmp_marmot_unregister`) are **not** a second action-dispatch path. They are
kernel-shaped observer / projection / opaque-handle lifecycle registrations —
the same pattern Chirp's `nmp_app_chirp_*` cluster uses. They are sanctioned
because MLS groups carry handle-scoped cryptographic state
(`nmp_marmot::mls::GroupHandle`) that must live in a typed Rust handle and
cannot survive serialization through a stateless `dispatch_action` payload.

Hard limit: the cluster **must not grow** with new feature symbols. Any new
Marmot capability that does not need handle-scoped crypto state MUST route
through `dispatch_action`.

### #1727 normalization — secret material no longer crosses the ABI

Two surfaces were normalized away in #1727; **no native-facing `nmp_marmot_*`
symbol carries secret key material**:

- The pull symbols `_snapshot`, `_group_messages`, `_string_free` were already
  deleted in V-107 (ADR-0039); Swift reads Marmot state reactively from the
  `nmp.marmot.snapshot` / `nmp.marmot.messages` push projections.
- The secret-bearing `nmp_marmot_register(app, secret_key_hex, …)` C symbol —
  which native code never called — was demoted to a plain Rust function
  (`nmp_marmot::ffi::register_with_secret_hex`, not `extern "C"`) and its
  `NmpCore.h` declaration was removed. It survives ONLY as an in-process Rust
  entry point for the nsec sign-in path: `nmp_app_signin_nsec` enqueues
  `AddSigner` **asynchronously**, so the `mls_local_nsec` slot is not yet
  populated when registration must run synchronously on the same call;
  `register_with_secret_hex` reuses the in-hand secret to avoid that race. The
  secret never re-crosses the C/JNI ABI — the app-shell already holds it from
  the sign-in import. Native-facing registration is `register_active` only.
- The bespoke `nmp_marmot_fetch_key_packages(handle, pubkeys_json)` C symbol —
  also never called by native — was deleted. The same key-package lookup
  interest is pushed internally by the invite/group flow
  (`projection::state` / `projection::pending`), so the fetch is already hidden
  inside those flows per the #1727 target shape.

## Retained: the raw-nsec slot

Marmot's MLS layer needs the active account's raw secret key (`nostr::Keys`)
to drive the OpenMLS credential. To keep that key Rust-owned (D0 — Swift never
sees it on the `createAccount` path), `NmpApp` carries a dedicated slot:

- **`NmpApp::mls_local_nsec: Arc<Mutex<Option<Zeroizing<String>>>>`** — the
  active local account's `nsec1…` in bech32 form, written by the actor after
  every identity mutation, read by `nmp_marmot_register_active` via the
  `NmpApp::mls_local_nsec()` accessor.

This slot is part of the retained exception (it is a read-only credential seam,
not a write seam, so the write-path retirement does not touch it). Hard limits:

- The slot is named `mls_local_nsec` (describing the MLS protocol purpose, not
  the Marmot consumer — D0 forbids app nouns at the substrate level). The D13
  doctrine-lint's part-B path-scope check enforces that only `crates/nmp-marmot/`
  may call `mls_local_nsec()`.
- **NIP-17 DMs must NOT read this slot.** DM gift-wrapping also needs signer
  access, but it must go through a dedicated `ActorCommand::SendGiftWrappedDm`
  kernel command — never by reading `mls_local_nsec` directly. This limit
  stays in force.

## Consequences

- The surviving read/lifecycle Marmot cluster + the raw-nsec slot remain
  sanctioned; the write path is gone.
- Future feature work touching Marmot must justify any new `nmp_marmot_*`
  symbol against this ADR before landing — mutating ops go through
  `dispatch_action`, not a new bespoke symbol.
- The `dispatch_action` namespace census now *includes* Marmot mutating ops
  (they route through `nmp.marmot`); only the read/lifecycle handle symbols
  remain outside it.
