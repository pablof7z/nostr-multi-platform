import * as flatbuffers from "flatbuffers";
import { NotificationRow } from "./generated/nmp/relations/notification-row";
import { NotificationsSnapshot } from "./generated/nmp/relations/notifications-snapshot";
import { FrameKind } from "./generated/nmp/transport/frame-kind";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";

const FILE_ID = "NNTF";
const KEY_PREFIX = "nmp.relations.notifications.";

export type NotificationItem = {
  eventId: string;
  actorPubkey: string;
  eventKind: number;
  notificationKind: string;
  createdAt: number;
  content: string;
  targetEventId?: string;
  sourceRelays: string[];
  read: boolean;
};

export type NotificationsFrame = {
  viewerPubkey: string;
  rows: NotificationItem[];
  unreadCount: number;
};

export function decodeNotificationsFrame(
  bytes: Uint8Array | undefined,
  sessionId: string,
): NotificationsFrame | undefined {
  if (!bytes) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!UpdateFrame.bufferHasIdentifier(bb)) return undefined;
    const frame = UpdateFrame.getRootAsUpdateFrame(bb);
    if (frame.kind() !== FrameKind.Snapshot) return undefined;
    const snap = frame.snapshot();
    return snap ? decodeSnapshot(snap, sessionId) : undefined;
  } catch {
    return undefined;
  }
}

function decodeSnapshot(snap: SnapshotFrame, sessionId: string): NotificationsFrame | undefined {
  const key = `${KEY_PREFIX}${sessionId}`;
  for (let i = 0; i < snap.typedProjectionsLength(); i++) {
    const projection = snap.typedProjections(i);
    const payload = projection?.payload();
    const bytes = payload?.payloadArray();
    if (!projection || !payload || !bytes || bytes.length === 0) continue;
    if (projection.key() !== key || payload.fileIdentifier() !== FILE_ID) continue;
    return decodePayload(bytes);
  }
  return undefined;
}

function decodePayload(bytes: Uint8Array): NotificationsFrame | undefined {
  const bb = new flatbuffers.ByteBuffer(bytes);
  if (!NotificationsSnapshot.bufferHasIdentifier(bb)) return undefined;
  const snapshot = NotificationsSnapshot.getRootAsNotificationsSnapshot(bb);
  const rows: NotificationItem[] = [];
  for (let i = 0; i < snapshot.rowsLength(); i++) {
    const row = snapshot.rows(i);
    if (!row) continue;
    rows.push(decodeRow(row));
  }
  return {
    viewerPubkey: snapshot.viewerPubkey() ?? "",
    rows,
    unreadCount: snapshot.unreadCount(),
  };
}

function decodeRow(row: NotificationRow): NotificationItem {
  const sourceRelays: string[] = [];
  for (let i = 0; i < row.sourceRelaysLength(); i++) {
    const relay = row.sourceRelays(i);
    if (relay) sourceRelays.push(relay);
  }
  return {
    eventId: row.eventId() ?? "",
    actorPubkey: row.actorPubkey() ?? "",
    eventKind: row.eventKind(),
    notificationKind: row.notificationKind() ?? "unknown",
    createdAt: numberFromBigint(row.createdAt()),
    content: row.content() ?? "",
    targetEventId: row.targetEventId() ?? undefined,
    sourceRelays,
    read: row.read(),
  };
}

function numberFromBigint(value: bigint | null): number {
  if (value === null) return 0;
  const max = BigInt(Number.MAX_SAFE_INTEGER);
  return Number(value > max ? max : value);
}
