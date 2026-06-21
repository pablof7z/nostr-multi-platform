import { describe, expect, it } from "vitest";
import goldenHex from "../../../../crates/nmp-nip01/tests/fixtures/op_feed_populated_v1.fb.hex?raw";
import * as flatbuffers from "flatbuffers";
import { FrameKind, SnapshotFrame, UpdateFrame } from "@nmp/wire-ts/nmp/transport";
import { TypedPayload } from "@nmp/wire-ts/nmp/transport/typed-payload";
import { TypedProjection } from "@nmp/wire-ts/nmp/transport/typed-projection";
import { decodeHomeFeed, decodeOpFeedSnapshot } from "./feedProjection";

// ── helpers ─────────────────────────────────────────────────────────────────

/** Parse the checked-in hex fixture into bytes (strips whitespace). */
function loadGoldenBytes(): Uint8Array {
  const hex = goldenHex.replace(/\s+/g, "");
  if (hex.length % 2 !== 0) {
    throw new Error("hex fixture must contain full bytes");
  }
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
}

/** Repeat a two-char hex string 32 times — mirrors Rust `hex32(byte)`. */
function hex32(byte: number): string {
  return byte.toString(16).padStart(2, "0").repeat(32);
}

/**
 * Build an `UpdateFrame > SnapshotFrame` binary whose one TypedProjection
 * carries the supplied NOFS bytes as its payload.
 *
 * This exercises the full `decodeHomeFeed` path end-to-end, matching the
 * exact runtime path the web worker will follow when `nmp.feed.home` is
 * live (PR-F1 + PR-F3).
 */
function makeFrameWithFeedProjection(nofsBytes: Uint8Array): Uint8Array {
  const builder = new flatbuffers.Builder(nofsBytes.length + 512);

  // Strings and byte vectors must be written before the tables that reference them.
  const keyOff = builder.createString("nmp.feed.home");
  const schemaIdOff = builder.createString("nmp.nip01.opfeed");
  const fileIdentOff = builder.createString("NOFS");
  const payloadVecOff = TypedPayload.createPayloadVector(builder, nofsBytes);

  // TypedPayload
  TypedPayload.startTypedPayload(builder);
  TypedPayload.addSchemaId(builder, schemaIdOff);
  TypedPayload.addSchemaVersion(builder, 1);
  TypedPayload.addFileIdentifier(builder, fileIdentOff);
  TypedPayload.addPayload(builder, payloadVecOff);
  const payloadOff = TypedPayload.endTypedPayload(builder);

  // TypedProjection
  TypedProjection.startTypedProjection(builder);
  TypedProjection.addKey(builder, keyOff);
  TypedProjection.addPayload(builder, payloadOff);
  const projOff = TypedProjection.endTypedProjection(builder);

  const projVecOff = SnapshotFrame.createTypedProjectionsVector(builder, [projOff]);

  // SnapshotFrame
  SnapshotFrame.startSnapshotFrame(builder);
  SnapshotFrame.addSchemaVersion(builder, 1);
  SnapshotFrame.addRunning(builder, true);
  SnapshotFrame.addTypedProjections(builder, projVecOff);
  const snapOff = SnapshotFrame.endSnapshotFrame(builder);

  // UpdateFrame (root)
  UpdateFrame.startUpdateFrame(builder);
  UpdateFrame.addKind(builder, FrameKind.Snapshot);
  UpdateFrame.addSnapshot(builder, snapOff);
  const frameOff = UpdateFrame.endUpdateFrame(builder);
  UpdateFrame.finishUpdateFrameBuffer(builder, frameOff);

  return builder.asUint8Array();
}

// ── tests ────────────────────────────────────────────────────────────────────

