---
scenario: 4-nip42-auth
verdict: PASS
generated_at: 1779089109
relays: ["wss://auth.nostr1.com"]
---

# Scenario 4 — NIP-42 relay AUTH challenge/response

## Verdict: PASS

Relay `wss://auth.nostr1.com` returned `["AUTH", <challenge>]` in response to a `kinds:[1] limit:1` REQ. We parsed the challenge, generated a fresh ephemeral key, built the kind:22242 AUTH event (`["relay","wss://auth.nostr1.com"]` + `["challenge",…]` tags, empty content), signed it with `LocalKeySigner`, and sent `["AUTH", <event>]`.

The relay acknowledged with `OK=true` for our auth event id.

- relay: `wss://auth.nostr1.com`
- challenge: `7c872349-00f4-469f-9595-2056faab8db7`
- auth event id: `31c515bcc4f552561d3b4e60de5cc2fd4a054eb86dd238c6540e2ff7ab640a79`
- auth pubkey (ephemeral): `61225afb0d34da8776927369e43efabd5c8acbb276fb782935f8a95fd403b00d`
- OK accepted: `true`
- OK reason: ``

Proves the NIP-42 challenge/response handshake works against a relay that genuinely requires it.

## Known limitation

The AUTH wire format (kind:22242 builder, `["AUTH", {event}]` frame, OK
parser) is **inlined as a consumer copy** in `real_relay_nip42.rs` rather
than imported from `nmp-nip42`, because `nmp-nip42` is not currently a
dev-dependency of `nmp-testing` and `Cargo.toml` was outside this session's
file-disjoint territory. Live wire behaviour is genuinely validated; the
`nmp_nip42::{build_auth_event, wire_frame_for, parse_auth_frame,
parse_ok_frame}` Rust API is not directly exercised by this test. A
follow-up should add `nmp-nip42` to `crates/nmp-testing/Cargo.toml`'s
dev-dependencies and replace the inlined helpers with the crate's public
surface so a refactor breaking that API is caught here.
