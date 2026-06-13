# Keyring threat-model posture (iOS + Android parity)

The keyring capability (`nmp.keyring.capability`) stores the per-account Nostr
`nsec` secret encrypted at rest. The kernel restores identity from it on cold
start — *before any UI exists* — so the key that protects the secret must be
usable without user interaction once the device is unlocked. This document
records the resolved threat-model posture for both shells, kept in parity.

Resolves issue #1201 (Android AES key previously had no device-unlock gate).

## Posture (both platforms)

| Property | iOS | Android |
| --- | --- | --- |
| Storage | Keychain `kSecClassGenericPassword` | AES-256-GCM ciphertext in app-private `SharedPreferences`, key in AndroidKeyStore |
| Device-unlock gate | `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` | `setUnlockedDeviceRequired(true)` (API 28+) |
| Per-use biometric prompt | **No** | **No** (`setUserAuthenticationRequired(false)`) |
| Bound to this device only | Yes (`...ThisDeviceOnly`, never iCloud-synced) | Yes (AndroidKeyStore key is non-exportable, never leaves the device) |
| Hardware backing | Secure Enclave (where present) | StrongBox preferred (`setIsStrongBoxBacked(true)`), TEE fallback |

### Why "device-unlocked, no per-use prompt"

The encrypting key is gated on the device being unlocked, which is the
behavioral analog of iOS `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`. It is
**not** gated on a per-use biometric/PIN prompt:

- Cold-start identity restore runs in the kernel before any UI is presented.
  The user has already unlocked the device to launch the app, so the key is
  available with zero interaction.
- Using `setUserAuthenticationRequired(true)` (Android) /
  `kSecAccessControl` with biometry (iOS) would force a prompt before restore
  could run. That regresses below the current iOS behavior and breaks headless
  cold-start restore. It is deliberately **not** used.

### StrongBox preference + fallback (Android)

`setIsStrongBoxBacked(true)` requests a dedicated hardware security module
(StrongBox) when the device has one. Devices without StrongBox throw
`StrongBoxUnavailableException` at key generation; the capability catches it and
retries generation with StrongBox disabled (still TEE-backed). See
`getOrCreateKey()` / `generateKey(strongBox:)` in
`android/app/src/main/java/org/nmp/android/KeystoreKeyringCapability.kt`.

### API-level guards (Android)

`setUnlockedDeviceRequired` and `setIsStrongBoxBacked` are API 28+ (Android P).
The app's `minSdk` is 26, so both are applied only when
`Build.VERSION.SDK_INT >= Build.VERSION_CODES.P`. On API 26–27 the key remains
TEE-backed (non-exportable) without the device-unlocked constraint — the best
available posture on those platforms.

## Residual risk (accepted for v1)

On a *currently-unlocked, compromised* device, the encrypting key is usable, so
an attacker with code execution while the screen is unlocked could decrypt the
`nsec`. This matches the iOS posture and is the accepted tradeoff for
interaction-free cold-start identity restore. Tightening further (e.g. requiring
authentication and deferring restore until first unlock) would change the
restore UX on both platforms and is out of scope for v1.
