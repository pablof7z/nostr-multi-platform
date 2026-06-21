import * as flatbuffers from "flatbuffers";

import { decodeKrdgTones } from "./relayDiagnosticsProjection";
import { FrameKind, UpdateFrame } from "@nmp/wire-ts/nmp/transport";
import {
  decodeUpdateFrameBytes,
  SNAPSHOT_SCHEMA_VERSION,
  type DecodedLogicalInterest,
  type DecodedMetrics,
  type DecodedWireSub,
} from "./updateFrame";

/** Full Tier-3 snapshot data decoded from the most recent FlatBuffers frame.
 *  This type is NOT populated on every frame — it is decoded lazily by the
 *  Inspector component when the dock is opened (`decodeInspectorSnapshot`).
 *  KRDG tone fields are merged in during that lazy decode. */
export type DecodedSnapshot = {
  rev: bigint;
  lastTickMs: bigint;
  metrics: DecodedMetrics | null;
  logicalInterests: DecodedLogicalInterest[];
  wireSubscriptions: DecodedWireSub[];
  storeOpenFailure: string | null;
  lastErrorToast: string | null;
  lastErrorCategory: string | null;
};

/**
 * Decode the full Tier-3 Inspector snapshot from raw bytes, including:
 * - logicalInterests and wireSubscriptions (skipped on the hot path)
 * - metrics
 * - KRDG tone enrichment for interests and wire subs
 *
 * Called lazily by the Inspector component when the dock is opened — never
 * on the hot subscribe path. Returns `undefined` on any decode failure so
 * the Inspector can gracefully show stale-or-empty state.
 */
export function decodeInspectorSnapshot(bytes: Uint8Array): DecodedSnapshot | undefined {
  try {
    // Full decode (no lite) to get logicalInterests, wireSubscriptions, metrics.
    const decoded = decodeUpdateFrameBytes(bytes);
    if (decoded.type !== "snapshot" || decoded.schemaVersion !== SNAPSHOT_SCHEMA_VERSION) {
      return undefined;
    }

    let { logicalInterests, wireSubscriptions } = decoded;

    // Second pass for KRDG tone enrichment (full detail — wireSubTones +
    // interestTones needed by PanelSubs).
    try {
      const bb = new flatbuffers.ByteBuffer(bytes);
      if (UpdateFrame.bufferHasIdentifier(bb)) {
        const frame = UpdateFrame.getRootAsUpdateFrame(bb);
        if (frame.kind() === FrameKind.Snapshot) {
          const snap = frame.snapshot();
          if (snap) {
            const krdgTones = decodeKrdgTones(snap); // full detail
            if (krdgTones !== undefined) {
              logicalInterests = logicalInterests.map((li) => {
                const tone = krdgTones.interestTones.get(li.key);
                return tone !== undefined ? { ...li, stateTone: tone } : li;
              });
              wireSubscriptions = wireSubscriptions.map((ws) => {
                const relayEntry = krdgTones.relayTones.get(ws.relayUrl);
                if (!relayEntry) return ws;
                const tone = relayEntry.wireSubTones.get(ws.wireId);
                return tone !== undefined ? { ...ws, stateTone: tone } : ws;
              });
            }
          }
        }
      }
    } catch {
      // Corrupt KRDG: return snapshot without tone enrichment.
    }

    return {
      rev: decoded.rev,
      lastTickMs: decoded.lastTickMs,
      metrics: decoded.metrics,
      logicalInterests,
      wireSubscriptions,
      storeOpenFailure: decoded.storeOpenFailure,
      lastErrorToast: decoded.lastErrorToast,
      lastErrorCategory: decoded.lastErrorCategory,
    };
  } catch {
    return undefined;
  }
}
