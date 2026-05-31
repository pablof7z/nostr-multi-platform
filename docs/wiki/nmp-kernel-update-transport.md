---
title: NMP Kernel Update Transport & FlatBuffers FFI
slug: nmp-kernel-update-transport
summary: The C-ABI `nmp_app_set_update_callback` signature is `(*context, *const u8, usize)`, requiring all FFI consumers to handle a byte pointer and length instead of
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-29
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:200932fb-5a92-44e0-8d42-2184d2e69094
  - session:54fc9b94-b995-46c6-8372-59c4abe0f95a
  - session:95156e27-58fe-4e26-9530-1778033c4559
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:485a5310-d073-41c9-b230-e6e77926a143
  - session:c066a9a0-1c78-4b21-8511-4be986a736de
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
  - session:d1ce3b4a-2a79-40f5-ba0e-fe608f5c7884
  - session:cd331450-f93f-48d0-960e-3c73e927775e
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# NMP Kernel Update Transport & FlatBuffers FFI

## FFI Interface

The C-ABI `nmp_app_set_update_callback` signature is `(*mut c_void, *const u8, usize)`, requiring all FFI consumers to handle a byte pointer and length instead of a JSON string. The `NmpCore.h` committed FFI header must match what cbindgen generates from Rust source; the `ffi-header-drift` CI gate enforces this. The `ffi-surface-freeze` CI gate rejects new or changed `nmp_app_*` C-ABI symbols unless a merged ADR authorizes them. CI gates enforce exact-text ABI match for the `(*mut c_void, *const u8, usize)` callback shape across all three iOS header copies to prevent silent ABI drift. The update callback delivers FlatBuffers update frames (not JSON) on every emit tick; no JSON debug fallback is retained for the update transport—a single canonical FlatBuffers transport is the only path. The transport layer uses an additive typed_projections sidecar with opaque payload bytes, keeping the existing byte callback ABI stable and avoiding app-specific FlatBuffers unions in nmp-core. The NmpUpdateCallback ABI passes borrowed FlatBuffers `nmp.transport.UpdateFrame` (file id `NMPU`) bytes carrying `Snapshot` and `Panic` variants where the pointer is valid only for the callback duration and Swift must copy before decoding. The `NmpUpdateBridge` is stored in `AppRuntime` to keep it alive, preventing the FFI callback from dereferencing a dangling pointer, and is explicitly taken and dropped in the `AppRuntime` `Drop` implementation after unregistering the callback. The native Rust FFI snapshot transport path drives real relay connections end-to-end through FlatBuffers encode/decode via the `(*const u8, usize)` C-ABI callback. ProtocolCommandContext::now_secs() uses the direct self.kernel.as_deref()?.now_secs() path when a kernel handle is attached, bypassing KernelClockAdapter and its RefCell entirely. Per-app snapshot functions (`nmp_app_podcast_snapshot` and `nmp_app_chirp_snapshot`) must use FlatBuffers encoding instead of JSON strings to comply with the F-10 specification requiring no production JSON runtime fallback. Migrating per-app snapshots to FlatBuffers requires per-app FlatBuffers schemas (or a generic value-tree schema), generated Swift decode paths via nmp-codegen, and updates to both nmp-app-chirp and nmp-app-podcast snapshot builders. V-37 introduces a generic `nmp_app_get_snapshot` pull path for non-Chirp apps, requiring an ADR because it introduces a new FFI symbol. The podcast app's NmpCore.h and KernelBridge must be updated to use the binary FlatBuffers callback contract `(const uint8_t *bytes, uintptr_t len)` instead of the stale JSON string contract `(const char *json)` for kernel update frames. The iOS host decodes typed nmp.feed.home using TypedHomeFeedDecoder.swift and regenerated Swift FlatBuffers bindings. Maintaining two parallel decode paths (generic and typed) for the same feed on iOS and Android is architecturally prohibited as a dual-seam violation under NMP doctrine. The nmp-ffi FFI body fills the {1,6} default for thread reply kinds, keeping the C-ABI signature unchanged so iOS is untouched and no peer collision occurs.

<!-- citations: [^95156-2] [^20093-2] [^20093-3] [^54fc9-1] [^37e35-5] [^e4861-3] [^f2605-10] [^c066a-2] [^56db9-9] [^d1ce3-11] [^cd331-8] [^42908-14] [^4edd4-18] -->
## Schema Versioning

The `nmp-core` crate owns the canonical FlatBuffers update-frame schema under `crates/nmp-core/schema/` and the Rust encode/decode helpers. Schema versioning for updates is canonical in `update_envelope.rs` as `SNAPSHOT_SCHEMA_VERSION`, re-exported as `KERNEL_SCHEMA_VERSION` in `kernel/update.rs`, and the Swift host drops frames with mismatched schema versions at decode time. Schema-version enforcement matches iOS on both Android and web with frame-level and payload-level checks. A checked-in v1 FlatBuffers wire fixture for the snapshot update frame must remain stable across regenerations.

