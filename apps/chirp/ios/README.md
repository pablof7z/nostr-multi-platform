# iOS Apps

## Chirp

The production Nostr client and canonical iOS showcase for NMP. All active iOS
development lives here.

Chirp is expected to surface every reusable NMP feature before that feature is
called product-ready, unless the feature has a documented platform exception.
The old Twitter-like timeline is only the social baseline; Chirp's goal is a
complete client that demonstrates the whole framework.

Chirp is NMP's reference iOS client and primary showcase app for the NMP framework; it demonstrates every reusable NMP feature as it ships.

### Absorbed apps

Two companion apps — **NmpStress** and **NmpPulse** — were deleted on 2026-05-18 and their goals merged into Chirp.

| Former app | Goal | Now lives in |
|---|---|---|
| **NmpStress** | Performance diagnostics of the real kernel FFI pipeline (Swift-side timing, NMP_PERF prints, logical interests, wire subscriptions) | `Chirp/Features/DiagnosticsView.swift` (Settings → Diagnostics) |
| **NmpPulse** | E2E kernel validation through the real FFI surface (smoke scenarios hitting real relays) | `ChirpTests/SmokeScenariosTests.swift` (gated behind `NMP_SMOKE=1`) |

The UI test that exercises navigation end-to-end (timeline → profile → thread) is in `ChirpUITests/ChirpUITests.swift`.
