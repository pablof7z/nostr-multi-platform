# ADR-0030 — Binding surface split

- **Status:** Accepted — M14-0 partially executed (Android app-loop lane migrated)
- **Date:** 2026-05-23
- **Updated:** 2026-06-26 (M14-0 / issue #2129 — Android app-loop lane migrated to UniFFI)
- **Relates to:** ADR-0009, ADR-0010, ADR-0037, ADR-0044

## Context

NMP has two different host binding problems:

1. **Write/register verbs**: lifecycle, callback installation, capability
   callbacks, identity helpers, and generic action dispatch.
2. **Read/update decoding**: the high-volume pushed update stream and typed
   projection payloads.

Those surfaces have different owners. UniFFI is a fit for object/verb bindings.
It is not the transport for high-volume update frames.

## Decision

Keep the write/register surface on the existing ABI until the UniFFI migration is
scheduled as its own binding milestone.

Generate and check in typed read decoders for the update stream through
`nmp-codegen` and schema-owned FlatBuffers generation. The read surface is typed
snapshot envelope fields plus typed projection sidecars; host decoder drift is a
codegen problem, not a UniFFI problem.

## M14-0 execution (issue #2129 — Android app-loop lane, 2026-06-26)

The Android app-loop lane has been migrated from JNI to UniFFI proc-macro
bindings (pinned `uniffi = "=0.29.5"`). The `AppHandle` UniFFI object now
exposes `new()`, `start()`, `stop()`, `close()`, `dispatch_action_bytes()`,
`dispatch_action_json()`, `dispatch_intent_json()`, `set_update_sink()`, and
`clear_update_sink()`. The `UpdateSink` callback interface delivers FlatBuffers
frames push-side (D8). The deleted JNI symbols are:
`nativeNew`, `nativeStart`, `nativeStop`, `nativeClose`, `nativeFree`,
`nativeSetUpdateListener`, `nativeClearUpdateListener`,
`nativeDispatchIntentBytes`, `nativeDispatchActionBytes`.

FlatBuffers remains the byte payload format for both action dispatch (NMPD
envelopes) and update delivery (NMPU frames). UniFFI wraps the transport;
it does not own or transcode it.

The generated Kotlin binding is checked in at:
`apps/chirp/android/app/src/main/java/org/nmp/android/uniffi/nmp_android_ffi.kt`
and gated by `ci/check-uniffi-kotlin-drift.sh`.

## Rules

- New write verbs need a clear reason they cannot be routed through generic
  action dispatch.
- Projection/read schema changes must regenerate host decoder glue and pass the
  checked-in diff gate.
- UniFFI migration work must not take ownership of the update stream.
- The UniFFI binding is proc-macro only (no UDL). The `cdylib_name` in
  `uniffi.toml` must match the `[lib] name` in `Cargo.toml`.

## Consequences

- The write surface can migrate deliberately without blocking typed read safety.
- Host projection drift becomes a generated-code diff instead of a hand-mirrored
  decoder bug.
- Platform hosts decode update frames through schema-owned generated bindings.
- The Android shell no longer holds a raw `jlong` session handle for app-loop
  lifecycle; the UniFFI `AppHandle` object owns the session lifetime.
