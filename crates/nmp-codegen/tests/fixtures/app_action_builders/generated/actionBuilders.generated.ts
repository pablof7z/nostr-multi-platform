// GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen action-builders --registry <app>/action-builders.json \
//       --platform ts --out <output>
//
// Source of truth: app-local action-builders registry JSON passed via
// `--registry`. NOT NMP's built-in `ACTION_BUILDERS` table.

import * as flatbuffers from "flatbuffers";

import { encodeDispatchEnvelope } from "./dispatchEnvelope";

export type PublishRouteClass =
  | "manual_override"
  | "group_host_pin"
  | "verified_private_inbox"
  | "imported_or_presigned"
  | "diagnostic";

export type PublishTargetSelection =
  | { kind: "auto" }
  | { kind: "explicit"; relays: string[]; routeClass: PublishRouteClass };

/** Encode a `[string]` FlatBuffers vector (built last element first) and
 * return its offset. Shared by the generated builders below. */
function stringVector(fbb: flatbuffers.Builder, values: string[]): flatbuffers.Offset {
  const offsets = values.map((s) => fbb.createString(s));
  fbb.startVector(4, offsets.length, 4);
  for (let i = offsets.length - 1; i >= 0; i--) fbb.addOffset(offsets[i]!);
  return fbb.endVector();
}

/** Encode a `[uint]` FlatBuffers vector and return its offset. */
function uintVector(fbb: flatbuffers.Builder, values: number[]): flatbuffers.Offset {
  fbb.startVector(4, values.length, 4);
  for (let i = values.length - 1; i >= 0; i--) fbb.addInt32(values[i]!);
  return fbb.endVector();
}

/** Map a relay role string to the RelayMarker ubyte (Both=0, Read=1, Write=2, Indexer=3),
* mirroring `RelayMarker::from_role_string` in `nmp-router` EXACTLY — including rejection.
* Unknown tokens or no-flag input (e.g. empty string) encode as 255 (out-of-range sentinel)
* so the Rust decoder (`marker_from_wire`) fails closed instead of silently becoming Both.
* Role strings may be comma-separated (e.g. `"both,indexer"`); comparisons are case-insensitive. */
function relayMarkerByte(role: string): number {
  let hasBoth = false, hasRead = false, hasWrite = false, hasIndexer = false;
  let invalid = false;
  for (const part of role.split(",").map((s) => s.trim().toLowerCase())) {
    if (part === "") { /* no-op: empty part (e.g. trailing comma) matches Rust */ }
    else if (part === "both") hasBoth = true;
    else if (part === "read") hasRead = true;
    else if (part === "write") hasWrite = true;
    else if (part === "indexer") hasIndexer = true;
    else invalid = true;
  }
  if (invalid) return 255;
  if (hasBoth || (hasRead && hasWrite)) return 0;
  if (hasRead) return 1;
  if (hasWrite) return 2;
  if (hasIndexer) return 3;
  return 255;
}

export const GeneratedActionBuilders = {
  /** Publish an app-private note event. */
  publishNote(
    correlationId: string,
    title: string,
    retryCount: number,
    topics: string[] | null,
  ): Uint8Array {
    const fbb = new flatbuffers.Builder(64);
    const titleOffset = fbb.createString(title);
    const topicsOffset =
      topics === null || topics.length === 0 ? 0 : stringVector(fbb, topics);
    fbb.startObject(4);
    fbb.addFieldInt32(0, 42, 0); // slot 0: schema_version
    fbb.addFieldOffset(1, titleOffset, 0); // slot 1: title
    fbb.addFieldInt32(2, retryCount, 0); // slot 2: retryCount
    if (topicsOffset !== 0) fbb.addFieldOffset(3, topicsOffset, 0); // slot 3: topics
    const payloadRoot = fbb.endObject();
    fbb.finish(payloadRoot, "APPA");
    const payload = fbb.asUint8Array();
    return encodeDispatchEnvelope(correlationId, "app.notes.publish_note", payload);
  },

};
