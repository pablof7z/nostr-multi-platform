import { describe, expect, it } from "vitest";
import * as flatbuffers from "flatbuffers";

import { ClaimedEvent } from "./generated/nmp/kernel/claimed-event";
import { ClaimedEventEntry } from "./generated/nmp/kernel/claimed-event-entry";
import { ClaimedEventsSnapshot } from "./generated/nmp/kernel/claimed-events-snapshot";
import { RefRow } from "./generated/nmp/refs/ref-row";
import { RefRowDeltaBatch } from "./generated/nmp/refs/ref-row-delta-batch";
import { RefRowState } from "./generated/nmp/refs/ref-row-state";
import { RefEventStore } from "./refEventStore";

describe("RefEventStore", () => {
  it("merges refs.event KCEV row deltas into claimed event cards", () => {
    const store = new RefEventStore();
    const key = "aa".repeat(32);

    const outcome = store.applySidecar(
      refEventBatch([{ key, payload: claimedEventPayload(key) }]),
      1n,
      1n,
    );

    expect(outcome).toEqual({ changedKeys: [key], decodeFailed: false });
    expect(store.event(key)).toMatchObject({
      primaryId: key,
      id: key,
      authorPubkey: "bb".repeat(32),
      kind: 1,
      createdAt: 1_700_000_000,
      content: "quoted hello",
    });
    expect(store.events().get(key)?.tags).toEqual([["e", key]]);
  });

  it("rejects a KCEV row whose entry key does not match the refs.event key", () => {
    const store = new RefEventStore();
    const key = "aa".repeat(32);
    const other = "cc".repeat(32);

    const outcome = store.applySidecar(
      refEventBatch([{ key, payload: claimedEventPayload(other) }]),
      1n,
      1n,
    );

    expect(outcome).toEqual({ changedKeys: [], decodeFailed: true });
    expect(store.event(key)).toBeUndefined();
  });

  it("applies cleared rows to remove resolved events", () => {
    const store = new RefEventStore();
    const key = "aa".repeat(32);
    store.applySidecar(refEventBatch([{ key, payload: claimedEventPayload(key) }]), 1n, 1n);

    const outcome = store.applySidecar(
      refEventBatch([{ key, cleared: true, rev: 2n }], false),
      1n,
      1n,
    );

    expect(outcome).toEqual({ changedKeys: [key], decodeFailed: false });
    expect(store.event(key)).toBeUndefined();
  });
});

function claimedEventPayload(primaryId: string): Uint8Array {
  const builder = new flatbuffers.Builder(256);
  const primaryOff = builder.createString(primaryId);
  const authorOff = builder.createString("bb".repeat(32));
  const contentOff = builder.createString("quoted hello");
  const tagNameOff = builder.createString("e");
  const tagValueOff = builder.createString(primaryId);
  TagRowVector.start(builder, [tagNameOff, tagValueOff]);
  const tagValues = builder.endVector();
  const tagRow = TagRowVector.finishRow(builder, tagValues);
  ClaimedEvent.startTagsVector(builder, 1);
  builder.addOffset(tagRow);
  const tags = builder.endVector();
  ClaimedEvent.startClaimedEvent(builder);
  ClaimedEvent.addPrimaryId(builder, primaryOff);
  ClaimedEvent.addId(builder, primaryOff);
  ClaimedEvent.addAuthorPubkey(builder, authorOff);
  ClaimedEvent.addKind(builder, 1);
  ClaimedEvent.addCreatedAt(builder, 1_700_000_000n);
  ClaimedEvent.addTags(builder, tags);
  ClaimedEvent.addContent(builder, contentOff);
  const event = ClaimedEvent.endClaimedEvent(builder);
  const keyOff = builder.createString(primaryId);
  ClaimedEventEntry.startClaimedEventEntry(builder);
  ClaimedEventEntry.addKey(builder, keyOff);
  ClaimedEventEntry.addValue(builder, event);
  const entry = ClaimedEventEntry.endClaimedEventEntry(builder);
  ClaimedEventsSnapshot.startEntriesVector(builder, 1);
  builder.addOffset(entry);
  const entries = builder.endVector();
  ClaimedEventsSnapshot.startClaimedEventsSnapshot(builder);
  ClaimedEventsSnapshot.addEntries(builder, entries);
  const snapshot = ClaimedEventsSnapshot.endClaimedEventsSnapshot(builder);
  ClaimedEventsSnapshot.finishClaimedEventsSnapshotBuffer(builder, snapshot);
  return builder.asUint8Array();
}

function refEventBatch(
  rows: Array<{ key: string; payload?: Uint8Array; cleared?: boolean; rev?: bigint }>,
  baseline = true,
): Uint8Array {
  const builder = new flatbuffers.Builder(256);
  const rowOffsets = rows.map((row, index) => {
    const keyOff = builder.createString(row.key);
    const payloadOff = RefRow.createPayloadVector(builder, row.payload ?? new Uint8Array(0));
    RefRow.startRefRow(builder);
    RefRow.addKey(builder, keyOff);
    RefRow.addRev(builder, row.rev ?? BigInt(index + 1));
    RefRow.addState(builder, row.cleared ? RefRowState.Cleared : RefRowState.Changed);
    RefRow.addPayload(builder, payloadOff);
    return RefRow.endRefRow(builder);
  });
  const rowsOff = RefRowDeltaBatch.createRowsVector(builder, rowOffsets);
  const namespaceOff = builder.createString("event");
  RefRowDeltaBatch.startRefRowDeltaBatch(builder);
  RefRowDeltaBatch.addNamespace(builder, namespaceOff);
  RefRowDeltaBatch.addBaseline(builder, baseline);
  RefRowDeltaBatch.addRows(builder, rowsOff);
  const batch = RefRowDeltaBatch.endRefRowDeltaBatch(builder);
  RefRowDeltaBatch.finishRefRowDeltaBatchBuffer(builder, batch);
  return builder.asUint8Array();
}

const TagRowVector = {
  start(builder: flatbuffers.Builder, offsets: flatbuffers.Offset[]) {
    builder.startVector(4, offsets.length, 4);
    for (let i = offsets.length - 1; i >= 0; i -= 1) {
      builder.addOffset(offsets[i]!);
    }
  },
  finishRow(builder: flatbuffers.Builder, values: flatbuffers.Offset): flatbuffers.Offset {
    builder.startObject(1);
    builder.addFieldOffset(0, values, 0);
    return builder.endObject();
  },
};
