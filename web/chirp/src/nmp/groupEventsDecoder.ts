import * as flatbuffers from "flatbuffers";
import { GroupEvent } from "./generated/nmp/nip29/group-event";
import { GroupEventsSnapshot } from "./generated/nmp/nip29/group-events-snapshot";
import { FrameKind } from "./generated/nmp/transport/frame-kind";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";

const GROUP_EVENTS_KEY = "nmp.nip29.group_events";
const GROUP_EVENTS_FILE_ID = "NGEV";

export type GroupEventsRow = {
  id: string;
  pubkey: string;
  content: string;
  createdAt: number;
  kind: number;
};

export type GroupEventsFrame = {
  rows: GroupEventsRow[];
};

export function decodeGroupEventsFrame(
  bytes: Uint8Array | undefined,
): GroupEventsFrame | undefined {
  if (!bytes) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!UpdateFrame.bufferHasIdentifier(bb)) return undefined;
    const frame = UpdateFrame.getRootAsUpdateFrame(bb);
    if (frame.kind() !== FrameKind.Snapshot) return undefined;
    const snap = frame.snapshot();
    if (!snap) return undefined;
    for (let i = 0; i < snap.typedProjectionsLength(); i++) {
      const projection = snap.typedProjections(i);
      const payload = projection?.payload();
      const payloadBytes = payload?.payloadArray();
      if (
        projection?.key() !== GROUP_EVENTS_KEY ||
        payload?.fileIdentifier() !== GROUP_EVENTS_FILE_ID ||
        !payloadBytes ||
        payloadBytes.length === 0
      ) {
        continue;
      }
      return decodePayload(payloadBytes);
    }
    return undefined;
  } catch {
    return undefined;
  }
}

function decodePayload(bytes: Uint8Array): GroupEventsFrame | undefined {
  const bb = new flatbuffers.ByteBuffer(bytes);
  if (!GroupEventsSnapshot.bufferHasIdentifier(bb)) return undefined;
  const snapshot = GroupEventsSnapshot.getRootAsGroupEventsSnapshot(bb);
  const rows: GroupEventsRow[] = [];
  for (let i = 0; i < snapshot.eventsLength(); i++) {
    const event = snapshot.events(i);
    const row = event ? decodeEvent(event) : undefined;
    if (row) rows.push(row);
  }
  return { rows };
}

function decodeEvent(event: GroupEvent): GroupEventsRow | undefined {
  const id = event.id();
  const pubkey = event.pubkey();
  const content = event.content();
  if (!id || !pubkey || content === null) return undefined;
  return {
    id,
    pubkey,
    content,
    createdAt: Number(event.createdAt()),
    kind: event.kind(),
  };
}
