# Codex review — #1493 P9 PR1b (nostrconnect perms app-supplied)

Date: 2026-06-19. codex (gpt-5-codex). Branch: fix/1493-p9-nostrconnect-perms.

Made the nostrconnect:// NIP-46 perms app-supplied (was hardcoded sign_event:1,7 in broker/nostrconnect.rs). New NostrConnectPermsSlot (Arc<Mutex<Option<String>>>, mirrors bootstrap-relay), AppHost setter, FFI getter/setter, broker takes perms: Option<String>, NmpDefaults default None, Chirp supplies from nmp-chirp-config via nmp_app_chirp_register.

## Codex verdict: NO findings.
- None omits &perms= entirely; Some("sign_event:1,sign_event:7") → &perms=sign_event%3A1%2Csign_event%3A7 appended after relay/secret/name, no double & (nostrconnect.rs:48).
- FFI reads Option<String>, lock failure → None, no panic (relay_config.rs:96, signer_broker.rs:148).
- NmpDefaults nostrconnect_perms None default, wired only when Some, bootstrap-relay wiring undisturbed (tiers.rs:149, lib.rs:274).
- All start_nostrconnect_handshake callers updated to 2-arg.
- D14: Arc<Mutex<Option<String>>> (allowed), not Vec (slots.rs:360).

Verified locally: build (nmp-core/ffi/signer-broker/defaults/chirp-config/app-chirp) clean; nmp-signer-broker 46; nmp-defaults; nmp-app-chirp green (#1553); doctrine_lint_smoke 78; ffi-header-drift OK (73 symbols, no new C-ABI); file-size clean (register.rs trimmed to 499).
