---
title: Capability Socket
slug: capability-socket
topic: capability-socket
summary: The capability trampoline routes all non-external_signer namespaces synchronously to a Kotlin handler registered via nativeSetCapabilityHandler; the existing ex
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Capability Socket

## Routing

The capability trampoline routes all non-external_signer namespaces synchronously to a Kotlin handler registered via nativeSetCapabilityHandler; the existing external_signer async channel drain remains unchanged.

<!-- citations: [^78c8e-17] [^2e544-2] [^2e544-50] [^78c8e-43] -->
## Memory Safety

Android JNI local references in the capability trampoline must be scoped via with_local_frame to prevent local-ref-table overflow on permanently-attached threads. The capability_handler mutex (not a non-existent capability-socket quiescence) is the load-bearing UAF guard for teardown safety; the capability socket clones the registration and drops its lock before invoking the callback, so nmp_app_set_capability_callback(None) does not quiesce in-flight dispatches. The JNI trampoline uses modified-UTF-8 for strings (new_string/get_string); this is safe for current keyring payloads (ASCII base64/account-id) but would corrupt supplementary-plane characters if reused for richer payloads.

<!-- citations: [^78c8e-18] [^78c8e-44] [^78c8e-98] -->
## Initialization Order

iOS capability handler registration (KernelModel.swift:266) must precede restoreChirpIdentity (:344) to ensure the keyring probe succeeds before Marmot registration. <!-- [^78c8e-63] -->

## Visibility Constraints

set_pending_mls_autopublish must remain pub(crate) (not pub); the test exercises the real nmp_app_signin_nsec entry point rather than poking the raw atomic setter directly. <!-- [^78c8e-81] -->

## Error Mapping

CapabilityCredentialStore error mapping ensures no transport/handler failure path maps to NoEntry; only explicit KeyringStatus::NotFound yields NoEntry, all others yield PlatformFailure. <!-- [^78c8e-97] -->
