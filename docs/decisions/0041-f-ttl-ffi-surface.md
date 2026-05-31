# ADR-0041: F-TTL FFI surface — nmp_app_refresh_replaceable

Status: PROPOSED

> Numbering note: the F-TTL task draft referenced "ADR-0013", but `0013` is
> already taken (`0013-nip29-metadata-signer-trust-model.md`) and the decisions
> log has advanced to 0040. This stub takes the next free number, 0041, per the
> repo's single-source-of-truth / no-duplicate-id discipline.

## Context

F-TTL requires a force-refresh entry point on the C-ABI surface. Per the
[FFI Surface Freeze Gate](../wiki/ffi-surface-freeze-gate.md), new symbols must
be approved via an ADR before they are added to the frozen C-ABI.

The T-D commit added `nmp_app_refresh_replaceable` to the C-ABI
(`crates/nmp-ffi/src/timeline.rs`). This ADR records the decision that gates it.

## Decision

Add `nmp_app_refresh_replaceable(NmpApp*, uint32_t kind, const char* pubkey, const char* d_tag_or_null)`
to the C-ABI. Fire-and-forget; it drives the kernel's `refresh_replaceable`
reducer entry point, which enqueues the `(kind, pubkey, d_tag?)` identity for
immediate re-verification (the next `drain_pending_reverify` cycle issues a fresh
REQ). Semantically this is "treat this replaceable identity as due now" —
equivalent to setting `check_again_after = 0` and triggering a re-verify.

## Consequences

- iOS/Android callers gain an explicit profile/article refresh trigger
  (e.g. pull-to-refresh on a profile or long-form article view).
- No ABI break — this is an addition only.
- The symbol routes through the existing TTL machinery (`claim_replaceable` /
  `pending_reverify`), so it inherits the in-flight guard and EOSE re-stamp
  behaviour; it does not open a second, parallel refresh path.
