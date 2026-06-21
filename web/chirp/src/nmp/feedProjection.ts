import * as flatbuffers from "flatbuffers";

import { OpFeedSnapshot, RelationCount, ReplyAttribution, RootCard } from "./generated/nmp/nip01";
import { RelationCountState } from "./generated/nmp/nip01/relation-count-state";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";
import { ContentTreeWire } from "./generated/nmp/content/content-tree-wire";
import { REFS_PROFILE_KEY } from "./refProfileStore";

// ── Schema descriptor constants (ADR-0038) ──────────────────────────────────

const NOFS_SCHEMA_ID = "nmp.nip01.opfeed";
const NOFS_SCHEMA_VERSION = 1;
const NOFS_FILE_IDENTIFIER = "NOFS";
const NOFS_PROJECTION_KEY = "nmp.feed.home";

// `refs.profile` typed projection (NRRD row-delta carrier) — the kernel-owned
// per-key reference-resolution projection that replaced the whole-map
// `resolved_profiles` (KRPR). The opaque payload is an NRRD batch the stateful
// `RefProfileStore`/`RefRowCache` merges; this module only extracts the raw
// sidecar bytes for the client to feed into the cache.
const NRRD_FILE_IDENTIFIER = "NRRD";

// ── Public types ─────────────────────────────────────────────────────────────

/** ADR-0035 attribution badge — one follow who replied in the thread. */
export type FeedAttribution = {
  /** Raw hex pubkey of the replying follow. */
  authorPubkey: string;
  /** Kind:0 display name, absent until a kind:0 arrives. */
  authorDisplayName?: string;
  /** Kind:0 profile picture URL, absent until a kind:0 arrives. */
  authorPictureUrl?: string;
  /** Raw hex event id of the reply. */
  replyEventId: string;
  /** Unix seconds created_at of the reply. */
  replyCreatedAt: number;
};

/** NIP-18 repost attribution — the follow who surfaced this note. */
export type FeedRepostBy = {
  /** Raw hex pubkey of the reposter. */
  authorPubkey: string;
  /** Kind:0 display name, absent until a kind:0 arrives. */
  authorDisplayName?: string;
};

/** Decoded relation count — either a known value or a loading indicator. */
export type FeedCountState =
  | { type: "known"; count: number }
  | { type: "loading" };

/** Reaction/reply/repost/zap counts for one feed item. */
export type FeedRelationCounts = {
  replies: FeedCountState;
  reactions: FeedCountState;
  reposts: FeedCountState;
  zaps: FeedCountState;
};

/**
 * One decoded feed row — a thread root card.
 *
 * Field set implements §2 steps 4–5 of feed-render-and-ui-plan.md (PR-F2).
 * Sub-buffers (NFCT content tree, NFWM window) are deferred to PR-F4 and
 * a gap PR respectively; the raw `content` string suffices for F3 rendering.
 */
export type FeedItem = {
  /** Raw hex event id. */
  id: string;
  /** Raw hex pubkey of the event author. */
  authorPubkey: string;
  /** Kind:0 display name, absent until a kind:0 arrives. */
  authorDisplayName?: string;
  /** Kind:0 profile picture URL, absent until a kind:0 arrives. */
  authorPictureUrl?: string;
  /** Raw NIP-01 content string (fallback when no NFCT tree is present). */
  content: string;
  /** Decoded NFCT content tree — present when the kernel ships
   *  `content_tree_bytes` in the timeline card. Rendered by NostrContentView;
   *  falls back to `content` when absent. */
  contentTree?: ContentTreeWire;
  /** Unix seconds created_at. */
  createdAt: number;
  /** Raw relay URLs that delivered this event, in store provenance order. */
  relayProvenance: string[];
  /** Known-or-loading relation counts. */
  relationCounts: FeedRelationCounts;
  /** NIP-18 repost attribution — who surfaced this note. */
  repostedBy?: FeedRepostBy;
  /**
   * ADR-0035 reply-attribution badges. Length IS the count (op_feed.fbs:62-63);
   * bounded by the engine's D5 per-root cap at encode time.
   */
  attribution: FeedAttribution[];
};

