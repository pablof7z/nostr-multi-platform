import * as flatbuffers from "flatbuffers";

// ADR-0064 / S2 (#1750) — the open write-command byte transport.
//
// A write command crosses the wasm boundary as the raw bytes of a finished
// `DispatchEnvelope` FlatBuffers root over the ONE `dispatch_bytes` doorway —
// identical in shape to the native FFI `nmp_app_dispatch_action_bytes` seam.
// There is no wasm-only write vocabulary (the hand-rolled `app_action` envelope
// was deleted in #1743 Cut A).
//
// These constants MUST stay in lockstep with
// `crates/nmp-core/src/transport/dispatch_envelope.rs`:
//   - file identifier `"NMPD"` (distinct from `UpdateFrame`'s `NMPU`)
//   - `schema_version` tripwire `1` (fail-closed; bumping is a deliberate,
//     lockstep change across every host builder + the registry decode)
//   - the `DispatchEnvelope` field slots (declaration order in
//     `dispatch_envelope.fbs`): correlation_id=0, action_namespace=1,
//     schema_version=2, payload=3.

/** `root_type DispatchEnvelope` file identifier — mirrors the Rust constant. */
export const DISPATCH_ENVELOPE_FILE_IDENTIFIER = "NMPD";

/** The single recognised envelope schema version (fail-closed tripwire). */
export const DISPATCH_ENVELOPE_SCHEMA_VERSION = 1;

/**
 * Encode a `DispatchEnvelope` to finished, file-identified bytes.
 *
 * `payload` is carried verbatim (opaque) — the caller owns its typed per-crate
 * FlatBuffers encoding. The runtime decodes these bytes through
 * `nmp_core::dispatch_envelope::decode_dispatch_envelope`.
 */
export function encodeDispatchEnvelope(
  correlationId: string,
  actionNamespace: string,
  payload: Uint8Array,
): Uint8Array {
  const builder = new flatbuffers.Builder(payload.byteLength + 64);
  const correlationOffset = builder.createString(correlationId);
  const namespaceOffset = builder.createString(actionNamespace);
  const payloadOffset = builder.createByteVector(payload);

  // Field slots match the `dispatch_envelope.fbs` declaration order. The
  // generated Rust binding uses the same slots (VT_CORRELATION_ID = 4, i.e.
  // slot 0, etc.); a finished buffer is portable across hosts.
  builder.startObject(4);
  builder.addFieldOffset(0, correlationOffset, 0);
  builder.addFieldOffset(1, namespaceOffset, 0);
  builder.addFieldInt32(2, DISPATCH_ENVELOPE_SCHEMA_VERSION, 0);
  builder.addFieldOffset(3, payloadOffset, 0);
  const envelope = builder.endObject();

  builder.finish(envelope, DISPATCH_ENVELOPE_FILE_IDENTIFIER);
  return builder.asUint8Array();
}

/** The routing fields of a `DispatchEnvelope` (the opaque payload is not read). */
export interface DispatchEnvelopeRouting {
  correlationId: string;
  actionNamespace: string;
}

/**
 * Read the routing fields (`correlation_id`, `action_namespace`) from a finished
 * `DispatchEnvelope` buffer. The opaque `payload` is never interpreted.
 *
 * Used by the degraded in-process runtime to key its honest `capability_failure`
 * to the right operation when no real wasm kernel is present. Returns `null` for
 * a buffer that is not a `DispatchEnvelope` root (wrong/absent file identifier).
 */
export function decodeDispatchEnvelopeRouting(bytes: Uint8Array): DispatchEnvelopeRouting | null {
  if (bytes.byteLength < 8) {
    return null;
  }
  // File identifier lives at byte offset 4 (before any table traversal).
  const id = String.fromCharCode(bytes[4], bytes[5], bytes[6], bytes[7]);
  if (id !== DISPATCH_ENVELOPE_FILE_IDENTIFIER) {
    return null;
  }
  const bb = new flatbuffers.ByteBuffer(bytes);
  const root = bb.readInt32(bb.position()) + bb.position();
  const correlationId = readStringField(bb, root, 4) ?? "";
  const actionNamespace = readStringField(bb, root, 6) ?? "";
  return { correlationId, actionNamespace };
}

/** Read a `string` table field at the given vtable offset, or `null` if absent. */
function readStringField(bb: flatbuffers.ByteBuffer, root: number, vtableOffset: number): string | null {
  const offset = bb.__offset(root, vtableOffset);
  if (offset === 0) {
    return null;
  }
  return bb.__string(root + offset) as string;
}
