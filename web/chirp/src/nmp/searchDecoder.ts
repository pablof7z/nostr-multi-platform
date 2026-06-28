import * as flatbuffers from "flatbuffers";
import { FrameKind } from "./generated/nmp/transport/frame-kind";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";
import { SearchResultsSnapshot } from "./generated/nmp/nip50/search-results-snapshot";
import type { SearchHit } from "./generated/nmp/nip50/search-hit";

const SEARCH_FILE_ID = "N50S";

export type SearchResultRow = {
  id: string;
  authorPubkey: string;
  kind: number;
  createdAt: number;
  content: string;
  tags: string[][];
  relayProvenance: string[];
  source: "cache" | "relay";
  sourceRelay?: string;
};

export type SearchResultsFrame = {
  rows: SearchResultRow[];
};

export function decodeSearchResultsFrame(
  bytes: Uint8Array | undefined,
  sessionId: string,
): SearchResultsFrame | undefined {
  if (!bytes || !sessionId) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!UpdateFrame.bufferHasIdentifier(bb)) return undefined;
    const frame = UpdateFrame.getRootAsUpdateFrame(bb);
    if (frame.kind() !== FrameKind.Snapshot) return undefined;
    const snap = frame.snapshot();
    if (!snap) return undefined;
    const key = `nmp.nip50.search.${sessionId}`;
    for (let i = 0; i < snap.typedProjectionsLength(); i++) {
      const projection = snap.typedProjections(i);
      const payload = projection?.payload();
      const payloadBytes = payload?.payloadArray();
      if (
        projection?.key() !== key ||
        payload?.fileIdentifier() !== SEARCH_FILE_ID ||
        !payloadBytes ||
        payloadBytes.length === 0
      ) {
        continue;
      }
      return { rows: decodePayload(payloadBytes) };
    }
    return undefined;
  } catch {
    return undefined;
  }
}

function decodePayload(bytes: Uint8Array): SearchResultRow[] {
  const bb = new flatbuffers.ByteBuffer(bytes);
  if (!SearchResultsSnapshot.bufferHasIdentifier(bb)) return [];
  const snapshot = SearchResultsSnapshot.getRootAsSearchResultsSnapshot(bb);
  const rows: SearchResultRow[] = [];
  for (let i = 0; i < snapshot.hitsLength(); i++) {
    const hit = snapshot.hits(i);
    const row = hit ? decodeHit(hit) : undefined;
    if (row) rows.push(row);
  }
  return rows;
}

function decodeHit(hit: SearchHit): SearchResultRow | undefined {
  const id = hit.id();
  const authorPubkey = hit.author();
  if (!id || !authorPubkey) return undefined;
  const sourceRelay = hit.sourceRelay() || undefined;
  return {
    id,
    authorPubkey,
    kind: hit.kind(),
    createdAt: numberFromBigint(hit.createdAt()),
    content: hit.content() ?? "",
    tags: decodeTags(hit),
    relayProvenance: decodeRelayProvenance(hit),
    source: hit.isCache() ? "cache" : "relay",
    sourceRelay,
  };
}

function decodeTags(hit: SearchHit): string[][] {
  const rows: string[][] = [];
  for (let i = 0; i < hit.tagsLength(); i++) {
    const tag = hit.tags(i);
    if (!tag) continue;
    const cells: string[] = [];
    for (let j = 0; j < tag.cellsLength(); j++) {
      const cell = tag.cells(j);
      if (typeof cell === "string") cells.push(cell);
    }
    rows.push(cells);
  }
  return rows;
}

function decodeRelayProvenance(hit: SearchHit): string[] {
  const relays: string[] = [];
  for (let i = 0; i < hit.relayProvenanceLength(); i++) {
    const relay = hit.relayProvenance(i);
    if (typeof relay === "string") relays.push(relay);
  }
  return relays;
}

function numberFromBigint(value: bigint): number {
  const max = BigInt(Number.MAX_SAFE_INTEGER);
  return Number(value > max ? max : value);
}
