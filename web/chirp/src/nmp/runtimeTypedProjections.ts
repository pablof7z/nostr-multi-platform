// Decode kernel-owned typed projections used by runtime diagnostics.
//
// The web host only maps FlatBuffers fields into view data. It does not infer
// publish policy, relay routing, or action lifecycle semantics.

import * as flatbuffers from "flatbuffers";
import { ActionResultsSnapshot } from "./generated/nmp/kernel/action-results-snapshot";
import { ActionStagesSnapshot } from "./generated/nmp/kernel/action-stages-snapshot";
import { PublishOutboxSnapshot } from "./generated/nmp/kernel/publish-outbox-snapshot";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";

const PUBLISH_OUTBOX_KEY = "publish_outbox";
const ACTION_RESULTS_KEY = "action_results";
const ACTION_STAGES_KEY = "action_stages";
const FOLLOW_LIST_KEY = "nmp.follow_list";
const ACTIVE_ACCOUNT_KEY = "active_account";
const BOOKMARKS_KEY = "nmp.nip51.bookmarks";
const PUBLISH_OUTBOX_FILE_ID = "KPBO";
const ACTION_RESULTS_FILE_ID = "KARS";
const ACTION_STAGES_FILE_ID = "KAST";
const FOLLOW_LIST_FILE_ID = "NF02";
const ACTIVE_ACCOUNT_FILE_ID = "KACT";
const BOOKMARKS_FILE_ID = "N51L";

export type PublishOutboxRelayRuntimeRow = {
  relayUrl: string;
  status: string;
  attempt: number;
  message?: string;
  relayReason?: string;
};

export type PublishOutboxRuntimeItem = {
  handle: string;
  eventId: string;
  kind: number;
  status: string;
  canRetry: boolean;
  targetRelays: number;
  createdAt: number;
  content: string;
  relays: PublishOutboxRelayRuntimeRow[];
};

export type ActionResultRuntimeRow = {
  correlationId: string;
  status: string;
  error?: string;
  result?: string;
  eventId?: string;
};

export type ActionStageRuntimeRow = {
  correlationId: string;
  stage: string;
  atMs: number;
  reason?: string;
  detail?: string;
};

export type TypedRuntimeProjections = {
  publishOutbox: PublishOutboxRuntimeItem[];
  actionResults: ActionResultRuntimeRow[];
  actionStages: ActionStageRuntimeRow[];
  followList: string[];
  activeAccountPubkey?: string;
  bookmarkedEventIds: string[];
};

function numberFromBigint(value: bigint | null): number {
  if (value === null) return 0;
  const max = BigInt(Number.MAX_SAFE_INTEGER);
  return Number(value > max ? max : value);
}

export function decodeTypedRuntimeProjections(snap: SnapshotFrame): TypedRuntimeProjections {
  const result: TypedRuntimeProjections = {
    publishOutbox: [],
    actionResults: [],
    actionStages: [],
    followList: [],
    bookmarkedEventIds: [],
  };

  for (let i = 0; i < snap.typedProjectionsLength(); i++) {
    const projection = snap.typedProjections(i);
    const payload = projection?.payload();
    const bytes = payload?.payloadArray();
    if (!projection || !payload || !bytes || bytes.length === 0) continue;
    const key = projection.key();
    const fileId = payload.fileIdentifier();

    if (key === PUBLISH_OUTBOX_KEY && fileId === PUBLISH_OUTBOX_FILE_ID) {
      result.publishOutbox = decodePublishOutbox(bytes);
    } else if (key === ACTION_RESULTS_KEY && fileId === ACTION_RESULTS_FILE_ID) {
      result.actionResults = decodeActionResults(bytes);
    } else if (key === ACTION_STAGES_KEY && fileId === ACTION_STAGES_FILE_ID) {
      result.actionStages = decodeActionStages(bytes);
    } else if (key === FOLLOW_LIST_KEY && fileId === FOLLOW_LIST_FILE_ID) {
      result.followList = decodeFollowList(bytes);
    } else if (key === ACTIVE_ACCOUNT_KEY && fileId === ACTIVE_ACCOUNT_FILE_ID) {
      result.activeAccountPubkey = decodeActiveAccount(bytes);
    } else if (key === BOOKMARKS_KEY && fileId === BOOKMARKS_FILE_ID) {
      result.bookmarkedEventIds = decodeBookmarkEventIds(bytes);
    }
  }

  return result;
}

function decodeActiveAccount(bytes: Uint8Array): string | undefined {
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!bb.__has_identifier(ACTIVE_ACCOUNT_FILE_ID)) return undefined;
    const root = bb.readInt32(bb.position()) + bb.position();
    const hasOffset = bb.__offset(root, 4);
    if (!hasOffset || !bb.readInt8(root + hasOffset)) return undefined;
    const pubkeyOffset = bb.__offset(root, 6);
    if (!pubkeyOffset) return undefined;
    const pubkey = bb.__string(root + pubkeyOffset);
    return typeof pubkey === "string" && pubkey.length > 0 ? pubkey : undefined;
  } catch {
    return undefined;
  }
}