// ── Internal helpers ─────────────────────────────────────────────────────────

function decodeCountState(rc: RelationCount | null): FeedCountState {
  if (!rc || rc.state() === RelationCountState.Loading) {
    return { type: "loading" };
  }
  return { type: "known", count: Number(rc.count()) };
}

function decodeAttribution(ra: ReplyAttribution): FeedAttribution {
  const out: FeedAttribution = {
    authorPubkey: ra.authorPubkey() ?? "",
    replyEventId: ra.replyEventId() ?? "",
    replyCreatedAt: Number(ra.replyCreatedAt()),
  };
  // ADR-0032 / #1493: flat mirrors removed from ReplyAttribution; read from
  // nested authorDisplay table.
  const display = ra.authorDisplay();
  if (display?.hasName()) {
    // Guard against empty string: `?? ""` would set authorDisplayName="" which
    // blocks the refs.profile display-name fallback in feedItemsToRows because
    // `"" ?? resolvedProfiles.get(pubkey)` returns "" (nullish coalescing only
    // bypasses null/undefined, not empty string).  Leave undefined so the
    // presentation-layer join can supply the name from the refs.profile cache.
    const dn = display.name();
    if (dn) out.authorDisplayName = dn;
  }
  if (display?.hasPictureUrl()) {
    const url = display.pictureUrl();
    if (url) out.authorPictureUrl = url;
  }
  return out;
}

function decodeRootCard(rootCard: RootCard): FeedItem | null {
  const card = rootCard.card();
  if (!card) {
    return null;
  }

  const rc = card.relationCounts();
  const relationCounts: FeedRelationCounts = {
    replies: decodeCountState(rc?.replies() ?? null),
    reactions: decodeCountState(rc?.reactions() ?? null),
    reposts: decodeCountState(rc?.reposts() ?? null),
    zaps: decodeCountState(rc?.zaps() ?? null),
  };

  const repostedByFb = card.repostedBy();
  let repostedBy: FeedRepostBy | undefined;
  if (repostedByFb) {
    repostedBy = { authorPubkey: repostedByFb.authorPubkey() ?? "" };
    if (repostedByFb.hasAuthorDisplayName()) {
      const dn = repostedByFb.authorDisplayName();
      if (dn) repostedBy.authorDisplayName = dn;
    }
  }

  const attribution: FeedAttribution[] = [];
  const attrLen = rootCard.attributionLength();
  for (let j = 0; j < attrLen; j += 1) {
    const ra = rootCard.attribution(j);
    if (ra) {
      attribution.push(decodeAttribution(ra));
    }
  }

  const item: FeedItem = {
    id: card.id() ?? "",
    authorPubkey: card.authorPubkey() ?? "",
    content: card.content() ?? "",
    createdAt: Number(card.createdAt()),
    relayProvenance: [],
    relationCounts,
    attribution,
  };
  for (let j = 0; j < card.relayProvenanceLength(); j += 1) {
    const relay = card.relayProvenance(j);
    if (relay) {
      item.relayProvenance.push(relay);
    }
  }
  if (card.hasAuthorDisplayName()) {
    const dn = card.authorDisplayName();
    if (dn) item.authorDisplayName = dn;
  }
  if (card.hasAuthorPictureUrl()) {
    const url = card.authorPictureUrl();
    if (url) item.authorPictureUrl = url;
  }
  if (repostedBy) {
    item.repostedBy = repostedBy;
  }
  // Decode the NFCT content tree when the card ships `content_tree_bytes`.
  // Keep-last-good (honest-empty) on missing bytes / bad identifier / decode
  // error — the caller falls back to the raw `content` string.
  const ctBytes = card.contentTreeBytesArray();
  if (ctBytes && ctBytes.length > 0) {
    try {
      const ctBb = new flatbuffers.ByteBuffer(ctBytes);
      if (ContentTreeWire.bufferHasIdentifier(ctBb)) {
        item.contentTree = ContentTreeWire.getRootAsContentTreeWire(ctBb);
      }
    } catch {
      // Corrupt NFCT bytes — leave contentTree undefined.
    }
  }
  return item;
}