<!-- citations: [^20093-4] [^20093-5] [^54fc9-3] [^e4861-4] -->
## Build and Dependency Management

Generated FlatBuffers bindings for Rust, Swift, Kotlin, and TS are committed to the repository tree so that `flatc` is not required at build time. FlatBuffers runtime version pins are intentionally asymmetric across platforms: Rust and Swift use 25.12.19, Web/TypeScript uses 25.9.23, and Android/Kotlin uses 25.2.10. CI enforces FlatBuffers runtime version pins for each platform to fail before mismatched generated bindings reach CI builds. WASM update bytes are delivered as Uint8Array directly through the snapshot callback, and WorkerEvent::UpdateBytes is never JSON-stringified across the worker boundary.

NMP v0.2.0 is a non-breaking release: the C-ABI (NmpCore.h) and FlatBuffers schemas are byte-identical to v0.1.0; apps upgrade by re-pinning to 0.2.0 and picking up fixes. Release changelogs are derived from interface diffs (C-ABI, FlatBuffers schemas), not fuzzy commit summaries, so app upgrade instructions are mechanically true. [^4edd4-19]

<!-- citations: [^20093-6] [^20093-7] [^54fc9-2] [^e4861-5] -->
## Supervisor and Panic Safety

The supervisor closure clones `update_tx_panic` before spawning to ensure panic frames still reach the listener after the actor's own sender is dropped. [^20093-8]

## Serialization and Performance

The FlatBuffers transport serializes `KernelSnapshot` to `serde_json::Value` then encodes that JSON value tree as FlatBuffers binary, rather than using a static schema. Rust snapshot serialization traverses the data twice: first via `serde_json::to_value` to build a JSON tree, then via an `encode_value` walk to transcribe it into FlatBuffers. Update serialization metrics `make_update_us` and `serialize_us` are tracked to make the performance cost of the transport change observable. The snapshot perf gate enforces a ceiling (MAX_MAKE_UPDATE_US=250000, MAX_SERIALIZE_US=150000) rather than a no-regression invariant. Before merging the FlatBuffers transport PR, performance numbers showing `make_update_us`, `serialize_us`, and `payload_bytes` deltas vs master at steady state must be provided. The generic FlatBuffers value tree is an interim transport shape; typed snapshot tables are the planned next performance step if foreground metrics show `make_update_us` or payload size approaching the 4 Hz tick budget.

<!-- citations: [^20093-9] [^20093-10] [^20093-11] [^54fc9-6] [^e4861-7] [^485a5-6] -->
## Data Integrity and Degradation Tracking

The kernel tracks update-frame encoding/decoding degradations via a monotonic `update_frame_degradations_total` counter for its lifetime. When update snapshot serialization fails, the kernel increments `update_frame_degradations_total` and emits a distinct 'transport degraded' frame kind (rather than a confusing partial snapshot), allowing the host to clear projection state. The degraded fallback contains the schema version, rev, tick, metrics, and error indicators. The FlatBuffers `decode_value` function returns a `Result` type that errors on non-finite float values, missing string, list, and map values, and unknown value kinds instead of degrading them to null. `decode_update_frame` propagates errors from `decode_value` rather than unwrapping them. `UpdateFrameDecodeError` includes an `InvalidValue` variant for invalid FlatBuffer value decoding errors. The Web FlatBuffers decoder throws typed errors on malformed input, returns BigInt for 64-bit integers outside Number.MAX_SAFE_INTEGER, and rejects NaN/Inf. The Android FlatBuffers decoder throws UpdateFrameDecodeException on malformed input, uses JsonUnquotedLiteral to preserve full u64 precision instead of clamping to Long.MAX_VALUE, and rejects NaN/Inf. The Swift FlatBuffer snake-case key converter preserves leading and trailing underscores while only removing underscores between words, matching the Rust convention and preventing private-looking fields from aliasing public names. Snapshot decode errors cascade: a decode failure in any projection (e.g. `ChirpTimelineSnapshot`) propagates through `SnapshotProjections` and `KernelUpdate`, dropping the entire snapshot and preventing `activeAccount` from updating. KernelBridge's decodeFlatBuffer catch block logs the specific missing key name and coding path for DecodingError.keyNotFound and typeMismatch.

<!-- citations: [^54fc9-5] [^20093-12] [^54fc9-4] [^37e35-4] [^e4861-6] [^485a5-5] -->
## See Also

