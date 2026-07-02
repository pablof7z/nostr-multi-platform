import * as flatbuffers from "flatbuffers";

import { FrameKind } from "./generated/nmp/transport/frame-kind";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";
import { PROJECTION_CONTRACT } from "./projectionContract.generated";

// #2722 — decode an `NMPU` UpdateFrame into the read side every consumer
// needs: the declared projection-key set for this snapshot, plus a lookup that
// hands back one projection's typed sidecar bytes, verified against the
// GENERATED `PROJECTION_CONTRACT` (schemaId + fileIdentifier) rather than a
// hand-copied `"NRRD"` / `"refs.profile"` literal at every call site — the
// pattern hl's `updateFrameDecoder.ts` and `web/nmp-gallery`'s inline
// `findTypedSidecar` each re-implemented separately.
//
// PURE: no worker/DOM globals — SSR-safe. Zero protocol logic beyond the
// mechanical envelope lookup Swift's generated `TypedXDecoder.decode(from:)`
// also performs (see `crates/nmp-codegen/src/swift_typed_decoders.rs`); the
// TS twin exposes it as one generic function rather than per-key enums
// because TS callers dispatch dynamically, not via static per-key types.

/** A decoded `UpdateFrame`. `findTypedProjection` is only valid to call
 *  synchronously against the SAME `bytes` `decodeUpdateFrame` was given — it
 *  closes over the live FlatBuffers reader, not a copy, so it must not be
 *  stashed across a later frame. */
export type DecodedUpdateFrame = {
  /** All projection keys present in this snapshot's typed-sidecar set
   *  (builtin + app-registered). */
  projectionKeys: string[];
  /** Session identity from the SnapshotFrame — feeds `KeyedRefCache.merge`. */
  sessionId: bigint;
  /** Snapshot epoch from the SnapshotFrame — feeds `KeyedRefCache.merge`. */
  snapshotEpoch: bigint;
  /** Whether the runtime reports itself running this frame. */
  running: boolean;
  /**
   * The raw typed-sidecar payload bytes for one projection `key`, or
   * `undefined` when the key is undeclared (no `PROJECTION_CONTRACT` row),
   * absent from this frame, empty, or carries a `fileIdentifier` other than
   * the contract's (a wire/registry mismatch — fail closed rather than hand
   * a decoder bytes it does not own).
   */
  findTypedProjection(key: string): Uint8Array | undefined;
};

/**
 * Decode an `NMPU` UpdateFrame from raw bytes.
 *
 * Returns `undefined` when:
 * - bytes are malformed or the buffer lacks the `NMPU` file identifier
 * - the frame is not a Snapshot kind (e.g. a Panic frame)
 *
 * On `undefined` the caller retains its last-good state (D6 fail-closed).
 */
export function decodeUpdateFrame(bytes: Uint8Array): DecodedUpdateFrame | undefined {
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!UpdateFrame.bufferHasIdentifier(bb)) return undefined;
    const frame = UpdateFrame.getRootAsUpdateFrame(bb);
    if (frame.kind() !== FrameKind.Snapshot) return undefined;
    const snapshot = frame.snapshot();
    if (!snapshot) return undefined;
    return decodeFromSnapshot(snapshot);
  } catch {
    return undefined;
  }
}

function decodeFromSnapshot(snapshot: SnapshotFrame): DecodedUpdateFrame {
  const projectionKeys: string[] = [];
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    const key = proj?.key();
    if (key) projectionKeys.push(key);
  }
  return {
    projectionKeys,
    sessionId: snapshot.sessionId(),
    snapshotEpoch: snapshot.snapshotEpoch(),
    running: snapshot.running(),
    findTypedProjection: (key) => findTypedProjection(snapshot, key),
  };
}

function findTypedProjection(snapshot: SnapshotFrame, key: string): Uint8Array | undefined {
  const contract = PROJECTION_CONTRACT[key];
  if (!contract) return undefined;
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    if (!proj || proj.key() !== key) continue;
    const payload = proj.payload();
    if (!payload || payload.fileIdentifier() !== contract.fileIdentifier) return undefined;
    const bytes = payload.payloadArray();
    if (!bytes || bytes.length === 0) return undefined;
    // Copy so the view outlives the ByteBuffer once the caller returns.
    return bytes.slice();
  }
  return undefined;
}
