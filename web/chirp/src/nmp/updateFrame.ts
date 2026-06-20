import * as flatbuffers from "flatbuffers";

import {
  FrameKind,
  LogicalInterestStatus,
  Metrics,
  RelayStatus,
  UpdateFrame,
  WireSubscriptionStatus,
} from "./generated/nmp/transport";

export const SNAPSHOT_SCHEMA_VERSION = 1;

export type UpdateFrameDecodeErrorKind =
  | "invalid_flatbuffer"
  | "missing_panic_payload"
  | "unexpected_panic_frame"
  | "schema_version_mismatch";

export class UpdateFrameDecodeError extends Error {
  constructor(
    public readonly kind: UpdateFrameDecodeErrorKind,
    message: string,
  ) {
    super(message);
    this.name = "UpdateFrameDecodeError";
  }
}

/** Relay status decoded from the SnapshotFrame Tier-3 `relay_statuses` field.
 *  The optional tone fields are NOT present in the Tier-3 wire format — they
 *  are filled in a second pass from the KRDG typed projection in client.ts. */
export type DecodedRelayStatus = {
  url: string;
  role: string;
  /** Connection state string from the kernel, e.g. "Connected", "Connecting". */
  status: string;
  auth: string | null;
  negentropyProbe: string | null;
  activeWireSubs: bigint;
  reconnectCount: number;
  eventsRx: bigint;
  bytesRx: bigint;
  bytesTx: bigint;
  denied: boolean;
  lastError: string | null;
  lastCloseReason: string | null;
  /** Pre-computed connection tone from KRDG (`"ok"/"warn"/"error"/"muted"`). */
  connectionTone?: string;
  /** Pre-computed auth tone from KRDG (`"ok"/"warn"/"muted"`). */
  authTone?: string;
  /** Pre-computed role tone from KRDG (`"primary"/"write"/"accent"/"secondary"`). */
  roleTone?: string;
};

/** Key metrics decoded from the Tier-3 `metrics` field. */
export type DecodedMetrics = {
  storedEvents: bigint;
  visibleItems: bigint;
  eventsRx: bigint;
  eoseRx: bigint;
  bytesRx: bigint;
  bytesTx: bigint;
  updateSequence: bigint;
  estimatedStoreBytes: bigint;
  actorQueueDepth: number;
  openViews: number;
  makeUpdateUs: bigint;
  claimDropsTotal: bigint;
};

/** A logical interest decoded from the Tier-3 `logical_interests` vector. */
export type DecodedLogicalInterest = {
  key: string;
  state: string;
  refcount: number;
  relayUrls: string[];
  cacheCoverage: string | null;
  /** Pre-computed interest state tone from KRDG (`"ok"/"warn"/"muted"`). */
  stateTone?: string;
};

/** A wire subscription decoded from the Tier-3 `wire_subscriptions` vector. */
export type DecodedWireSub = {
  wireId: string;
  relayUrl: string;
  filterSummary: string;
  state: string;
  logicalConsumerCount: number;
  eventsRx: bigint;
  /** Pre-computed wire sub state tone from KRDG (`"ok"/"warn"/"muted"/"error"`). */
  stateTone?: string;
};

export type DecodedUpdateFrame =
  | {
      type: "snapshot";
      schemaVersion: number;
      rev: bigint;
      lastTickMs: bigint;
      /** True when the kernel's run state is Running. */
      running: boolean;
      /** Per-relay status rows decoded from the Tier-3 `relay_statuses` field. */
      relayStatuses: DecodedRelayStatus[];
      /** Key metrics from the Tier-3 `metrics` field; null if absent in frame. */
      metrics: DecodedMetrics | null;
      /** Logical interests (always non-empty once the kernel has configured relays). */
      logicalInterests: DecodedLogicalInterest[];
      /** Active wire subscriptions; empty until the kernel opens its first REQ. */
      wireSubscriptions: DecodedWireSub[];
      storeOpenFailure: string | null;
      lastErrorToast: string | null;
      lastErrorCategory: string | null;
    }
  | { type: "panic"; message: string };

function decodeRelayStatus(relay: RelayStatus): DecodedRelayStatus {
  return {
    url: relay.relayUrl() ?? "",
    role: relay.role() ?? "",
    status: relay.connection() ?? "",
    auth: relay.auth(),
    negentropyProbe: relay.negentropyProbe(),
    activeWireSubs: relay.activeWireSubscriptions(),
    reconnectCount: relay.reconnectCount(),
    eventsRx: relay.eventsRx(),
    bytesRx: relay.bytesRx(),
    bytesTx: relay.bytesTx(),
    denied: relay.denied(),
    lastError: relay.lastError(),
    lastCloseReason: relay.lastCloseReason(),
  };
}