describe("decodeOpFeedSnapshot (NOFS golden buffer — op_feed_populated_v1)", () => {
  it("decodes 2 root cards from the populated golden fixture", () => {
    const result = decodeOpFeedSnapshot(loadGoldenBytes());
    expect(result).toBeDefined();
    expect(result!.items).toHaveLength(2);
  });

  it("card 0 — root card: id, author, content, created_at", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    const card = items[0]!;
    expect(card.id).toBe(hex32(0x03));
    expect(card.authorPubkey).toBe(hex32(0x04));
    expect(card.content).toBe("a thread root");
    expect(card.createdAt).toBe(1_700_000_500);
  });

  it("card 0 — no repostedBy (plain root note)", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    expect(items[0]!.repostedBy).toBeUndefined();
  });

  it("card 0 — two attribution entries (ADR-0035)", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    expect(items[0]!.attribution).toHaveLength(2);
  });

  it("card 0, attribution[0] — display name + picture URL present (Alice, byte=0x10)", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    const a0 = items[0]!.attribution[0]!;
    expect(a0.authorPubkey).toBe(hex32(0x10));
    expect(a0.authorDisplayName).toBe("Alice");
    expect(a0.authorPictureUrl).toBe("https://example.com/a.png");
    expect(a0.replyEventId).toBe(hex32(0x90));
    expect(a0.replyCreatedAt).toBe(1_700_000_916);
  });

  it("card 0, attribution[1] — display name absent (no kind:0 yet, byte=0x11)", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    const a1 = items[0]!.attribution[1]!;
    expect(a1.authorPubkey).toBe(hex32(0x11));
    expect(a1.authorDisplayName).toBeUndefined();
    expect(a1.authorPictureUrl).toBeUndefined();
    expect(a1.replyEventId).toBe(hex32(0x91));
    expect(a1.replyCreatedAt).toBe(1_700_000_917);
  });

  it("card 1 — repost card: id, author, content, created_at", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    const card = items[1]!;
    expect(card.id).toBe(hex32(0x09));
    expect(card.authorPubkey).toBe(hex32(0x02));
    expect(card.content).toBe("hello world");
    expect(card.createdAt).toBe(1_700_000_000);
  });

  it("card 1 — repostedBy attribution present (NIP-18, byte=0x42)", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    const repost = items[1]!.repostedBy;
    expect(repost).toBeDefined();
    expect(repost!.authorPubkey).toBe(hex32(0x42));
  });

  it("card 1 — no reply attribution (empty vector)", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    expect(items[1]!.attribution).toHaveLength(0);
  });

  it("card 0 — relation counts: replies Known(1), reposts Known(0), zaps Loading", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    const rc = items[0]!.relationCounts;
    expect(rc.replies).toEqual({ type: "known", count: 1 });
    expect(rc.reposts).toEqual({ type: "known", count: 0 });
    expect(rc.zaps).toEqual({ type: "loading" });
  });

  it("card 1 — relation counts: replies Known(2), reposts Known(1), reactions Loading", () => {
    const { items } = decodeOpFeedSnapshot(loadGoldenBytes())!;
    const rc = items[1]!.relationCounts;
    expect(rc.replies).toEqual({ type: "known", count: 2 });
    expect(rc.reposts).toEqual({ type: "known", count: 1 });
    expect(rc.reactions).toEqual({ type: "loading" });
  });
});

describe("decodeOpFeedSnapshot — rejection / degradation", () => {
  it("returns undefined for an empty buffer", () => {
    expect(decodeOpFeedSnapshot(new Uint8Array(0))).toBeUndefined();
  });

  it("returns undefined when the NOFS file identifier is missing", () => {
    // Build a minimal FlatBuffers buffer without the NOFS magic bytes.
    const builder = new flatbuffers.Builder(32);
    builder.startObject(0);
    const off = builder.endObject();
    builder.finish(off);
    expect(decodeOpFeedSnapshot(builder.asUint8Array())).toBeUndefined();
  });
});

