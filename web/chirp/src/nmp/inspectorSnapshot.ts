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
 *  Carries raw protocol tokens only; the inspector panels derive their own
 *  hue from them (#1768). */
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
 *
 * Carries raw protocol tokens only; the inspector panels derive their own hue
 * from them (#1768). Called lazily by the Inspector component when the dock is
 * opened — never on the hot subscribe path. Returns `undefined` on any decode
 * failure so the Inspector can gracefully show stale-or-empty state.
 */
export function decodeInspectorSnapshot(bytes: Uint8Array): DecodedSnapshot | undefined {
  try {
    // Full decode (no lite) to get logicalInterests, wireSubscriptions, metrics.
    const decoded = decodeUpdateFrameBytes(bytes);
    if (decoded.type !== "snapshot" || decoded.schemaVersion !== SNAPSHOT_SCHEMA_VERSION) {
      return undefined;
    }

    return {
      rev: decoded.rev,
      lastTickMs: decoded.lastTickMs,
      metrics: decoded.metrics,
      logicalInterests: decoded.logicalInterests,
      wireSubscriptions: decoded.wireSubscriptions,
      storeOpenFailure: decoded.storeOpenFailure,
      lastErrorToast: decoded.lastErrorToast,
      lastErrorCategory: decoded.lastErrorCategory,
    };
  } catch {
    return undefined;
  }
}
