---
title: NIP-55 External Signer
slug: nip-55-signer
topic: nip-55-signer
summary: NIP-55 (Amber external signer) ADR-0048 places the signer behind the uniform V-78 SignEventForAccount port with a 90-second per-op interactive deadline, pubkey-
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# NIP-55 External Signer

## Architecture & Integration

NIP-55 (Amber external signer) ADR-0048 places the signer behind the uniform V-78 SignEventForAccount port with a 90-second per-op interactive deadline, pubkey-only persistence, and degradation via the existing connection-state pattern. A pubkey-only active_pubkey() accessor replaces active_local_keys() for identity-only consumers (WOT bootstrap, DM relay list, self-zap receipts), enabling bunker accounts. The NIP-55 capability seam uses a registry-handle-id context for in-flight dispatch (not a raw Arc pointer), preventing a teardown use-after-free window where nmp_app_set_capability_callback(None) could free the context while a dispatch is active. HL integrates NIP-55 via UniFFI (not JNI), with a blocking timed drain using recv_timeout and a registry-handle-id context for safe teardown rather than raw Arc pointer. NIP-55 Stage 2 was initially built but unwired (the V-14 pattern): the ExternalSignerCapabilityBridge existed in both apps but was registered/dispatched nowhere, and no kernel↔host capability seam existed. The ExternalSignerCapability bridge dispatches Intent vs ContentResolver based on the host's granted_permissions, with Kotlin reporting raw results (per D7 doctrine that the host decides nothing about signing).

<!-- citations: [^da6b1-18] [^da6b1-47] [^da6b1-71] [^da6b1-84] [^2e544-451] -->
## Timeouts & Deadlines

The per-op sign timeout for NIP-55 interactive approval is 90 seconds, sourced from a per-op property (RemoteSignerHandle::sign_timeout()) rather than a bumped global PENDING_SIGN_TIMEOUT; NIP-46 and local signers remain at 5 seconds. The named-account deadline uses sign_deadline_for(signer_pubkey) instead of the active account's budget, preventing 5-second timeouts on 90-second NIP-55 operations. The hl app's NIP-55 sign-in deadline was initially 30s (truncating deliberate Amber approvals); the fix mirrors PairBunker end-to-end via pair_nip55 + OpOutcome::Nip55SignIn with credential persisted only after success, and raised the deadline to 310s (OP_DEADLINE_SIGNER_PAIR) matching the interactive-approval timeout.

<!-- citations: [^da6b1-19] [^2e544-30] [^da6b1-48] -->
## Acceptance & Registry Criteria

The Android login-block (gallery canonical, Chirp vendored) flips registry compose status from 'soon' to 'stable' only after an emulator E2E proving sign-in and a kind:1 published through the kernel signed by the Amber key. The registry content.ts login-block bar sentence must not be rewritten to lower the acceptance bar; it must state the original requirement (sign in → publish kind:1 signed by Amber key). The Android login-block should support NIP-55 (tested with Amber on the emulator) per owner request for Android-specific signing.

<!-- citations: [^da6b1-20] [^da6b1-49] [^da6b1-72] [^da6b1-85] -->
## Android Bridge & Intent Contract

Android apps support NIP-55 signing via an ExternalSignerCapabilityBridge that dispatches Intent-based requests to Amber; the bridge is gallery-canonical and vendored identically across consuming apps (Chirp with only the package declaration changed, etc.) enforced by a VendorDriftGateTest. The vendored Kotlin bridge files (ExternalSignerWire.kt, AmberIntentCodec.kt, ExternalSignerCapabilityBridge.kt) must remain byte-identical across gallery canonical and Chirp/web/CLI vendor copies (modulo the package declaration), enforced by a VendorDriftGateTest. The NIP-55 Intent uses a bare nostrsigner: URI with the payload sent in the data URI (encoded with Uri.encode) and type/permissions passed as Intent extras (not URI query params), matching the NIP-55 spec and Amber v6.x's parsing behavior. The NIP-55 `selectAmberResultValue` prefers the `event` extra for sign_event responses (since Rust verifies the complete event) while also handling `result` (signature hex) and `rejected:true`, with regression tests calling the production function in both app suites. Android manifests must include a `<queries>` block with `nostrsigner:` scheme and `com.greenart7c3.nostrsigner` package for API 30+ package visibility; without it, the signer cannot be opened. The gallery Android app registers ExternalSignerCapabilityBridge in MainActivity.onCreate and starts a nip55-drain daemon thread that loops on a blocking timed recv (250ms tick, the exact nmp-android-ffi contract), not polling.

<!-- citations: [^da6b1-21] [^da6b1-50] [^da6b1-73] [^da6b1-86] -->
## DM Handling

DM send through NIP-55 requires zero production code changes because `active_signer_for_seal()` routes any RemoteSignerHandle impl (NIP-46 or NIP-55) through the same nip44 seal seam; DM receive-side decrypt wiring is deferred to V-08/#961 to avoid creating a NIP-55-specific receive path that must later be unified. The bulk-decrypt policy for bunker accounts surfaces errors-as-state (decrypt_state: ok|limited|unavailable + undecrypted_count), never silent no-op.

<!-- citations: [^da6b1-22] [^da6b1-51] [^da6b1-74] [^da6b1-87] [^2e544-452] -->
## Known Issues & Follow-Ups

The `restore_nip55_session` kernel bug (NMP #1238) prevents silent cold-start restore of NIP-55 accounts, causing one Connect dialog per cold start in both hl and podcast-player; every cold start re-prompts one Connect dialog, suspected to be an NMP-side hook-registration init-order issue. hl's SignInNip55 was initially fire-and-forget with nothing completing the login or clearing is_signing_in; the fix mirrors PairBunker end-to-end via pair_nip55 + OpOutcome::Nip55SignIn with credential persisted only after success. The D7 Kotlin-side permissions-format conversion (internal NIP-55 format to Amber's {type,kind} JSON) is triplicated across vendor copies and should move to Rust per D7 as a follow-up, not a blocker.

<!-- citations: [^da6b1-23] [^da6b1-52] [^da6b1-88] -->
