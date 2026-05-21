# ADR-0023 — `HttpCapability` over the synchronous capability socket

**Date:** 2026-05-21
**Status:** Accepted (NIP-57 LNURL HTTP leg — capability definition + iOS impl)
**Doctrines invoked:** D0 (no app nouns — the capability carries only generic
HTTP primitives), D6 (no panics — every failure is `HttpResult::Error` data),
D7 (a capability reports and executes; the kernel decides which URL to call),
D8 (the synchronous call blocks the actor thread — a deliberate, documented
trade-off)

## Context

NIP-57 zaps have two legs. Leg 1 — the kind:9734 zap-request event — is a
Nostr event; `nmp-nip57`'s `ZapModule` already owns it and the executor
publishes it to relays. Leg 2 — the LNURL-pay round-trip — is an **HTTP**
exchange:

1. `GET {lnurl}` → JSON with a `callback` URL and `minSendable`/`maxSendable`.
2. `POST {callback}?amount={msats}&nostr={url-encoded signed kind:9734}` →
   `{"pr":"lnbc…"}`, a bolt11 invoice.
3. The wallet pays the invoice; the LN provider publishes the kind:9735
   receipt.

The kernel had **no HTTP transport**. The actor knows how to open relay
WebSockets, not how to make an HTTP request. So leg 2 was unbuilt and zaps
could not complete — `ZapModule` was a scaffold that validated and carried the
`lnurl` field but dispatched it nowhere.

The kernel already has one host-injected capability: `KeyringCapability`
(`nmp.keyring.capability`), a `CapabilityModule` whose typed request/result
vocabulary rides the generic `CapabilityRequest`/`CapabilityEnvelope` envelopes
through the FFI capability socket (`crates/nmp-core/src/capability_socket.rs`).
The platform (iOS Keychain) supplies the implementation; the kernel decides
policy. That pattern is proven.

The capability socket is **synchronous**: `dispatch_capability` invokes the
registered native C callback and blocks the calling thread until it returns a
result envelope. For the keyring this is fine — a Keychain read is sub-
millisecond. For HTTP it means the **actor thread blocks for the full duration
of the HTTP call** (~500ms typical for an LNURL endpoint).

## Decision

Add `HttpCapability` (`nmp.http.capability`) as the **second
`CapabilityModule`**, following the `KeyringCapability` shape exactly:

- `crates/nmp-core/src/substrate/http.rs` defines the typed marker, the
  `HttpRequest` (method/url/headers/body) and `HttpResult`
  (`Ok { status_code, body }` | `Error { message }`) vocabulary, and an
  `HttpCapabilityWiring` helper that builds the generic `CapabilityRequest`
  envelopes (`get` / `post` / `decode_result`).
- The iOS implementation (`ios/Chirp/Chirp/Capabilities/HttpCapability.swift`)
  is `URLSession`-backed. There is a single C capability callback
  (`nmp_app_set_capability_callback`); `ChirpCapabilities.handleJSON` routes a
  request to the keyring or HTTP capability by its `namespace` field.

It rides the **existing synchronous socket** — **Option A**. No new C symbols,
no change to the `CapabilityModule` trait, no change to `capability_socket.rs`.

### Why Option A (synchronous socket) — and the rejected alternatives

- **Option A — host HTTP over the synchronous capability socket (chosen).**
  The platform owns the transport (iOS `URLSession`, desktop `reqwest`); the
  kernel owns policy (which URL, what to do with the bolt11). It reuses the
  proven `KeyringCapability` mechanism wholesale. The cost — a blocked actor
  thread — is bounded and acceptable for the use case (see below).

- **Option B — iOS owns the entire LNURL-pay flow.** Rejected: it pushes
  protocol logic (LNURL JSON parsing, msat math, kind:9734 URL-encoding) into
  Swift. That violates D0/D7 and the Chirp thin-shell rule, and it would have
  to be re-implemented per platform. The kernel must own the zap *policy*; the
  host must own only the transport.

- **Option C — spawn a blocking worker thread inside the kernel and make HTTP
  calls with `reqwest` directly.** Rejected for v1: it puts a TLS/HTTP stack
  and its dependency surface inside `nmp-core`, and it still needs an
  async-result delivery path back to the actor — i.e. it is the non-blocking
  variant below with extra baggage. The capability seam already exists; use it.

### The known limitation — actor thread stalls during the HTTP call

Because the socket is synchronous, `dispatch_capability` for an HTTP request
**blocks the actor thread** until the HTTP round-trip completes. While blocked,
the actor processes no other `ActorCommand`, emits no snapshot tick, and
services no relay frame.

This is an **acceptable MVP trade-off** for NIP-57 zaps specifically:

- A zap is a **rare, explicit user action** — not a hot path.
- A ~500ms stall is **not a visible UI freeze**: the UI runs on the main
  thread, the actor on its own thread; the snapshot the UI last rendered stays
  on screen. The user sees a spinner, not a frozen app.
