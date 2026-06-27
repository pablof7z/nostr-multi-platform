import * as flatbuffers from "flatbuffers";
import { GroupTimelineEvent } from "./generated/nmp/nip29/group-timeline-event";
import { GroupTimelineSnapshot } from "./generated/nmp/nip29/group-timeline-snapshot";
import { FrameKind } from "./generated/nmp/transport/frame-kind";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";

const GROUP_TIMELINE_KEY = "nmp.nip29.group_timeline";
const GROUP_TIMELINE_FILE_ID = "NGTL";

export type GroupTimelineRow = {
  id: string;
  pubkey: string;
  content: string;
  createdAt: number;
  kind: number;
};

export type GroupTimelineFrame = {
  rows: GroupTimelineRow[];
};

export function decodeGroupTimelineFrame(
  bytes: Uint8Array | undefined,
): GroupTimelineFrame | undefined {
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
        projection?.key() !== GROUP_TIMELINE_KEY ||
        payload?.fileIdentifier() !== GROUP_TIMELINE_FILE_ID ||
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

function decodePayload(bytes: Uint8Array): GroupTimelineFrame | undefined {
  const bb = new flatbuffers.ByteBuffer(bytes);
  if (!GroupTimelineSnapshot.bufferHasIdentifier(bb)) return undefined;
  const snapshot = GroupTimelineSnapshot.getRootAsGroupTimelineSnapshot(bb);
  const rows: GroupTimelineRow[] = [];
  for (let i = 0; i < snapshot.eventsLength(); i++) {
    const event = snapshot.events(i);
    const row = event ? decodeEvent(event) : undefined;
    if (row) rows.push(row);
  }
  return { rows };
}

function decodeEvent(event: GroupTimelineEvent): GroupTimelineRow | undefined {
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
