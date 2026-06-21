# ADR-0041: F-TTL FFI surface — `force` argument on the claim functions

Status: ACCEPTED

> Numbering note: the F-TTL task draft referenced "ADR-0013", but `0013` is
> already taken (`0013-nip29-metadata-signer-trust-model.md`) and the decisions
> log has advanced to 0040. This stub takes the next free number, 0041, per the
> repo's single-source-of-truth / no-duplicate-id discipline.

## Context

F-TTL requires a force-refresh entry point so a host can say "treat this
replaceable identity as due now" (e.g. the user explicitly opens a profile /
article, or pulls to refresh).

The T-D commit first added a **new** symbol `nmp_app_refresh_replaceable` to the
C-ABI (`crates/nmp-ffi/src/timeline.rs`). That approach failed two CI gates:

- **ffi-surface-freeze** — the seam-migration doctrine (ADR-0027 direction)
  freezes the per-verb `nmp_app_*` C-ABI; a genuinely new symbol requires an
  exemption and widens the hand-written Swift-mirror surface.
- **ffi-drift** — the symbol was exported from Rust but never declared in
  `NmpCore.h`, so the header/Rust symbol sets diverged.

## Decision

Do **not** add a new symbol. Expose force-refresh as a trailing `force` argument
on the two existing claim functions:

- `nmp_app_claim_profile(app, pubkey, consumer_id, force: c_int)` — kind:0 profile.
- `nmp_app_claim_event(app, uri, consumer_id, force: c_int)` — `naddr` addressable
  identities; a silent no-op for immutable `nevent`/`note` URIs (no TTL record).

`force` (`force != 0`) propagates as `force: bool` through
`ActorCommand::ClaimProfile`/`ClaimEvent`, `KernelReducer`/`Kernel::claim_profile`
/`claim_event`, and into `Kernel::claim_replaceable(kind, pubkey, d_tag?, force)`.
When `force == true` the kernel treats the stored `check_again_after` as `0`, so
the TTL gate always reads as due and enqueues a re-verification REQ — semantically
identical to what the deleted `nmp_app_refresh_replaceable` did. When
`force == false` (the default) the claim re-verifies lazily, only when the TTL has
elapsed.

The gate runs in the **cached/known** branch of each claim function: an
already-cached replaceable identity is re-verified per the TTL (or unconditionally
when forced), while a cold/unknown identity issues its normal one-shot fetch and
is not double-fetched.

## Consequences

- iOS/Android callers gain an explicit profile/article refresh trigger
  (pass `force = 1`) without any new C-ABI symbol — the surface stays frozen.
- The Swift bridges expose `force: Bool = false`, so every background /
  `.onAppear` caller is unchanged and passes `0` implicitly.
- No ABI break: modifying a function signature is invisible to the name-based
  ffi-drift / surface-freeze gates; `NmpCore.h` is updated to the 4-arg form.
- Force routes through the existing TTL machinery (`claim_replaceable` /
  `pending_reverify`), inheriting the in-flight guard and EOSE re-stamp; it does
  not open a second, parallel refresh path.
- `nmp_app_refresh_replaceable` (symbol, `ActorCommand::RefreshReplaceable`
  variant, `KernelReducer::refresh_replaceable`) is deleted.