// ── Public decode API ────────────────────────────────────────────────────────

/**
 * Decode a bare NOFS `OpFeedSnapshot` buffer into a feed-item list.
 *
 * The golden fixture `op_feed_populated_v1.fb.hex` is exactly this format —
 * it is the bytes that sit in `TypedPayload.payload` at runtime. Returns
 * `undefined` when the buffer lacks the NOFS file identifier or is otherwise
 * undecodable (honest degradation — caller should keep last-good rows).
 */
export function decodeOpFeedSnapshot(bytes: Uint8Array): { items: FeedItem[] } | undefined {
  if (bytes.length === 0) {
    return undefined;
  }
  const bb = new flatbuffers.ByteBuffer(bytes);
  if (!OpFeedSnapshot.bufferHasIdentifier(bb)) {
    return undefined;
  }
  const snapshot = OpFeedSnapshot.getRootAsOpFeedSnapshot(bb);
  const items: FeedItem[] = [];
  for (let i = 0; i < snapshot.cardsLength(); i += 1) {
    const rootCard = snapshot.cards(i);
    if (rootCard) {
      const item = decodeRootCard(rootCard);
      if (item) {
        items.push(item);
      }
    }
  }
  return { items };
}

/**
 * Find the `nmp.feed.home` typed projection in a decoded `SnapshotFrame`,
 * validate its NOFS descriptor, and decode it to a feed-item list.
 *
 * Returns `undefined` when the projection is absent (kernel feed composition
 * not yet wired — expected until PR-F1 lands), when the descriptor mismatches
 * (`schema_id`, `schema_version`, or `file_identifier` differ from the pinned
 * values), or when the inner NOFS buffer is corrupt. Callers should keep the
 * last-good feed rows — this mirrors the `schema_version_mismatch` degradation
 * in `updateFrame.ts`.
 */
export function decodeHomeFeed(snapshot: SnapshotFrame): { items: FeedItem[] } | undefined {
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    if (!proj || proj.key() !== NOFS_PROJECTION_KEY) {
      continue;
    }
    const payload = proj.payload();
    if (!payload) {
      return undefined;
    }
    if (
      payload.schemaId() !== NOFS_SCHEMA_ID ||
      payload.schemaVersion() !== NOFS_SCHEMA_VERSION ||
      payload.fileIdentifier() !== NOFS_FILE_IDENTIFIER
    ) {
      return undefined;
    }
    const payloadBytes = payload.payloadArray();
    if (!payloadBytes) {
      return undefined;
    }
    return decodeOpFeedSnapshot(payloadBytes);
  }
  return undefined;
}

/**
 * Extract the raw `refs.profile` (NRRD) sidecar payload bytes from a
 * `SnapshotFrame`, or `undefined` when the projection is absent / empty / carries
 * the wrong file identifier.
 *
 * The `refs.profile` projection is a per-KEY row-delta carrier: its payload is an
 * NRRD `RefRowDeltaBatch` that MUST be merged into the stateful `RefProfileStore`
 * (`RefRowCache`), not decoded in isolation. This function only returns the bytes;
 * the client feeds them to `RefProfileStore.applySidecar(payload, sessionId,
 * snapshotEpoch)`. A frame with NO `refs.profile` entry returns `undefined` and
 * the caller leaves the persistent cache untouched (keep-last-good).
 */
export function findRefsProfileSidecar(snapshot: SnapshotFrame): Uint8Array | undefined {
  for (let i = 0; i < snapshot.typedProjectionsLength(); i += 1) {
    const proj = snapshot.typedProjections(i);
    if (!proj || proj.key() !== REFS_PROFILE_KEY) continue;
    const payload = proj.payload();
    if (!payload || payload.fileIdentifier() !== NRRD_FILE_IDENTIFIER) return undefined;
    const payloadBytes = payload.payloadArray();
    if (!payloadBytes || payloadBytes.length === 0) return undefined;
    // payloadArray() is a view over the frame buffer; the cache copies the rows
    // it commits, so passing the view through is safe (no retained reference).
    return payloadBytes;
  }
  return undefined;
}
