---
title: FlatBuffers Transport
slug: flatbuffers-transport
topic: crate-architecture
summary: "PR #582 replaces the Rustâhost snapshot transport with FlatBuffers (nmp.transport.UpdateFrame, file id NMPU) carrying two variants: Snapshot (full kernel snap"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-06-19
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:019edbff-1d29-7533-99ab-0b8130b805dc
  - session:019edc16-8e40-7a92-9ea1-7405af0d34f3
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# FlatBuffers Transport

## Overview

PR #582 replaces the Rust→host snapshot transport with FlatBuffers (nmp.transport.UpdateFrame, file id NMPU) carrying two variants: Snapshot (full kernel snapshot as a JSON-like value tree) and Panic (terminal actor-death signal). This is a standalone transport-switch PR, not the first half of a typed-tables migration; the 2–3× hot-path perf regression is an acceptable interim with a tracked follow-up for typed tables. FullState/full snapshot must remain the correctness path; granular ViewBatch or delta variants may only be added when profiling proves the snapshot path is the bottleneck and the delta is lossless. The Rust encoder fallback on serde_json to_value failure constructs a degraded stub; the host may need a distinct 'transport degraded' frame kind so it can clear projection state rather than render a confusing partial snapshot. The FFI callback shape for the snapshot callback is (*mut c_void, *const u8, usize) and is guarded by an exact-text ABI check across all three iOS header copies via ci/check-ffi-header-drift.sh.

Every external effect must be represented as typed data crossing the Rust/native boundary; Rust requests a capability, native reports raw results, Rust decides next state. <!-- [^019ed-58] -->

New nondeterministic inputs (time, randomness, network, OS callbacks, capability completions) must enter the actor as explicit actions/events or injected seams; reducers must remain replayable from message history. <!-- [^019ed-59] -->

The P1 campaign removes all presentation formatting (SF Symbol names, English labels, pluralization, bech32, emoji) from Rust projections and FlatBuffers paths, delegating it to native shells, enforcing aim.md §2 (not the bogus in-code citations of §4.4/V-24 that claim the opposite).

Nip10ReplyAttribution flat mirrors (author_display_name, author_picture_url) and AuthorDisplay.npub must be removed from Rust; shells use the nested authorDisplay + nmp_app_encode_profile path instead. <!-- [^11850-55] -->

P4 Finding 4 (ExternalSignerCapabilityBridge transport selection + concurrent-Intent rejection) is NOT a violation — transport selection is mechanical from Rust-set fields and concurrent-Intent rejection is an OS capacity constraint.

<!-- citations: [^019ed-58] [^019ed-59] [^11850-55] [^e4861-1] [^e4861-2] [^e4861-3] [^019ed-10] [^11850-11] [^11850-96] -->
## Web Client

Web updateFrame.ts throws typed errors on malformed FlatBuffers input (unknown ValueKind, non-finite floats, missing nested values), returns BigInt for 64-bit integers outside Number.MAX_SAFE_INTEGER, and rejects NaN/Infinity. On snapshot decode failure, schema mismatch, or panic frame, client.ts preserves the last good latestUpdateBytes rather than clearing it to undefined, so the UI does not flash on transient bad frames. The web SNAPSHOT_SCHEMA_VERSION is hardcoded to 1 in updateFrame.ts rather than crossing the FFI boundary. <!-- [^e4861-4] -->

Web client.ts routes schema mismatch to the existing protocol_mismatch RuntimeStatus variant and decode failure to browser_actor_driver_missing, rather than introducing a new transport_schema_mismatch variant. <!-- [^e4861-5] -->

The web protocol.ts bytes type is Uint8Array only (dropped the number[] fallback). WASM update bytes are delivered as Uint8Array directly through the snapshot callback; WorkerEvent::UpdateBytes is drained server-side in handle_json and never JSON-stringified across the worker boundary, while UpdateBytes remains Serialize on native for protocol test round-tripping. <!-- [^e4861-6] -->

On Start and kernel-namespaced dispatch, the update_bytes postMessage fires before the runtime_status/action_accepted postMessage; client.ts records them independently so there is no causal break, but a future caller assuming the opposite order would need to be aware. <!-- [^e4861-7] -->

No transfer list is used on postMessage for updateBytesEvent because client.ts caches latestUpdateBytes across snapshots; transferring would detach the ArrayBuffer and break the cache. Structured clone copies the ~870KB worst-case frame cheaply. <!-- [^e4861-8] -->

The web golden fixture is loaded via Vite's ?raw import as part of the browser-runtime rebuild under #2038; the ?raw path keeps typecheck green while failing the suite if the Rust golden fixture drifts. <!-- [^e4861-9] -->

## Android Client

Android NmpUpdateFrameDecoder.kt throws UpdateFrameDecodeException on malformed input (unknown ValueKind, non-finite floats, missing payloads), uses JsonUnquotedLiteral to preserve full u64 precision instead of clamping to Long.MAX_VALUE, and GalleryModel.applyFrame catches UpdateFrameDecodeException to log and drop the frame. Android enforces schema version against SCHEMA_VERSION_EXPECTED = 1u on both SnapshotFrame.schemaVersion and the payload's inner schema_version key. <!-- [^e4861-10] -->

The Android golden fixture is loaded via classpath-relative src/test/resources/fixtures/ rather than a file-relative path to the Rust fixture, with a comment documenting the manual sync requirement and that no CI gate exists for it today. <!-- [^e4861-11] -->


Android WalletScreen must bind the Rust-computed WalletStatus.is_connected boolean through TypedWalletDecoder -> Snapshot model -> WalletScreen rather than deriving connection state from the tone discriminant in native code, matching the iOS WalletView.swift pattern. <!-- [^019ed-60] -->

Rust computes is_connected as (status == "connecting" || status == "ready"); the Android projection uses nullable Boolean (null when no wallet projection) with ?: false fallback. <!-- [^019ed-61] -->

The old Android tone-based derivation (walletTone != null && walletTone != "inactive") treated the "error" tone as connected, while the new Rust-computed is_connected treats "error" as not connected — a behavioral divergence that the PR description must acknowledge or the fix must address. <!-- [^019ed-62] -->
## Schema Version Enforcement

Schema-version enforcement is now applied on both Android and web (frame-level and payload-level checks), mirroring the iOS KernelBridge.swift:525-528 pattern. <!-- [^e4861-12] -->

## Golden Fixtures

The golden hex fixture (update_frame_snapshot_v1.fb.hex) is decoded and asserted by both a vitest case and a JVM unit test, catching the regenerated-with-wrong-flatc class of bugs that the version-pin guard alone cannot. Golden wire fixtures (including triplicated Rust .fb.hex + Kotlin POPULATED_HEX + Swift populatedHex) must be regenerated and both shells compiled before merging any wire-shape change.

<!-- citations: [^e4861-13] [^11850-117] [^11850-250] -->