function decodeBookmarkEventIds(bytes: Uint8Array): string[] {
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!bb.__has_identifier(BOOKMARKS_FILE_ID)) return [];
    const root = bb.readInt32(bb.position()) + bb.position();
    const itemsOffset = bb.__offset(root, 4);
    if (!itemsOffset) return [];
    const items = bb.__vector(root + itemsOffset);
    const length = bb.__vector_len(root + itemsOffset);
    const ids: string[] = [];
    for (let i = 0; i < length; i++) {
      const item = bb.__indirect(items + i * 4);
      const kindOffset = bb.__offset(item, 4);
      const valueOffset = bb.__offset(item, 6);
      if (!valueOffset) continue;
      const kind = kindOffset ? bb.readUint8(item + kindOffset) : 0;
      if (kind !== 0) continue;
      const value = bb.__string(item + valueOffset);
      if (typeof value === "string") ids.push(value);
    }
    return ids;
  } catch {
    return [];
  }
}

function decodeFollowList(bytes: Uint8Array): string[] {
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!bb.__has_identifier(FOLLOW_LIST_FILE_ID)) return [];
    const root = bb.readInt32(bb.position()) + bb.position();
    const followsOffset = bb.__offset(root, 4);
    if (!followsOffset) return [];
    const follows = bb.__vector(root + followsOffset);
    const length = bb.__vector_len(root + followsOffset);
    const pubkeys: string[] = [];
    for (let i = 0; i < length; i++) {
      const entry = bb.__indirect(follows + i * 4);
      const pubkeyOffset = bb.__offset(entry, 4);
      if (!pubkeyOffset) continue;
      const pubkey = bb.__string(entry + pubkeyOffset);
      if (typeof pubkey === "string") {
        pubkeys.push(pubkey);
      }
    }
    return pubkeys;
  } catch {
    return [];
  }
}

function decodePublishOutbox(bytes: Uint8Array): PublishOutboxRuntimeItem[] {
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!PublishOutboxSnapshot.bufferHasIdentifier(bb)) return [];
    const snapshot = PublishOutboxSnapshot.getRootAsPublishOutboxSnapshot(bb);
    const items: PublishOutboxRuntimeItem[] = [];
    for (let i = 0; i < snapshot.itemsLength(); i++) {
      const item = snapshot.items(i);
      if (!item) continue;
      const relays: PublishOutboxRelayRuntimeRow[] = [];
      for (let j = 0; j < item.relaysLength(); j++) {
        const relay = item.relays(j);
        if (!relay) continue;
        relays.push({
          relayUrl: relay.relayUrl() ?? "unknown relay",
          status: relay.status() ?? "unknown",
          attempt: relay.attempt(),
          message: relay.message() || undefined,
          relayReason: relay.relayReason() || undefined,
        });
      }
      items.push({
        handle: item.handle() ?? "",
        eventId: item.eventId() ?? "",
        kind: item.kind(),
        status: item.status() ?? "unknown",
        canRetry: item.canRetry(),
        targetRelays: item.targetRelays(),
        createdAt: numberFromBigint(item.createdAt()),
        content: item.content() ?? "",
        relays,
      });
    }
    return items;
  } catch {
    return [];
  }
}

function decodeActionResults(bytes: Uint8Array): ActionResultRuntimeRow[] {
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!ActionResultsSnapshot.bufferHasIdentifier(bb)) return [];
    const snapshot = ActionResultsSnapshot.getRootAsActionResultsSnapshot(bb);
    const rows: ActionResultRuntimeRow[] = [];
    for (let i = 0; i < snapshot.resultsLength(); i++) {
      const row = snapshot.results(i);
      if (!row) continue;
      rows.push({
        correlationId: row.correlationId() ?? "",
        status: row.status() ?? "unknown",
        error: row.hasError() ? row.error() ?? "" : undefined,
        result: row.hasResult() ? row.result() ?? "" : undefined,
        eventId: row.hasEventId() ? row.eventId() ?? "" : undefined,
      });
    }
    return rows;
  } catch {
    return [];
  }
}

function decodeActionStages(bytes: Uint8Array): ActionStageRuntimeRow[] {
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!ActionStagesSnapshot.bufferHasIdentifier(bb)) return [];
    const snapshot = ActionStagesSnapshot.getRootAsActionStagesSnapshot(bb);
    const rows: ActionStageRuntimeRow[] = [];
    for (let i = 0; i < snapshot.entriesLength(); i++) {
      const entry = snapshot.entries(i);
      if (!entry) continue;
      const correlationId = entry.key() ?? "";
      for (let j = 0; j < entry.stagesLength(); j++) {
        const stage = entry.stages(j);
        if (!stage) continue;
        rows.push({
          correlationId,
          stage: stage.stage() ?? "unknown",
          atMs: numberFromBigint(stage.atMs()),
          reason: stage.hasReason() ? stage.reason() ?? "" : undefined,
          detail: stage.hasDetail() ? stage.detail() ?? "" : undefined,
        });
      }
    }
    return rows;
  } catch {
    return [];
  }
}
