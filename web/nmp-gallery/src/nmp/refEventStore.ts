// ADR-0063 Lane F — host-side `refs.event` consumption helper (TypeScript).
//
// The kernel emits event refs as per-KEY row deltas under `refs.event`. Each
// row payload is a single-entry KCEV `ClaimedEventsSnapshot`; the whole-map
// `claimed_events` projection is not the runtime source.

import * as flatbuffers from "flatbuffers";

import { ContentTreeWire } from "./generated/nmp/content/content-tree-wire";
import { ClaimedEventsSnapshot } from "./generated/nmp/kernel/claimed-events-snapshot";
import { RefRowCache, type RefRowApplyOutcome } from "./refRowCache";

export const REFS_EVENT_KEY = "refs.event";
const REFS_EVENT_NAMESPACE = "event";

export type ClaimedEventWire = {
  primaryId: string;
  id: string;
  kind: number;
  content: string;
  createdAt: number;
  authorPubkey: string;
  tags: string[][];
  contentTree?: ContentTreeWire;
};

export function tagValue(ev: ClaimedEventWire, name: string): string | undefined {
  const row = ev.tags.find((t) => t[0] === name);
  return row && row.length > 1 ? row[1] : undefined;
}

function decodeEventRow(key: string, bytes: Uint8Array): ClaimedEventWire | undefined {
  if (bytes.length < 8) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!ClaimedEventsSnapshot.bufferHasIdentifier(bb)) return undefined;
    const root = ClaimedEventsSnapshot.getRootAsClaimedEventsSnapshot(bb);
    if (root.entriesLength() !== 1) return undefined;
    const entry = root.entries(0);
    if (!entry || entry.key() !== key) return undefined;
    const ev = entry.value();
    if (!ev || ev.primaryId() !== key) return undefined;

    const tags: string[][] = [];
    for (let t = 0; t < ev.tagsLength(); t += 1) {
      const row = ev.tags(t);
      if (!row) continue;
      const values: string[] = [];
      for (let v = 0; v < row.valuesLength(); v += 1) {
        values.push((row.values(v) as string) ?? "");
      }
      tags.push(values);
    }

    const wire: ClaimedEventWire = {
      primaryId: key,
      id: ev.id() ?? "",
      kind: ev.kind(),
      content: ev.content() ?? "",
      createdAt: Number(ev.createdAt()),
      authorPubkey: ev.authorPubkey() ?? "",
      tags,
    };
    const ctBytes = ev.contentTreeBytesArray();
    if (ctBytes && ctBytes.length > 0) {
      try {
        const ctBb = new flatbuffers.ByteBuffer(ctBytes);
        if (ContentTreeWire.bufferHasIdentifier(ctBb)) {
          wire.contentTree = ContentTreeWire.getRootAsContentTreeWire(ctBb);
        }
      } catch {
        // Corrupt NFCT bytes leave contentTree absent; callers can render raw text.
      }
    }
    return wire;
  } catch {
    return undefined;
  }
}

export class RefEventStore {
  private cache = new RefRowCache();

  applySidecar(payload: Uint8Array, sessionId: bigint, snapshotEpoch: bigint): RefRowApplyOutcome {
    return this.cache.applySidecar(
      payload,
      sessionId,
      snapshotEpoch,
      (key, bytes) => decodeEventRow(key, bytes) !== undefined,
    );
  }

  event(primaryId: string): ClaimedEventWire | undefined {
    const payload = this.cache.get(REFS_EVENT_NAMESPACE, primaryId);
    if (!payload) return undefined;
    return decodeEventRow(primaryId, payload);
  }

  events(): Map<string, ClaimedEventWire> {
    const out = new Map<string, ClaimedEventWire>();
    for (const [key, payload] of this.cache.snapshot(REFS_EVENT_NAMESPACE)) {
      const wire = decodeEventRow(key, payload);
      if (wire) out.set(key, wire);
    }
    return out;
  }
}
