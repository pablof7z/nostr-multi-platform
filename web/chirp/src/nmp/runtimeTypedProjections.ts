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
const PUBLISH_OUTBOX_FILE_ID = "KPBO";
const ACTION_RESULTS_FILE_ID = "KARS";
const ACTION_STAGES_FILE_ID = "KAST";

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
    }
  }

  return result;
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
