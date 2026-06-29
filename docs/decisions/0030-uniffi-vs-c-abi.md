# ADR-0030 — Native binding surface and FlatBuffers transport

- **Status:** Accepted — amended for clean-break native binding target
- **Date:** 2026-05-23
- **Updated:** 2026-06-26 (M14-0 / issue #2129 — Android app-loop lane migrated to UniFFI)
- **Updated:** 2026-06-29 (after #2403/#2463: migrated raw native ABI deleted; UniFFI is the native public surface)
- **Relates to:** ADR-0009, ADR-0010, ADR-0037, ADR-0044, ADR-0069..ADR-0073

## Context

NMP has two different host binding problems:

1. **Write/register verbs**: lifecycle, callback installation, capability
   callbacks, identity helpers, and generic action dispatch.
2. **Read/update decoding**: the high-volume pushed update stream and typed
   projection payloads.

Those surfaces have different concerns, but they do not justify two public
native ABI families by default. UniFFI is the target for native object/verb
bindings. FlatBuffers owns the hot read/write payload bytes that pass through
that binding. Browser/wasm remains separate because its ABI is the
`wasm-bindgen` worker surface owned by `nmp-browser-runtime`.

## Decision

The clean-break target is:

- native hosts expose one public binding surface: UniFFI;
- browser hosts expose the `wasm-bindgen` worker/runtime surface;
- FlatBuffers `Vec<u8>` / `ByteArray` payloads remain the action/update transport
  through those bindings;
- migrated native C/JNI byte lanes are deleted unless a measured hot-path
  exception proves UniFFI byte passing is insufficient for that exact lane.

#2403 completed the migrated raw native ABI deletion tracker, and #2463 deleted
the migrated runtime config/diagnostics C ABI. Any retained raw `nmp-ffi` or
app-owned C/JNI surface is internal/transitional compatibility, test support, or
app delivery glue with an owning issue. It is not a second app-facing native
binding family.

Generate and check in typed read decoders for the update stream through
`nmp-codegen` and schema-owned FlatBuffers generation. The read surface is typed
snapshot envelope fields plus typed projection sidecars; host decoder drift is a
codegen problem, not an argument for a separate public native byte ABI.

## M14-0 execution (issue #2129 — Android app-loop lane, 2026-06-26)

The Android app-loop lane proved the intended shape before Chirp was extracted to
its own repository (#2295/#2303). That slice migrated JNI app-loop bindings to
UniFFI proc-macro bindings (pinned `uniffi = "=0.29.5"`). The `AppHandle`
UniFFI object exposed lifecycle, byte dispatch, update-sink registration, and
shutdown methods. The `UpdateSink` callback interface delivered FlatBuffers
frames push-side (D8). The deleted JNI symbols were:
`nativeNew`, `nativeStart`, `nativeStop`, `nativeClose`, `nativeFree`,
`nativeSetUpdateListener`, `nativeClearUpdateListener`,
`nativeDispatchIntentBytes`, `nativeDispatchActionBytes`.

FlatBuffers remains the byte payload format for both action dispatch (NMPD
envelopes) and update delivery (NMPU frames). UniFFI wraps the transport;
it does not own or transcode it.

The original generated Kotlin binding moved with the external Chirp app. This
repository now owns `crates/nmp-uniffi`, checked-in Swift/Kotlin bindings, and
the `ci/check-uniffi-bindings-drift.sh` drift gate. Residual raw C/JNI exports
are not the native public target; #2125 owns deleting, hiding, or formally
scoping any remaining compatibility lanes after their UniFFI replacement exists.

## Rules

- New write verbs need a clear reason they cannot be routed through generic
  action dispatch.
- Projection/read schema changes must regenerate host decoder glue and pass the
  checked-in diff gate.
- UniFFI migration work must not take ownership of the update stream.
- New native app-facing binding work targets UniFFI. A raw C/JNI exception needs
  a measured reason, a deletion trigger, and an internal wrapper behind the
  UniFFI API.
- The UniFFI binding is proc-macro only (no UDL). The `cdylib_name` in
  `uniffi.toml` must match the `[lib] name` in `Cargo.toml`.

## Consequences

- The native binding surface can migrate deliberately without blocking typed read
  safety.
- Host projection drift becomes a generated-code diff instead of a hand-mirrored
  decoder bug.
- Platform hosts decode update frames through schema-owned generated bindings.
- The Android app-loop proof showed UniFFI can carry the lifecycle object and the
  FlatBuffers `Vec<u8>` update/action payloads together.
- Long-lived native C/JNI and UniFFI public surfaces for the same responsibility
  are fragmentation, not architecture.

## Measurement gate for exceptions

Do not replace FlatBuffers update/action bytes with UniFFI records for hot
projection frames unless a representative benchmark proves the byte payload lane
is the bottleneck and the replacement keeps D8 push delivery, frame ordering, and
allocation/latency budgets at least as good as the existing FlatBuffers path.

If a future slice proposes keeping a separate native C/JNI byte lane alongside
UniFFI, it must first show:

- the UniFFI `Vec<u8>` / `ByteArray` path fails a named production budget for a
  specific hot lane;
- the C/JNI lane is hidden behind the UniFFI API rather than exposed as a second
  app-facing surface;
- the issue or ADR records owners, benchmarks, thresholds, and a re-test/delete
  trigger.
