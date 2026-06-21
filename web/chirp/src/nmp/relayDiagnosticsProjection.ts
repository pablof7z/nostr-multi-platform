import * as flatbuffers from "flatbuffers";

import { RelayDiagnosticsSnapshot } from "@nmp/wire-ts/nmp/kernel/relay-diagnostics-snapshot.js";
import type { SnapshotFrame } from "@nmp/wire-ts/nmp/transport/snapshot-frame.js";

// ── Schema descriptor constants (ADR-0038) ──────────────────────────────────

const KRDG_FILE_IDENTIFIER = "KRDG";
const KRDG_PROJECTION_KEY = "relay_diagnostics";

// ── Public types ─────────────────────────────────────────────────────────────

/** Pre-computed tones for one relay, decoded from the KRDG typed projection. */
export type KrdgRelayTones = {
  connectionTone: string;
  authTone: string;
  roleTone: string;
  /** Maps wire_id → state_tone for wire subs owned by this relay. */
  wireSubTones: Map<string, string>;
};

/** Decoded tone maps from the KRDG (`relay_diagnostics`) typed projection. */
export type KrdgTones = {
  /** Maps relay_url → per-relay tone fields. */
  relayTones: Map<string, KrdgRelayTones>;
  /** Maps logical-interest key → state_tone. */
  interestTones: Map<string, string>;
};

// ── Decoder ──────────────────────────────────────────────────────────────────

/**
 * Find the `relay_diagnostics` KRDG typed projection in a `SnapshotFrame`,
 * validate the KRDG file identifier, and decode tone fields for every relay,
 * wire subscription, and logical interest.
 *
 * Returns `undefined` when the projection is absent or the buffer is corrupt
 * (callers should keep the last-good decoded tone state — keep-last-good).
 * Never throws (D6: error path returns `undefined`).
 *
 * @param options.skipDetails — When true, skip the per-relay `wireSubTones`
 *   and the `interestTones` maps. Only the per-relay scalar tones
 *   (`connectionTone`, `authTone`, `roleTone`) are decoded. Use on the hot
 *   subscribe path where only the pulse-strip connection dots need tone data;
 *   omit (or pass false) when the Inspector is open and panels need full data.
 */
export function decodeKrdgTones(
  snapshot: SnapshotFrame,
  options?: { skipDetails?: boolean },
): KrdgTones | undefined {
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    if (!proj || proj.key() !== KRDG_PROJECTION_KEY) {
      continue;
    }
    const payload = proj.payload();
    if (!payload || payload.fileIdentifier() !== KRDG_FILE_IDENTIFIER) {
      return undefined;
    }
    const payloadBytes = payload.payloadArray();
    if (!payloadBytes || payloadBytes.length === 0) {
      return undefined;
    }
    try {
      const bb = new flatbuffers.ByteBuffer(payloadBytes);
      if (!RelayDiagnosticsSnapshot.bufferHasIdentifier(bb)) {
        return undefined;
      }
      const root = RelayDiagnosticsSnapshot.getRootAsRelayDiagnosticsSnapshot(bb);

      const relayTones = new Map<string, KrdgRelayTones>();
      for (let j = 0; j < root.relaysLength(); j += 1) {
        const row = root.relays(j);
        if (!row) continue;
        const url = row.relayUrl();
        if (!url) continue;
        // wireSubTones is skipped in skipDetails mode — only needed by
        // PanelSubs/PanelRelays when the Inspector dock is expanded.
        const wireSubTones = new Map<string, string>();
        if (!options?.skipDetails) {
          for (let k = 0; k < row.wireSubsLength(); k += 1) {
            const ws = row.wireSubs(k);
            if (!ws) continue;
            const wireId = ws.wireId();
            if (!wireId) continue;
            wireSubTones.set(wireId, ws.stateTone() ?? "muted");
          }
        }
        relayTones.set(url, {
          connectionTone: row.connectionTone() ?? "muted",
          authTone: row.authTone() ?? "muted",
          roleTone: row.roleTone() ?? "muted",
          wireSubTones,
        });
      }

      // interestTones is skipped in skipDetails mode — only needed by PanelSubs.
      const interestTones = new Map<string, string>();
      if (!options?.skipDetails) {
        for (let j = 0; j < root.interestsLength(); j += 1) {
          const interest = root.interests(j);
          if (!interest) continue;
          const key = interest.key();
          if (!key) continue;
          interestTones.set(key, interest.stateTone() ?? "muted");
        }
      }

      return { relayTones, interestTones };
    } catch {
      return undefined;
    }
  }
  return undefined;
}