describe("decodeHomeFeed (end-to-end via SnapshotFrame)", () => {
  it("finds nmp.feed.home and returns the same card list as decodeOpFeedSnapshot", () => {
    const nofsBytes = loadGoldenBytes();
    const frameBytes = makeFrameWithFeedProjection(nofsBytes);

    // Navigate to SnapshotFrame via UpdateFrame (mirroring the runtime path).
    const buf = new flatbuffers.ByteBuffer(frameBytes);
    const frame = UpdateFrame.getRootAsUpdateFrame(buf);
    const snapshot = frame.snapshot()!;
    expect(snapshot).not.toBeNull();

    const result = decodeHomeFeed(snapshot);
    expect(result).toBeDefined();
    expect(result!.items).toHaveLength(2);
    expect(result!.items[0]!.id).toBe(hex32(0x03));
    expect(result!.items[1]!.id).toBe(hex32(0x09));
  });

  it("returns undefined when the projection key is absent", () => {
    // Build a SnapshotFrame with no typed_projections.
    const builder = new flatbuffers.Builder(64);
    SnapshotFrame.startSnapshotFrame(builder);
    SnapshotFrame.addSchemaVersion(builder, 1);
    const snapOff = SnapshotFrame.endSnapshotFrame(builder);
    UpdateFrame.startUpdateFrame(builder);
    UpdateFrame.addKind(builder, FrameKind.Snapshot);
    UpdateFrame.addSnapshot(builder, snapOff);
    const frameOff = UpdateFrame.endUpdateFrame(builder);
    UpdateFrame.finishUpdateFrameBuffer(builder, frameOff);

    const buf = new flatbuffers.ByteBuffer(builder.asUint8Array());
    const frame = UpdateFrame.getRootAsUpdateFrame(buf);
    const snap = frame.snapshot()!;
    expect(decodeHomeFeed(snap)).toBeUndefined();
  });

  it("returns undefined when the NOFS descriptor has the wrong schema_id", () => {
    const nofsBytes = loadGoldenBytes();
    const builder = new flatbuffers.Builder(nofsBytes.length + 512);

    const keyOff = builder.createString("nmp.feed.home");
    const schemaIdOff = builder.createString("nmp.wrong.schema");
    const fileIdentOff = builder.createString("NOFS");
    const payloadVecOff = TypedPayload.createPayloadVector(builder, nofsBytes);

    TypedPayload.startTypedPayload(builder);
    TypedPayload.addSchemaId(builder, schemaIdOff);
    TypedPayload.addSchemaVersion(builder, 1);
    TypedPayload.addFileIdentifier(builder, fileIdentOff);
    TypedPayload.addPayload(builder, payloadVecOff);
    const payloadOff = TypedPayload.endTypedPayload(builder);

    TypedProjection.startTypedProjection(builder);
    TypedProjection.addKey(builder, keyOff);
    TypedProjection.addPayload(builder, payloadOff);
    const projOff = TypedProjection.endTypedProjection(builder);

    const projVecOff = SnapshotFrame.createTypedProjectionsVector(builder, [projOff]);
    SnapshotFrame.startSnapshotFrame(builder);
    SnapshotFrame.addSchemaVersion(builder, 1);
    SnapshotFrame.addTypedProjections(builder, projVecOff);
    const snapOff = SnapshotFrame.endSnapshotFrame(builder);
    UpdateFrame.startUpdateFrame(builder);
    UpdateFrame.addKind(builder, FrameKind.Snapshot);
    UpdateFrame.addSnapshot(builder, snapOff);
    const frameOff = UpdateFrame.endUpdateFrame(builder);
    UpdateFrame.finishUpdateFrameBuffer(builder, frameOff);

    const buf = new flatbuffers.ByteBuffer(builder.asUint8Array());
    const snap = UpdateFrame.getRootAsUpdateFrame(buf).snapshot()!;
    expect(decodeHomeFeed(snap)).toBeUndefined();
  });

  it("returns undefined when the inner buffer lacks the NOFS file identifier", () => {
    // Replace the golden NOFS bytes with garbage that lacks the NOFS magic.
    const badBytes = new Uint8Array(16); // zeros — no file identifier
    const frameBytes = makeFrameWithFeedProjection(badBytes);
    const buf = new flatbuffers.ByteBuffer(frameBytes);
    const snap = UpdateFrame.getRootAsUpdateFrame(buf).snapshot()!;
    // Descriptor says NOFS, but inner buffer lacks the 4-byte magic.
    expect(decodeHomeFeed(snap)).toBeUndefined();
  });
});
