import { describe, expect, it } from "vitest";
import * as flatbuffers from "flatbuffers";

import { ProfileCard } from "./generated/nmp/kernel/profile-card";
import { ProfileSnapshot } from "./generated/nmp/kernel/profile-snapshot";
import { RefRow } from "./generated/nmp/refs/ref-row";
import { RefRowDeltaBatch } from "./generated/nmp/refs/ref-row-delta-batch";
import { RefRowState } from "./generated/nmp/refs/ref-row-state";
import { KeyedRefCache } from "./keyedRefCache.generated";
import { refRowProfile } from "./refRowDecoders";

function encodeProfileSnapshot(pubkey: string, displayName?: string): Uint8Array {
  const builder = new flatbuffers.Builder();
  const pubkeyOff = builder.createString(pubkey);
  const displayNameOff = displayName ? builder.createString(displayName) : 0;
  ProfileCard.startProfileCard(builder);
  ProfileCard.addPubkey(builder, pubkeyOff);
  if (displayNameOff) {
    ProfileCard.addHasDisplayName(builder, true);
    ProfileCard.addDisplayName(builder, displayNameOff);
  }
  const cardOff = ProfileCard.endProfileCard(builder);
  ProfileSnapshot.startProfileSnapshot(builder);
  ProfileSnapshot.addCard(builder, cardOff);
  const root = ProfileSnapshot.endProfileSnapshot(builder);
  ProfileSnapshot.finishProfileSnapshotBuffer(builder, root);
  return builder.asUint8Array().slice();
}

function encodeProfileRowDeltaBatch(rows: { pubkey: string; displayName?: string; rev: number }[]): Uint8Array {
  const builder = new flatbuffers.Builder();
  const rowOffsets = rows.map((r) => {
    const keyOff = builder.createString(r.pubkey);
    const payloadOff = RefRow.createPayloadVector(
      builder,
      encodeProfileSnapshot(r.pubkey, r.displayName),
    );
    RefRow.startRefRow(builder);
    RefRow.addKey(builder, keyOff);
    RefRow.addRev(builder, BigInt(r.rev));
    RefRow.addState(builder, RefRowState.Changed);
    RefRow.addPayload(builder, payloadOff);
    return RefRow.endRefRow(builder);
  });
  const rowsVec = RefRowDeltaBatch.createRowsVector(builder, rowOffsets);
  RefRowDeltaBatch.startRefRowDeltaBatch(builder);
  RefRowDeltaBatch.addBaseline(builder, true);
  RefRowDeltaBatch.addRows(builder, rowsVec);
  const root = RefRowDeltaBatch.endRefRowDeltaBatch(builder);
  RefRowDeltaBatch.finishRefRowDeltaBatchBuffer(builder, root);
  return builder.asUint8Array().slice();
}

describe("refRowProfile glue", () => {
  it("decodes a ProfileSnapshot reader into a ProfileWire", () => {
    const bytes = encodeProfileSnapshot("pubkey-1", "Alice");
    const bb = new flatbuffers.ByteBuffer(bytes);
    const reader = ProfileSnapshot.getRootAsProfileSnapshot(bb);
    const wire = refRowProfile(reader);
    expect(wire).toEqual({ pubkey: "pubkey-1", displayName: "Alice" });
  });

  it("omits unset optional fields rather than emitting empty strings", () => {
    const bytes = encodeProfileSnapshot("pubkey-2");
    const bb = new flatbuffers.ByteBuffer(bytes);
    const reader = ProfileSnapshot.getRootAsProfileSnapshot(bb);
    const wire = refRowProfile(reader);
    expect(wire).toEqual({ pubkey: "pubkey-2" });
  });
});

describe("KeyedRefCache.profile / .profiles (typed accessor, #2722)", () => {
  it("decodes the refs.profile row cache into ProfileWire via the typed accessor", () => {
    const cache = new KeyedRefCache();
    const batch = encodeProfileRowDeltaBatch([
      { pubkey: "a", displayName: "Alice", rev: 1 },
      { pubkey: "b", displayName: "Bob", rev: 1 },
    ]);
    cache.merge("refs.profile", batch, 1n, 1n);

    expect(cache.profile("a")).toEqual({ pubkey: "a", displayName: "Alice" });
    expect(cache.profile("missing")).toBeUndefined();

    const all = cache.profiles();
    expect(all.size).toBe(2);
    expect(all.get("b")).toEqual({ pubkey: "b", displayName: "Bob" });
  });

  it("never exposes a decode-failed row through the typed accessor", () => {
    const cache = new KeyedRefCache();
    const builder = new flatbuffers.Builder();
    const rowsVec = RefRowDeltaBatch.createRowsVector(builder, [
      (() => {
        const keyOff = builder.createString("bad");
        RefRow.startRefRow(builder);
        RefRow.addKey(builder, keyOff);
        RefRow.addRev(builder, 1n);
        RefRow.addState(builder, RefRowState.Changed);
        return RefRow.endRefRow(builder);
      })(),
    ]);
    RefRowDeltaBatch.startRefRowDeltaBatch(builder);
    RefRowDeltaBatch.addBaseline(builder, true);
    RefRowDeltaBatch.addRows(builder, rowsVec);
    const root = RefRowDeltaBatch.endRefRowDeltaBatch(builder);
    RefRowDeltaBatch.finishRefRowDeltaBatchBuffer(builder, root);

    const out = cache.merge("refs.profile", builder.asUint8Array().slice(), 1n, 1n);
    expect(out.decodeFailed).toBe(true);
    expect(cache.needsResync()).toBe(true);
    expect(cache.profile("bad")).toBeUndefined();
    expect(cache.profiles().size).toBe(0);
  });
});
