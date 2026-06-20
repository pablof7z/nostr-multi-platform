# ADR-0023 — `HttpCapability` over the synchronous capability socket

**Date:** 2026-05-21
**Status:** Superseded (2026-06-21) — the chosen host-side capability mechanism
was replaced by an **in-kernel HTTP client** for the NIP-57 LNURL leg. See
**Supersession** below.

## Context

NIP-57 zaps have two legs. Leg 1 — the kind:9734 zap-request event — is a
Nostr event; `nmp-nip57` owns it and publishes it to relays. Leg 2 — the
LNURL-pay round-trip — is an **HTTP** exchange:

1. `GET {lnurl}` → JSON with a `callback` URL and `minSendable`/`maxSendable`.
2. `POST {callback}?amount={msats}&nostr={url-encoded signed kind:9734}` →
   `{"pr":"lnbc…"}`, a bolt11 invoice.
3. The wallet pays the invoice; the LN provider publishes the kind:9735
   receipt.

The kernel had **no HTTP transport**. The actor knew how to open relay
WebSockets, not how to make an HTTP request, so leg 2 was unbuilt and zaps
could not complete.

## Original decision (now superseded)

This ADR originally added `HttpCapability` (`nmp.http.capability`) as a second
host-injected `CapabilityModule` alongside `KeyringCapability`: a kernel-side
typed HTTP request/result vocabulary that rode the synchronous capability
socket, with the platform (iOS `URLSession`) supplying the transport. The
rationale was to reuse the proven `KeyringCapability` mechanism without adding a
TLS/HTTP stack to `nmp-core`. The known cost was a **blocked actor thread** for
the duration of each HTTP call (~500ms for a zap), accepted as an MVP trade-off
for a rare, explicit user action.

## Supersession (2026-06-21)

The host-side capability was **abandoned and removed**. NIP-57 now performs the
LNURL HTTP round-trip with an **in-kernel HTTP client** — the very approach
("spawn a blocking worker thread inside the kernel and make HTTP calls
directly") that this ADR originally rejected for v1.

**Live path:** `crates/nmp-nip57/src/lnurl/roundtrip.rs` (`http_get_json`, using
`ureq` behind the `native` feature, on a `std::thread::spawn` worker spawned by
`FetchLnurlInvoiceCommand`). The call is unconditional — there is no fallback to
a host capability. The HTTP leg is bounded by `LNURL_HTTP_TIMEOUT_SECS` and
`LNURL_MAX_RESPONSE_BYTES`, and runs off the actor thread, so it does **not**
stall the actor (the limitation the original synchronous-socket design carried).

Why the reversal held up in practice:

- Keeping the LNURL flow (JSON parsing, msat math, kind:9734 URL-encoding,
  bolt11 amount + description-hash verification) entirely in `nmp-nip57` is the
  correct home for that policy (D0/D7) and avoids per-platform re-implementation.
- A kernel-owned worker thread off-loads the round-trip without blocking the
  actor, so the synchronous-socket stall the original design tolerated is gone.
- The HTTP/TLS dependency surface is contained to the `nmp-nip57` `native`
  feature rather than threaded through a host capability seam.

### What was removed

- **Kernel:** `crates/nmp-core/src/substrate/http.rs` (the `HttpRequest` /
  `HttpResult` vocabulary and `HttpCapabilityWiring`) — no longer exists.
- **iOS host:** `ios/Chirp/Chirp/Capabilities/HttpCapability.swift` (the
  `URLSession`-backed implementation) and the `nmp.http.capability` routing arm
  in `ChirpCapabilities` — deleted. `nmp.http.capability` had **zero** Rust
  dispatchers, so the host code was orphaned. (`KeychainCapability` remains; it
  is unaffected.)

NIP-96 / Blossom / image-upload paths never routed through `nmp.http.capability`
and are unaffected by this change.

## Consequences

- The LNURL HTTP transport lives in the kernel (`nmp-nip57`), off the actor
  thread, with no host capability and no new FFI symbol.
- The capability socket (`crates/nmp-core/src/capability_socket.rs`) and the
  single C capability callback remain — now serving only `KeychainCapability`.
- This ADR is retained as a historical record of the abandoned host-HTTP design;
  the live behaviour is defined by `crates/nmp-nip57/src/lnurl/`.