function decodeMetrics(m: Metrics): DecodedMetrics {
  return {
    storedEvents: m.storedEvents(),
    visibleItems: m.visibleItems(),
    eventsRx: m.eventsRx(),
    eoseRx: m.eoseRx(),
    bytesRx: m.bytesRx(),
    bytesTx: m.bytesTx(),
    updateSequence: m.updateSequence(),
    estimatedStoreBytes: m.estimatedStoreBytes(),
    actorQueueDepth: m.actorQueueDepth(),
    openViews: m.openViews(),
    makeUpdateUs: m.makeUpdateUs(),
    claimDropsTotal: m.claimDropsTotal(),
  };
}

function decodeLogicalInterest(li: LogicalInterestStatus): DecodedLogicalInterest {
  const relayUrls: string[] = [];
  for (let i = 0; i < li.relayUrlsLength(); i += 1) {
    const u = li.relayUrls(i);
    if (u) relayUrls.push(u);
  }
  return {
    key: li.key() ?? "",
    state: li.state() ?? "",
    refcount: li.refcount(),
    relayUrls,
    cacheCoverage: li.cacheCoverage(),
  };
}

function decodeWireSub(ws: WireSubscriptionStatus): DecodedWireSub {
  return {
    wireId: ws.wireId() ?? "",
    relayUrl: ws.relayUrl() ?? "",
    filterSummary: ws.filterSummary() ?? "",
    state: ws.state() ?? "",
    logicalConsumerCount: ws.logicalConsumerCount(),
    eventsRx: ws.eventsRx(),
  };
}

/**
 * Decode a raw FlatBuffers update-frame buffer.
 *
 * @param options.lite — When true, skip the expensive Tier-3 arrays
 *   (`logicalInterests`, `wireSubscriptions`) and `metrics`. Returns empty
 *   arrays / null for those fields. Use on the hot subscribe path to keep
 *   the main thread lean; use without the flag when the Inspector is open
 *   and needs the full data.
 */
export function decodeUpdateFrameBytes(
  bytes: Uint8Array,
  options?: { lite?: boolean },
): DecodedUpdateFrame {
  if (bytes.length === 0) {
    throw new UpdateFrameDecodeError("invalid_flatbuffer", "empty update frame buffer");
  }
  const buffer = new flatbuffers.ByteBuffer(bytes);
  if (!UpdateFrame.bufferHasIdentifier(buffer)) {
    throw new UpdateFrameDecodeError(
      "invalid_flatbuffer",
      "missing NMPU file identifier",
    );
  }
  const frame = UpdateFrame.getRootAsUpdateFrame(buffer);
  switch (frame.kind()) {
    case FrameKind.Snapshot: {
      const snapshot = frame.snapshot();
      if (!snapshot) {
        throw new UpdateFrameDecodeError(
          "invalid_flatbuffer",
          "snapshot frame table is absent",
        );
      }

      const relayStatuses: DecodedRelayStatus[] = [];
      for (let i = 0; i < snapshot.relayStatusesLength(); i += 1) {
        const relay = snapshot.relayStatuses(i, new RelayStatus());
        if (relay) relayStatuses.push(decodeRelayStatus(relay));
      }

      // logicalInterests and wireSubscriptions are skipped in lite mode — these
      // loops can be large during the initial sync storm and are only needed by
      // the Inspector's Subs panel (rendered only when the dock is expanded).
      const logicalInterests: DecodedLogicalInterest[] = [];
      if (!options?.lite) {
        for (let i = 0; i < snapshot.logicalInterestsLength(); i += 1) {
          const li = snapshot.logicalInterests(i, new LogicalInterestStatus());
          if (li) logicalInterests.push(decodeLogicalInterest(li));
        }
      }

      const wireSubscriptions: DecodedWireSub[] = [];
      if (!options?.lite) {
        for (let i = 0; i < snapshot.wireSubscriptionsLength(); i += 1) {
          const ws = snapshot.wireSubscriptions(i, new WireSubscriptionStatus());
          if (ws) wireSubscriptions.push(decodeWireSub(ws));
        }
      }

      // metrics is also skipped in lite mode — only needed by PanelOverview.
      const metricsObj = options?.lite ? null : snapshot.metrics(new Metrics());
      const metrics = metricsObj ? decodeMetrics(metricsObj) : null;

      return {
        type: "snapshot",
        schemaVersion: snapshot.schemaVersion(),
        rev: snapshot.rev(),
        lastTickMs: snapshot.lastTickMs(),
        running: snapshot.running(),
        relayStatuses,
        metrics,
        logicalInterests,
        wireSubscriptions,
        storeOpenFailure: snapshot.storeOpenFailure(),
        lastErrorToast: snapshot.lastErrorToast(),
        lastErrorCategory: snapshot.lastErrorCategory(),
      };
    }
    case FrameKind.Panic: {
      const panic = frame.panic();
      if (!panic) {
        throw new UpdateFrameDecodeError(
          "missing_panic_payload",
          "panic frame missing payload",
        );
      }
      const message = panic.msg();
      if (message === null) {
        throw new UpdateFrameDecodeError(
          "missing_panic_payload",
          "panic frame missing msg",
        );
      }
      return { type: "panic", message };
    }
    default:
      throw new UpdateFrameDecodeError(
        "invalid_flatbuffer",
        `unknown frame kind ${frame.kind()}`,
      );
  }
}
