---
title: Keychain Usage
slug: keychain
summary: The OS keychain (iOS/macOS Keychain, Android Keystore) is used as the platform's secure secret store for the Nostr Multi-Platform project, accessed through a platform-agnostic keyring capability interface with a mock fallback.
tags:
  - keychain
  - keyring
  - capability
  - FFI
  - security
volatility: warm
confidence: low
created: 2026-05-29
updated: 2026-05-29
verified: 
compiled-from: codebase
sources:
  - codebase
---

The OS keychain is the primary hardware-backed secret store on Apple platforms, abstracted behind a kernel‑side `KeyringCapability` contract. The kernel never couples to a specific store; it only issues typed JSON requests (`store`, `retrieve`, `delete` keyed by an opaque `account_id`) and consumes typed `KeyringResult` responses (nmp‑core/src/substrate/keyring.rs:4‑8,25‑27). Doctrine **D6** guarantees that every failure is data inside the envelope, never an exception (keyring.rs:14‑16). Doctrine **D7** separates policy: the identity layer decides *which* account is active and *when* to persist/forget; the capability only *executes* the store and *reports* the result (keyring.rs:18).

### How the native keychain is wired
* The FFI capability socket (`nmp‑ffi` crate) registers a callback—on iOS this is `KeychainCapability.handleJSON(_:)`—that speaks the exact JSON vocabulary of the `KeyringRequest`/`KeyringResult` types (nmp‑ffi/src/capability.rs:5‑7,89).
* `nmp‑marmot::initialize()` probes the real Keychain first on Apple platforms. If it succeeds, `use_mock = false`; if the Keychain is unavailable (e.g., missing entitlement), it falls back to an in‑memory mock store and sets a runtime flag (`keyring_unavailable`) so the UI can warn the user—the system never silently switches from real to mock after initialization (nmp‑marmot/src/ffi.rs:227‑233,255‑257).
* On non‑Apple platforms the mock store is always used (nmp‑marmot/src/ffi.rs:245‑247).

### Identity session persistence
The identity layer uses the keyring to persist enough material to restore the active signing session on next launch. Concrete keyring accounts include:
- `ACTIVE_ACCOUNT_ID` (`nmp.identity.active.id`) and `ACTIVE_SIGNER_KIND_ID` (`nmp.identity.active.kind`) to remember which signer was active.
- For a local nsec: `nmp.identity.local_nsec.<pubkey>`.
- For a NIP‑46 remote bunker: `nmp.identity.remote_payload.<pubkey>`.
All persistence and restoration go through `run_keyring()` → `dispatch_capability()` → the native handler (nmp‑core/src/actor/session_persistence.rs:5‑6,25‑35,45‑50,70‑75,120‑130).

### Mock and test harness
The `nmp‑ffi` test suite provides a mock native handler that stores secrets in an in‑memory `HashMap`, mirroring the real Keychain’s behavior for `store`/`retrieve`/`delete` and covering error paths like a missing handler (null return) or a malformed request (nmp‑ffi/src/capability.rs:101‑110,117‑145). NIP‑42 handshake tests surface “keychain locked” as a signer failure, verifying that signer errors propagate without dispatching a wire frame (nmp‑nip42/tests/flow.rs:101‑104; nmp‑nip42/src/flow.rs:164).
