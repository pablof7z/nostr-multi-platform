# ADR-0019 — Failed NIP-42 AUTH is fail-closed (withhold gated REQs, do not downgrade)

**Date:** 2026-05-18
**Status:** Accepted (T76 — NIP-42 failed-auth fail-closed semantics)
**Doctrines invoked:** D3 (outbox — write/read to the relays the protocol
designates, not a silent fallback), D6 (no panics, errors never cross FFI),
D7 (capabilities report; the kernel decides policy), D8 (diagnostic
re-emit, not a rev bump)

## Context

NIP-42 lets a relay demand client authentication before serving (or
accepting) data. The kernel's per-relay handshake FSM
(`crates/nmp-core/src/kernel/auth.rs::Nip42DriverState`, mirrored by the
standalone `nmp-nip42` crate) reaches `RelayAuthState::Failed` when:

- the bound signer refuses to sign or errors,
- the signer returns a structurally invalid kind:22242,
- the relay rejects our AUTH event (`["OK", <id>, false, <reason>]`), or
- (operationally) the AUTH attempt never completes before the relay drops.

Two independent gates partition outbound REQs against this state:

1. `crates/nmp-core/src/subs/auth_gate.rs::AuthGate` — the M11
   `LogicalInterest` lifecycle path; buffers REQs per relay URL.
2. `crates/nmp-core/src/kernel/requests/mod.rs::partition_auth_paused` —
   the M1 hand-rolled kernel path; defers into the shared 64-slot
   `deferred_outbound` ring.

**Before T76 both gates treated `Failed` as pass-through.** The recorded
rationale was: "the actor/operator owns the resolution path (D7) and the
buffer would otherwise grow without bound." The consequence: when an
AUTH-required relay rejected AUTH, the gated REQ was emitted to that relay
**anyway** — a silent downgrade to an unauthenticated read attempt. For a
private/subscriber relay (the M5 demo target — a paid nostr.wine
subscription) this leaks the subscription's filter shape to a relay that
will not serve it, and presents as a confusing partial-data state rather
than an honest failure. It is the textbook fail-open defect.

## Decision

A relay in `Failed` is **fail-closed**:

1. **Withhold** — `is_paused` / `relay_auth_paused` now include `Failed`.
   AUTH-gated REQs for that relay are never emitted to the wire while it
   is `Failed`. No silent unauthenticated downgrade.

2. **Drop, do not buffer** — the original unbounded-buffer concern is
   answered head-on: REQs to a `Failed` relay are *discarded*, not queued.
   - `AuthGate::partition` drops REQs to a failed relay (distinct branch
     from the transient-pause buffer branch).
   - `partition_auth_paused` drops (does not `defer_outbound_silent`) REQs
     to a failed relay — the shared ring has no per-relay segregation, so
     deferring would either leak on the next non-paused drain or crowd
     the 64-slot ring.
   - The transition **into** `Failed` purges already-held REQs for that
     relay: `AuthGate::record_transition(.., Failed)` removes the relay's
     pending buffer; the kernel calls `purge_deferred_reqs_for(role)` at
     every Failed transition site (signer-invalid, signer-failed,
     relay-rejected). CLOSEs and other relays' messages are retained.

3. **Per-relay isolation** — both gates key on relay URL / `RelayRole`.
   A `Failed` relay never affects REQ flow to any other relay. This is
   pinned by `failed_relay_does_not_affect_other_relays` and the kernel
   integration test's healthy-Indexer assertion.

4. **Surface it** — `RelayStatus.auth = "failed"` and
   `last_error = <reason>` already flow via `update_relay_auth_status`
   (`changed_since_emit = true`, D8 re-emit; the rev bump stays with
   `make_update`, unchanged by T76).

5. **Recovery is reconnect-only** — no new retry plumbing. A relay
   reconnect (`relay_connected` / `relay_closed`) already calls
   `reset_on_disconnect`, returning the driver to `NotRequired`. The
   relay re-sends a fresh challenge and the handshake restarts cleanly;
   the post-reconnect recompile (M11) re-walks the interests, naturally
   reissuing the REQs that were dropped. Dropped REQs are *not*
   resurrected by a later in-session `Authenticated` — pinned by
   `failed_relay_drops_reqs_not_buffered`.

## Why drop instead of keep-and-retry

Keeping a per-relay `Failed` buffer reintroduces the exact unbounded-growth
problem the original fail-open comment cited: a relay can stay `Failed`
indefinitely (wrong signer, revoked subscription), and every recompile
would re-add REQs to a buffer that never drains. Dropping is sound because
the interest set is durable elsewhere — the lifecycle/registry re-derives
the REQs on the next recompile, and the only recompile that matters here
(reconnect) is precisely when the relay becomes usable again. We trade a
bounded amount of redundant recompile work for a strictly bounded buffer
and zero leak surface. This is the non-obvious part of the decision and
the one a future contributor is most likely to question before
re-introducing fail-open — it is deliberate.

## Consequences

- **Positive:** no unauthenticated downgrade; no filter-shape leak to a
  relay that will not serve the sub; honest `failed` diagnostic instead of
  silent partial data; bounded buffers; other relays unaffected; no new
  retry machinery.
- **Negative:** a transient AUTH failure that would have *happened* to
  succeed on an unauthenticated retry now surfaces as `Failed` until
  reconnect. This is correct for AUTH-*required* relays (the spec contract)
  and acceptable for the rare misconfigured relay — the operator sees the
  diagnostic and the reconnect path recovers.
- **Scope:** kernel-side only. The iOS signer binding (PD-005 / T59)
  remains the open part of the broader NIP-42 story; this ADR does not
  change it.

## Alternatives considered

- **Keep fail-open (status quo):** rejected — silent unauthenticated
  downgrade violates D3 and the NIP-42 contract.
- **Buffer-and-replay on Failed:** rejected — unbounded buffer growth for
  a persistently-failing relay; replay target is unauthenticated anyway
  until a fresh challenge, which only arrives on reconnect (where the
  recompile already reissues).
- **Take the relay fully offline (mark dead):** rejected — too broad;
  `Failed` is AUTH-scoped, the socket may still be useful for CLOSE
  bookkeeping, and other-relay isolation is cleaner expressed at the gate.

## Validation

`cargo test -p nmp-core` — new regressions:

- `subs::auth_gate::tests::failed_relay_withholds_reqs_fail_closed`
- `subs::auth_gate::tests::failed_relay_does_not_affect_other_relays`
- `subs::auth_gate::tests::failed_relay_drops_reqs_not_buffered`
- `subs::auth_gate::tests::transition_into_failed_drops_existing_pending_buffer`
- `kernel::auth_tests::nip42_kernel_failed_auth_fails_closed` (spec'd:
  reject → gated REQs withheld + prior deferred purged, healthy relay
  unaffected, status reflects it, reconnect resets)

Test 4 (`nip42_kernel_publish_retry_on_auth_required`) updated: its
Failed-window assertion now pins fail-closed, replacing the prior
pass-through assertion.
