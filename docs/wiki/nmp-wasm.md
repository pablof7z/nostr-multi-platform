---
title: NMP WASM Crate & Browser Facade
slug: nmp-wasm
summary: nmp-wasm is a deliberate stub that simulates wire protocol handling without depending on nmp-core
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-05-28
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:ea09995a-5ec4-4129-8696-16936d846911
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:0c4f2143-76f1-4bb0-ba37-ea0f65f1432c
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:594b7c34-efd1-4461-81ad-9fa33a6e76f9
---

# NMP WASM Crate & Browser Facade

## Purpose & Current State

nmp-wasm is a deliberate stub that simulates wire protocol handling without depending on nmp-core. Its one honest function is validating the wire protocol contract in unit tests while the full actor driver is pending. It lacks nmp-core dependency, real relay connections, real signing, subscriptions, and persistence. A WASM Nostr client must have live relay connectivity — it is non-negotiable. It stores PublishNote notes only in a local Vec<LocalNote> and emits a fake snapshot with author_pubkey set to 'browser-local'. It returns CapabilityFailure for all actions except PublishNote, with the message that the browser wasm facade accepts publish-note intents only and live relay-backed actions require the full actor driver. The nmp-wasm/src/lib.rs WasmRuntime implementation remains structurally unchanged from the toy stub.

<!-- citations: [^ea099-1] [^ea099-2] [^1670f-14] [^0c4f2-1] -->
## Degraded Runtime Fallback

The web worker in chirp falls back to DegradedRuntime that rejects everything if the wasm build artifact at /nmp-wasm/nmp_wasm.js is not available. The DegradedMode::BrowserActorDriverMissing protocol variant is the code's own acknowledgment of the wasm capability gap. [^ea099-3]

## Core Architectural Problem

nmp-core uses std::thread, flume blocking recv, tungstenite, and lmdb which do not compile to wasm32-unknown-unknown. This is the fundamental blocker that prevents nmp-core from being used directly in the wasm target. The fix requires splitting I/O from logic and adding a wasm-specific driver rather than CFG-gating nmp-core. WASM relay transport must share the same protocol logic as iOS/Android/desktop, not implement a parallel WASM-specific relay driver. The browser wasm WebSocket panic is a pre-existing issue outside the scope of the FlatBuffers transport PR.

<!-- citations: [^ea099-4] [^1670f-15] [^0c4f2-2] [^e4861-8] -->
## WASM Plan (docs/plans/WASM.md)

The WASM plan is committed to docs/plans/WASM.md and covers 8 phases for making nmp-wasm fully functional.

- Phase 0: Produce two ADRs (ADR-0030, ADR-0031) for actor driver model and storage tiers, plus a CI wasm32 check. ADR-0030 and ADR-0031 have not been written yet — nothing exists in docs/decisions/ for those.
- Phase 1: Feature-gate nmp-core's native I/O and add nmp-core as a dependency of nmp-wasm. Phase 1b (feature-gating nmp-core's native I/O with a 'native' feature) is already complete, landing in commit cd98db25.
- Phase 2: Introduce a RelaySocket trait with a web-sys::WebSocket wasm implementation. The WASM read path uses BrowserRelayDriver (WebSocket per relay), routing inbound frames through the kernel to a snapshot push callback.
- Phase 3: Replace the toy WasmRuntime with a WasmKernelDriver cooperative async event loop using spawn_local and gloo-timers idle ticks.
- Phase 4: Add NIP-07 capability via the ADR-0024 async capability pattern with nsec import fallback.
- Phase 5: Align snapshot projection so featureSnapshotFromEnvelope receives real data.
- Phase 6: Ship in-memory storage as a baseline with Phase 3 and add OPFS SQLite for v1.
- Phase 7: Establish the build pipeline with just build-wasm, Vite integration, and Playwright CI.

Two unmerged WASM branches exist: codex/chirp-web-wasm-bridge-lane1 (3 commits covering Phase 7b/7c and Phase 3c) and codex/chirp-web-vercel-wasm-asset (3 commits covering Phase 7b fix).

<!-- citations: [^ea099-5] [^0c4f2-3] [^594b7-7] -->
## TypeScript Layer

The TypeScript layer (worker.ts, wasmBridge.ts, snapshot.ts) is architecturally correct and needs minimal changes — only a new CapabilityRequest event handler.

<!-- citations: [^ea099-6] [^0c4f2-4] -->
## See Also

