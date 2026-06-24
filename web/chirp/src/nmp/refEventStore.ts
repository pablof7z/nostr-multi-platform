import * as flatbuffers from "flatbuffers";

import type { ContentTreeWire } from "./generated/nmp/content/content-tree-wire";
import { ContentTreeWire as ContentTreeWireFb } from "./generated/nmp/content/content-tree-wire";
import { ClaimedEventsSnapshot } from "./generated/nmp/kernel/claimed-events-snapshot";
import { RefRowCache, type RefRowApplyOutcome } from "./refRowCache";

export const REFS_EVENT_KEY = "refs.event";
const REFS_EVENT_NAMESPACE = "event";

export type ClaimedEventWire = {
  primaryId: string;
  id: string;
  authorPubkey: string;
  authorDisplayName?: string;
  authorPictureUrl?: string;
  kind: number;
  createdAt: number;
  tags: string[][];
  content: string;
  contentTree?: ContentTreeWire;
  contentTreeBytes?: Uint8Array;
  signedEventJson?: string;
};

function decodeContentTree(bytes: Uint8Array | null): {
  tree?: ContentTreeWire;
  bytes?: Uint8Array;
} {
  if (!bytes || bytes.length === 0) return {};
  try {
    const copied = bytes.slice();
    const bb = new flatbuffers.ByteBuffer(copied);
    if (!ContentTreeWireFb.bufferHasIdentifier(bb)) return {};
    return { tree: ContentTreeWireFb.getRootAsContentTreeWire(bb), bytes: copied };
  } catch {
    return {};
  }
}

function decodeEventRow(key: string, payload: Uint8Array): ClaimedEventWire | undefined {
  if (payload.length < 8) return undefined;
  try {
    const bb = new flatbuffers.ByteBuffer(payload);
    if (!ClaimedEventsSnapshot.bufferHasIdentifier(bb)) return undefined;
    const snapshot = ClaimedEventsSnapshot.getRootAsClaimedEventsSnapshot(bb);
    if (snapshot.entriesLength() !== 1) return undefined;
    const entry = snapshot.entries(0);
    const value = entry?.value();
    if (!entry || !value || entry.key() !== key || value.primaryId() !== key) {
      return undefined;
    }
    const tags: string[][] = [];
    for (let i = 0; i < value.tagsLength(); i += 1) {
      const row = value.tags(i);
      if (!row) continue;
      const values: string[] = [];
      for (let j = 0; j < row.valuesLength(); j += 1) {
        const tagValue = row.values(j);
        if (tagValue) values.push(tagValue);
      }
      tags.push(values);
    }
    const contentTree = decodeContentTree(value.contentTreeBytesArray());
    const row: ClaimedEventWire = {
      primaryId: key,
      id: value.id() ?? "",
      authorPubkey: value.authorPubkey() ?? "",
      kind: value.kind(),
      createdAt: Number(value.createdAt()),
      tags,
      content: value.content() ?? "",
    };
    if (value.hasAuthorDisplayName()) {
      const display = value.authorDisplayName();
      if (display) row.authorDisplayName = display;
    }
    if (value.hasAuthorPictureUrl()) {
      const picture = value.authorPictureUrl();
      if (picture) row.authorPictureUrl = picture;
    }
    if (contentTree.tree) row.contentTree = contentTree.tree;
    if (contentTree.bytes) row.contentTreeBytes = contentTree.bytes;
    if (value.hasSignedEventJson()) {
      const signed = value.signedEventJson();
      if (signed) row.signedEventJson = signed;
    }
    return row;
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
    return payload ? decodeEventRow(primaryId, payload) : undefined;
  }

  events(): Map<string, ClaimedEventWire> {
    const out = new Map<string, ClaimedEventWire>();
    for (const [key, payload] of this.cache.snapshot(REFS_EVENT_NAMESPACE)) {
      const event = decodeEventRow(key, payload);
      if (event) out.set(key, event);
    }
    return out;
  }

  baselined(): boolean {
    return this.cache.baselined();
  }
}

export function claimedEventsEqual(
  a: Map<string, ClaimedEventWire> | undefined,
  b: Map<string, ClaimedEventWire>,
): boolean {
  if (!a || a.size !== b.size) return false;
  for (const [key, next] of b) {
    const prev = a.get(key);
    if (!prev || !claimedEventEqual(prev, next)) return false;
  }
  return true;
}

function claimedEventEqual(a: ClaimedEventWire, b: ClaimedEventWire): boolean {
  return (
    a.primaryId === b.primaryId &&
    a.id === b.id &&
    a.authorPubkey === b.authorPubkey &&
    a.authorDisplayName === b.authorDisplayName &&
    a.authorPictureUrl === b.authorPictureUrl &&
    a.kind === b.kind &&
    a.createdAt === b.createdAt &&
    a.content === b.content &&
    a.signedEventJson === b.signedEventJson &&
    tagsEqual(a.tags, b.tags) &&
    bytesEqual(a.contentTreeBytes, b.contentTreeBytes)
  );
}

function tagsEqual(a: string[][], b: string[][]): boolean {
  if (a.length !== b.length) return false;
  return a.every((row, i) => row.length === b[i]!.length && row.every((v, j) => v === b[i]![j]));
}

function bytesEqual(a?: Uint8Array, b?: Uint8Array): boolean {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  return a.every((value, i) => value === b[i]);
}
