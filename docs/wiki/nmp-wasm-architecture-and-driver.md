---
title: NMP WASM — Architecture, Driver, and Browser Simulation
slug: nmp-wasm-architecture-and-driver
summary: The nmp-wasm crate is a stub with zero nmp-core dependency, providing only browser-local simulation
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-05-27
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:ea09995a-5ec4-4129-8696-16936d846911
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# NMP WASM — Architecture, Driver, and Browser Simulation

## Stub Architecture

The nmp-wasm crate is a stub with zero nmp-core dependency, providing only browser-local simulation. It returns CapabilityFailure for all actions except publish-note, with the message that live relay-backed actions require the full actor driver. Published notes in wasm never leave the browser process and use a hardcoded author_pubkey of 'browser-local'. [^ea099-1]


## Degraded Runtime

The web worker falls back to a DegradedRuntime that rejects everything if the wasm build artifact is not available. The DegradedMode::BrowserActorDriverMissing variant in the protocol acknowledges the gap between the stub and full implementation. [^ea099-2]

## Protocol Validation

The wasm layer validates the wire protocol contract in unit tests while the full actor driver is pending. [^ea099-3]

## TypeScript Bridge Layer

The TypeScript layer (worker.ts, wasmBridge.ts, snapshot.ts) is architecturally correct and needs only a new CapabilityRequest event handler. [^ea099-4]

## Phased Implementation Roadmap

Phase 0 requires two ADRs (actor driver model, storage tiers) and a CI wasm32 check. Phase 2 introduces a RelaySocket trait with a web-sys::WebSocket wasm implementation. Phase 3 replaces the toy WasmRuntime with a WasmKernelDriver using a cooperative async event loop (spawn_local + gloo-timers idle ticks). Phase 4 adds NIP-07 capability via the ADR-0024 async capability pattern with nsec import fallback. Phase 5 aligns snapshot projection so featureSnapshotFromEnvelope receives real data. Phase 6 uses in-memory storage as the baseline shipping with Phase 3, and OPFS SQLite for v1. Phase 7 establishes a build pipeline with just build-wasm, Vite integration, and Playwright CI. [^ea099-5]

## Known Pre-existing Issues

The browser WASM WebSocket `RefCell already borrowed` panic at `relay_pool.rs:100` is a pre-existing issue in `nmp-network::browser_driver` / `nmp-wasm::relay_pool`, not introduced by the current PR, and must be tracked as a separate issue. The `std::time::Instant::now` panic (`time not implemented on this platform`) in the `wasm32-unknown-unknown` target is a pre-existing limitation, not introduced by this PR. Phase 4 has five concrete wasm publish-path gaps: AppAction is not wired, NIP-46 bunker is blocked on wasm-native transport, the native ActorCommand publish path has no wasm equivalent, certain signer kinds are unrecognized, and wasm-bindgen-test coverage is zero.

<!-- citations: [^e4861-9] [^cd2b6-6] -->
## See Also

