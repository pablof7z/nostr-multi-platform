// runtimeProjection.ts — decode runtime-owned snapshot metadata for product UX.
//
// This module only decodes FlatBuffers bytes emitted by the Rust runtime. It
// does not infer relay policy, mutate runtime state, or construct Nostr data.

import * as flatbuffers from "flatbuffers";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";
import { FrameKind } from "./generated/nmp/transport/frame-kind";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";
import {
  decodeTypedRuntimeProjections,
  type ActionResultRuntimeRow,
  type ActionStageRuntimeRow,
  type PublishOutboxRuntimeItem,
} from "./runtimeTypedProjections";

export type RelayRuntimeRow = {
  url: string;
  role: string;
  connection: string;
  auth: string;
  subscriptions: number;
  eventsRx: number;
  reconnects: number;
  lastNotice?: string;
  lastError?: string;
  errorCategory?: string;
};

export type InterestRuntimeRow = {
  key: string;
  state: string;
  refcount: number;
  relayCount: number;
  cacheCoverage?: string;
};

export type WireSubscriptionRuntimeRow = {
  wireId: string;
  relayUrl: string;
  filterSummary: string;
  state: string;
  logicalConsumerCount: number;
  eventsRx: number;
  eoseObserved: boolean;
};

export type { ActionResultRuntimeRow, ActionStageRuntimeRow, PublishOutboxRuntimeItem };

export type RuntimeProjection = {
  running: boolean;
  rev: number;
  relays: RelayRuntimeRow[];
  interests: InterestRuntimeRow[];
  wireSubscriptions: WireSubscriptionRuntimeRow[];
  publishOutbox: PublishOutboxRuntimeItem[];
  actionResults: ActionResultRuntimeRow[];
  actionStages: ActionStageRuntimeRow[];
  followList: string[];
  activeAccountPubkey?: string;
  bookmarkedEventIds: string[];
  lastErrorToast?: string;
  lastErrorCategory?: string;
  lastPlannerError?: string;
  storeOpenFailure?: string;
  noConfiguredRelays: boolean;
};

function numberFromBigint(value: bigint | null): number {
  if (value === null) return 0;
  const max = BigInt(Number.MAX_SAFE_INTEGER);
  return Number(value > max ? max : value);
}

export function decodeRuntimeProjection(bytes: Uint8Array | undefined): RuntimeProjection | undefined {
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

function decodeSnapshot(snap: SnapshotFrame): RuntimeProjection {
  const relays: RelayRuntimeRow[] = [];
  for (let i = 0; i < snap.relayStatusesLength(); i++) {
    const row = snap.relayStatuses(i);
    if (!row) continue;
    relays.push({
      url: row.relayUrl() ?? "unknown relay",
      role: row.role() ?? "unknown",
      connection: row.connection() ?? "unknown",
      auth: row.auth() ?? "unknown",
      subscriptions: numberFromBigint(row.activeWireSubscriptions()),
      eventsRx: numberFromBigint(row.eventsRx()),
      reconnects: row.reconnectCount(),
      lastNotice: row.lastNotice() ?? undefined,
      lastError: row.lastError() ?? undefined,
      errorCategory: row.errorCategory() ?? undefined,
    });
  }

  const interests: InterestRuntimeRow[] = [];
  for (let i = 0; i < snap.logicalInterestsLength(); i++) {
    const row = snap.logicalInterests(i);
    if (!row) continue;
    interests.push({
      key: row.key() ?? "unknown interest",
      state: row.state() ?? "unknown",
      refcount: row.refcount(),
      relayCount: row.relayUrlsLength(),
      cacheCoverage: row.cacheCoverage() ?? undefined,
    });
  }

  const wireSubscriptions: WireSubscriptionRuntimeRow[] = [];
  for (let i = 0; i < snap.wireSubscriptionsLength(); i++) {
    const row = snap.wireSubscriptions(i);
    if (!row) continue;
    wireSubscriptions.push({
      wireId: row.wireId() ?? "unknown",
      relayUrl: row.relayUrl() ?? "unknown relay",
      filterSummary: row.filterSummary() ?? "unknown filter",
      state: row.state() ?? "unknown",
      logicalConsumerCount: row.logicalConsumerCount(),
      eventsRx: numberFromBigint(row.eventsRx()),
      eoseObserved: row.eoseAtMs() !== null,
    });
  }
  const typed = decodeTypedRuntimeProjections(snap);

  return {
    running: snap.running(),
    rev: numberFromBigint(snap.rev()),
    relays,
    interests,
    wireSubscriptions,
    publishOutbox: typed.publishOutbox,
    actionResults: typed.actionResults,
    actionStages: typed.actionStages,
    followList: typed.followList,
    activeAccountPubkey: typed.activeAccountPubkey,
    bookmarkedEventIds: typed.bookmarkedEventIds,
    lastErrorToast: snap.lastErrorToast() ?? undefined,
    lastErrorCategory: snap.lastErrorCategory() ?? undefined,
    lastPlannerError: snap.lastPlannerError() ?? undefined,
    storeOpenFailure: snap.storeOpenFailure() ?? undefined,
    noConfiguredRelays: snap.noConfiguredRelays() === true,
  };
}
