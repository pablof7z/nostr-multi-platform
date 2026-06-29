# Native Binding Surface Reference

> **Reviewed:** 2026-06-29 after #2403/#2463.
>
> **Current public native target:** UniFFI through `crates/nmp-uniffi` for
> iOS, Android, and desktop native hosts.
>
> **Browser target:** `wasm-bindgen` through `nmp-browser-runtime`; browser/wasm
> is not part of the native ABI collapse.
>
> **Payload target:** FlatBuffers remain the hot action/update payload bytes.
> UniFFI carries `Vec<u8>` / `ByteArray` frames; it does not replace `NMPD`
> dispatch envelopes or `NMPU` update frames with UniFFI records.

This document describes the maintained binding direction after the M14 raw
native ABI deletion work. It is not a compatibility catalog for deleted
`nmp-ffi` C symbols. If a symbol was removed by #2403 or #2463, do not teach it
as current public API and do not add a shim unless #2125 records a measured
internal exception behind the UniFFI API.

## Public Native Surface

Native app shells use the generated UniFFI bindings from `crates/nmp-uniffi`.
The checked-in Swift/Kotlin bindings are guarded by
`ci/check-uniffi-bindings-drift.sh`.

The public native app object is UniFFI `NmpApp`, an `Arc`-backed wrapper over
`nmp-native-runtime::NmpApp`. Its core lifecycle is:

1. construct `NmpApp`;
2. apply pre-start configuration and feature/capability wiring exposed by
   UniFFI;
3. register an update sink;
4. start the runtime;
5. dispatch actions/open sessions/render emitted state;
6. stop/reset/shutdown through UniFFI lifecycle methods.

The native shell renders and executes raw capabilities. Rust/NMP owns protocol
policy, relay routing, signing policy, durable state, retries, publish status,
session teardown, and cache truth.

## FlatBuffers Through UniFFI

FlatBuffers are still the wire payload for the hot paths:

- `NMPD` dispatch envelopes enter through UniFFI `NmpApp::dispatch_action`.
- `NMPU` update frames leave through the UniFFI `UpdateSink::on_update`
  callback.
- feed/search/ref/mirror helpers may return or accept FlatBuffers bytes where
  the owned runtime contract is byte-shaped.

The UniFFI layer owns object lifetimes, callbacks, typed records/errors, and
host-language generation. It does not own the action/update schemas. Schema and
payload evolution remain owned by the FlatBuffers/codegen crates.

## Current UniFFI Areas

`crates/nmp-uniffi` is organized by migrated responsibility:

| Area | Public native shape |
|---|---|
| App lifecycle | `NmpApp::new`, `start`, `configure`, `stop`, `reset`, `shutdown`, `set_update_sink` |
| Action doorway | `dispatch_action(Vec<u8>) -> DispatchOutcome` |
| Stateless helpers | NIP-19/NIP-21/content/intent helpers |
| Identity/signer/relay | account registration, local signer, NIP-46, external signer, relay edits |
| Reference resolution | profile/event/ref resolve and release helpers |
| Capability/action/publish control | capability sink, action-result observer, retry/cancel publish controls |
| Sessions | feed, search, and URI-routing sessions/helpers |
| Runtime config/diagnostics/lifecycle | storage/projection config, lifecycle callback, liveness, debug info, intent dispatch |
| Mirror pull | `mirror_pull_page` returning typed `MirrorPullResult` with byte payload variants |

Browser hosts use the separate `nmp-browser-runtime` `wasm-bindgen` surface. Do
not route browser guidance through UniFFI and do not use browser/wasm as a reason
to retain legacy native symbols.

## Deleted Legacy Native Symbols

#2403 completed the migrated raw native ABI deletion tracker, and #2463 deleted
the migrated runtime config/diagnostics native ABI. The following families are not
current framework public API:

- app-loop lifecycle/update callback/action doorway C symbols;
- stateless helper C exports migrated to UniFFI;
- lifecycle observer/signal C exports migrated to UniFFI;
- feed/search/URI session C exports migrated to UniFFI;
- mirror pull C exports migrated to UniFFI;
- capability/action/publish control C exports migrated to UniFFI;
- runtime config, input-intent dispatch, and diagnostics C exports migrated to
  UniFFI.

Historical references may name these symbols only as deleted history or test
evidence. New native guidance must name the UniFFI method, generated binding, or
Rust runtime seam that replaced the symbol.

## Retained Raw Surfaces

Some raw `extern "C"` functions remain in-tree. They are not a second framework
native public API.

| Surface | Current status | Owner |
|---|---|---|
| `crates/nmp-ffi` compatibility helpers for identity, signer broker, external signer, relay edits, and ref resolution | Transitional/internal compatibility while downstream and tests finish moving to UniFFI. New native app-facing work must not target these symbols. | #2125 |
| `nmp_free_string` | Shared allocator helper for any retained C string returns. It exists because raw compatibility/app-owned C seams still return Rust-owned strings. | #2125 |
| `crates/nmp-ffi` test-support exports | Test/perf harness only; never production binding guidance. | #2125 |
| Gallery app/native and Android bridge shims | App-owned delivery glue for `apps/nmp-gallery`, not reusable NMP framework ABI. | #2125 / app owner |
| Marmot native surface under `crates/nmp-marmot` | Post-v1/provisional mixed binding shape; do not promote it into a durable public native ABI. | #2232 |

Any proposal to keep a raw native byte lane after its UniFFI replacement exists
must meet ADR-0030's exception gate: measured production budget failure through
UniFFI bytes, an internal wrapper behind the UniFFI API, named owner,
thresholds, retest date, and delete trigger.

## Boundary Rules

- Native public documentation names UniFFI first.
- Browser public documentation names `wasm-bindgen`.
- FlatBuffers remain action/update payload bytes across both binding families.
- `nmp-ffi` is not a runtime owner; runtime lifecycle lives in
  `nmp-native-runtime` and is exposed to native hosts through `nmp-uniffi`.
- Native shells do not choose relays, mutate protocol tags, infer publish
  success, own retries, or cache product truth.
- Deleted legacy native symbols are not compatibility requirements.

## Verification Pointers

- `rg -n "pub extern \"C\" fn" crates/nmp-ffi/src` shows retained raw
  compatibility/test-support exports.
- `rg -n "uniffi::export|uniffi::Object|callback_interface" crates/nmp-uniffi/src`
  shows the current native public surface.
- `bash ci/check-uniffi-bindings-drift.sh` verifies generated native binding
  drift when UniFFI interfaces change.
