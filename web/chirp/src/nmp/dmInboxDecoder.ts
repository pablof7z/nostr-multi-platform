import * as flatbuffers from "flatbuffers";
import { FrameKind } from "./generated/nmp/transport/frame-kind";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";

const DM_INBOX_KEY = "nmp.nip17.dm_inbox";
const DM_INBOX_FILE_ID = "NDMI";

export type DmMessageItem = {
  id: string;
  senderPubkey: string;
  content: string;
  createdAt: number;
  replyTo?: string;
  isOutgoing: boolean;
  sourceRelays: string[];
};

export type DmConversationItem = {
  peerPubkey: string;
  messages: DmMessageItem[];
};

export type DmInboxFrame = {
  conversations: DmConversationItem[];
  decryptState: "unavailable" | "limited" | "ok" | string;
  undecryptedCount: number;
};

export function decodeDmInboxFrame(bytes: Uint8Array | undefined): DmInboxFrame | undefined {
  if (!bytes) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!UpdateFrame.bufferHasIdentifier(bb)) return undefined;
    const frame = UpdateFrame.getRootAsUpdateFrame(bb);
    if (frame.kind() !== FrameKind.Snapshot) return undefined;
    const snap = frame.snapshot();
    return snap ? decodeSnapshot(snap) : undefined;
  } catch {
    return undefined;
  }
}

function decodeSnapshot(snap: SnapshotFrame): DmInboxFrame | undefined {
  for (let i = 0; i < snap.typedProjectionsLength(); i++) {
    const projection = snap.typedProjections(i);
    const payload = projection?.payload();
    const bytes = payload?.payloadArray();
    if (!projection || !payload || !bytes || bytes.length === 0) continue;
    if (projection.key() !== DM_INBOX_KEY || payload.fileIdentifier() !== DM_INBOX_FILE_ID) {
      continue;
    }
    return decodePayload(bytes);
  }
  return undefined;
}

function decodePayload(bytes: Uint8Array): DmInboxFrame | undefined {
  const bb = new flatbuffers.ByteBuffer(bytes);
  if (!bb.__has_identifier(DM_INBOX_FILE_ID)) return undefined;
  const root = bb.readInt32(bb.position()) + bb.position();
  return {
    conversations: decodeConversations(bb, root),
    decryptState: stringField(bb, root, 6) || "unavailable",
    undecryptedCount: uint32Field(bb, root, 8),
  };
}

function decodeConversations(
  bb: flatbuffers.ByteBuffer,
  root: number,
): DmConversationItem[] {
  const offset = bb.__offset(root, 4);
  if (!offset) return [];
  const vector = bb.__vector(root + offset);
  const length = bb.__vector_len(root + offset);
  const conversations: DmConversationItem[] = [];
  for (let i = 0; i < length; i++) {
    const table = bb.__indirect(vector + i * 4);
    const peerPubkey = stringField(bb, table, 4);
    if (!peerPubkey) continue;
    conversations.push({
      peerPubkey,
      messages: decodeMessages(bb, table),
    });
  }
  return conversations;
}

function decodeMessages(bb: flatbuffers.ByteBuffer, conversation: number): DmMessageItem[] {
  const offset = bb.__offset(conversation, 6);
  if (!offset) return [];
  const vector = bb.__vector(conversation + offset);
  const length = bb.__vector_len(conversation + offset);
  const messages: DmMessageItem[] = [];
  for (let i = 0; i < length; i++) {
    const table = bb.__indirect(vector + i * 4);
    const id = stringField(bb, table, 4);
    const senderPubkey = stringField(bb, table, 6);
    const content = stringField(bb, table, 8);
    if (!id || !senderPubkey || content === undefined) continue;
    messages.push({
      id,
      senderPubkey,
      content,
      createdAt: numberFromBigint(uint64Field(bb, table, 10)),
      replyTo: boolField(bb, table, 12) ? stringField(bb, table, 14) : undefined,
      isOutgoing: boolField(bb, table, 16),
      sourceRelays: stringVectorField(bb, table, 18),
    });
  }
  return messages;
}

function stringVectorField(
  bb: flatbuffers.ByteBuffer,
  table: number,
  slot: number,
): string[] {
  const offset = bb.__offset(table, slot);
  if (!offset) return [];
  const vector = bb.__vector(table + offset);
  const length = bb.__vector_len(table + offset);
  const values: string[] = [];
  for (let i = 0; i < length; i++) {
    const value = bb.__string(vector + i * 4);
    if (typeof value === "string") values.push(value);
  }
  return values;
}

function stringField(
  bb: flatbuffers.ByteBuffer,
  table: number,
  slot: number,
): string | undefined {
  const offset = bb.__offset(table, slot);
  if (!offset) return undefined;
  const value = bb.__string(table + offset);
  return typeof value === "string" ? value : undefined;
}

function boolField(bb: flatbuffers.ByteBuffer, table: number, slot: number): boolean {
  const offset = bb.__offset(table, slot);
  return offset ? bb.readInt8(table + offset) !== 0 : false;
}

function uint32Field(bb: flatbuffers.ByteBuffer, table: number, slot: number): number {
  const offset = bb.__offset(table, slot);
  return offset ? bb.readUint32(table + offset) : 0;
}

function uint64Field(bb: flatbuffers.ByteBuffer, table: number, slot: number): bigint | null {
  const offset = bb.__offset(table, slot);
  return offset ? bb.readUint64(table + offset) : null;
}

function numberFromBigint(value: bigint | null): number {
  if (value === null) return 0;
  const max = BigInt(Number.MAX_SAFE_INTEGER);
  return Number(value > max ? max : value);
}
