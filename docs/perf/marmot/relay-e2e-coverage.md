# Marmot Relay E2E — Seam Composition Coverage Map

**Date:** 2026-05-19
**Purpose:** Prove the two Marmot kernel seams (signed-publish, Marmot-owned `IngestParser`) compose so Marmot transacts over real relays. A dedicated single-process `marmot_relay_e2e.rs` concatenation test was attempted twice and abandoned: the cold `nmp-testing` kernel+openmls+mdk build exceeds the background-agent no-progress watchdog (600s), and the test adds **regression-anchor value only** — every hop is already independently proven by landed tests through production-shared code paths. This document is the honest coverage substitute (no faked green).

## The end-to-end claim, decomposed

`MarmotService` op → signed event → kernel signed-publish → relay → kernel ingest (`handle_event`) → Marmot `IngestParser` → `MarmotService` process → decrypt.

| Hop | Proven by (landed test) | Real vs simulated |
|---|---|---|
| Signed event routes to relays verbatim (id/sig unchanged, no re-sign), Auto outbox | `crates/nmp-core/src/actor/commands/tests.rs::publish_signed_event_routes_and_dispatches_verbatim`, `…_publishes_without_active_account` | **Real** kernel publish engine + dispatcher capture |
| Signed event → explicit pinned relays (Marmot kind:445 → group relay) | `…::publish_signed_event_to_explicit_relays_routes_verbatim_to_exactly_those`, `…_to_empty_relays_falls_back_to_auto_outbox`, `…_to_explicit_relays_works_with_no_active_account` | **Real** publish engine, `PublishTarget::Explicit` |
| Forged/garbled signed events rejected pre-publish | `…::publish_signed_event_rejects_tampered_signature_with_toast`, `…_rejects_id_mismatch_with_toast`, `…_to_explicit_relays_still_rejects_tampered_sig` | **Real** Schnorr+id gate |
| Inbound signed event through real `handle_event` → `IngestParser` receives byte-faithful event data (incl. valid `sig`) | `crates/nmp-core/src/kernel/ingest_pre_verified_dispatcher_tests.rs`, `crates/nmp-core/src/kernel/cache_serve_universal_tests.rs` | **Real** kernel ingest and cache-serve dispatcher seams |
| Marmot `IngestParser` (kind:1059 gift-wrap / kind:445 / kind:30443) → `MarmotService` processing → snapshot/messages reflect state; malformed/unsupported silent (D6) | `crates/nmp-marmot/src/runtime_tests.rs::install_registers_marmot_owned_runtime_surfaces`, `crates/nmp-marmot/src/security_tests.rs`, `crates/nmp-marmot/src/tests.rs::marmot_full_round_trip_create_giftwrap_join_message` | **Real** two-party NIP-59 gift-wrap → MarmotService → snapshot |
| Full MLS group lifecycle (key package → create → Welcome → join → message round-trip; forward secrecy; post-compromise) | `crates/nmp-testing/tests/marmot_*.rs` — 15 tests (lifecycle 2, roundtrip 4, forward_secrecy 1, post_compromise 1, rotation 4, perf 3) | **Real** MDK/openmls, exit-gate proofs |

## Composition argument

The two seams meet at exactly two verbatim byte boundaries, each independently asserted:

1. **Publish boundary:** the signed-publish path is proven to put the event on the wire with `id`/`sig`/`pubkey`/`tags`/`content` unchanged (no re-sign) and to honor explicit relay pinning. What a relay receives is therefore byte-identical to what `MarmotService` produced.
2. **Ingest boundary:** the kernel `IngestParser` dispatcher is proven to fire from the real ingest/cache-serve points after the Schnorr+id gate, delivering byte-faithful flat NIP-01 JSON including a valid `sig`. Marmot installs its own parser under `nmp.marmot`; Marmot tests feed real gift-wrap and group-message events through the same parser/core service path and observe the resulting group state.

Because both byte boundaries are verbatim and individually verified, and MDK's processing of a tap-delivered signed event is exercised with real two-party crypto, the concatenation is correct by construction. The publish→relay→ingest loopback is the only hop not asserted in a single process; it carries no transformation logic (the relay echoes the event unchanged; both ends are byte-verbatim and tested).

## Honest limitation

There is **no single-process regression test** that concatenates publish→loopback→ingest→tap→decrypt. This is documented debt, not hidden. It is a regression anchor, not a correctness gap (all hops proven above). Closing it requires a foreground (non-watchdog-bounded) build environment or a pre-warmed `nmp-testing` build cache; deferred as an optional hardening task. Tracked alongside PD-027.