- The iOS implementation caps each call with a `URLSession` request timeout
  (20s) plus a `DispatchSemaphore` backstop, so a dead endpoint cannot stall
  the actor indefinitely — it returns an `HttpResult::Error` (D6).

It is **not** acceptable as a general-purpose HTTP path. A capability used on a
hot path, or for many concurrent calls, must not block the actor.

### Path to a non-blocking variant (future work)

The non-blocking design is fire-and-forget with an async result delivery:

1. `dispatch_capability` (or a new `dispatch_capability_async`) hands the
   request to the host and returns immediately — the actor thread is not
   blocked.
2. The host performs the HTTP call off-thread and, on completion, calls a new
   C symbol — `nmp_app_deliver_capability_result(app, envelope_json)` — which
   enqueues the result `CapabilityEnvelope` as an `ActorCommand` back onto the
   actor.
3. The issuing protocol module correlates the delivered result by
   `correlation_id` and resumes its multi-step plan.

That requires (a) a new C symbol, (b) a result-routing `ActorCommand` variant,
and (c) the protocol module modelling its execution as a resumable multi-step
state machine rather than a single synchronous call. It is deliberately
**out of scope** for this PR — the synchronous socket is sufficient to unblock
zaps now.

### ZapModule executor wiring — deferred, and why

This PR defines `HttpCapability` and ships the iOS implementation, so the
LNURL **transport** is unblocked. It does **not** wire `ZapModule`'s executor
through it. The action-registry executor closure has the signature
`Fn(&str, &str, &dyn Fn(ActorCommand)) -> Result<(), String>` — it receives
the action JSON, the correlation id, and a `send` callback, but **no handle to
the kernel's capability slot**. The slot (`CapabilityCallbackSlot`), the
`capability_socket` module, and `NmpApp::dispatch_capability` are all
`pub(crate)` / `&self`-bound — unreachable from a `'static` executor closure
registered by the `nmp-app-chirp` crate.

Reaching the slot from the executor would mean threading a capability handle
through the action-registry machinery — a larger structural change than this
seam warrants. Per the task scope, that is **explicitly deferred**: the
executor remains the kind:9734-only scaffold; wiring it through
`HttpCapabilityWiring::get`/`post` is a follow-up.

The multi-step execution, once wired, would be: the executor (with capability
access) issues `HttpCapabilityWiring::get(lnurl)`, parses the lnurl-pay
response for the `callback` URL, issues `HttpCapabilityWiring::post(callback,
signed_kind9734, "application/json")`, and yields the returned bolt11 invoice
as a `correlation_id`-keyed result for the wallet action. With the synchronous
socket each GET/POST blocks in place; with the non-blocking variant each step
resumes on result delivery.

### Precondition for `ZapsDomain` wiring — description-hash verification

Independent of the executor wiring above, the **inbound** side (`ZapsDomain`
consuming kind:9735 receipts) has a precondition: NIP-57 requires the kind:9735
receipt's `description` tag to contain the original kind:9734 request, and the
bolt11 invoice's `description_hash` to be the SHA-256 of that description.
`crates/nmp-nip57/src/decode.rs` currently parses the `description` tag but does
**not** verify the description hash. Before `ZapsDomain` is wired to drive
zap-total UI from receipts, `decode.rs` must add that verification — an
unverified receipt could otherwise inflate a zap total with a forged amount.
That work is **not part of this PR**; it is recorded here as the gating
precondition for the inbound zap-aggregation feature.

## Consequences

- **Positive:** the LNURL HTTP transport exists, reusing the proven
  `CapabilityModule` mechanism — no new C symbols, no trait change, no
  `capability_socket.rs` change. iOS has a working `URLSession` implementation.
  The zap feature is unblocked at the transport layer.
- **Negative:** an HTTP call blocks the actor thread for its duration (~500ms
  for a zap). Acceptable for rare user-triggered actions; unacceptable for hot
  paths — the non-blocking variant above is the documented escape hatch.
- **Scope:** this PR ships the capability definition (Rust) + iOS
  implementation + this ADR. `ZapModule`'s executor wiring through the
  capability and `decode.rs` description-hash verification are both follow-ups.

## Validation

`cargo test -p nmp-core` — new regressions in
`crates/nmp-core/src/substrate/http.rs`:

- `get_builds_correct_request_json` — `HttpCapabilityWiring::get` serialises a
  namespaced GET envelope; no `headers`/`body` keys on the wire.
- `post_includes_content_type_header` — POST carries the `Content-Type` header
  and body.
- `method_serialises_to_uppercase` — `HttpMethod` wire values are `"GET"` /
  `"POST"` (byte-compatible with the Swift `HttpMethod`).
- `decode_ok_result` / `decode_error_result` / `ok_and_error_round_trip_through_serde`
  — `HttpResult` round-trips through serde.
- `decode_malformed_envelope_reports_error` — D6: a non-JSON `result_json`
  surfaces as `HttpResult::Error`, never an exception.

iOS: `HttpCapability` + namespace routing in `ChirpCapabilities` are exercised
through the existing single C capability callback; no new FFI symbol.
