// feedDecoder.ts — decode the `nmp.feed.home` typed projection from UpdateFrame bytes.
//
// Reuses the gallery's FlatBuffers generated decoders (OpFeedSnapshot, UpdateFrame,
// SnapshotFrame). Zero protocol logic — pure decode of bytes the Rust kernel emits.
// The projection key, file identifier, and FlatBuffers decoder are single-sourced from
// the generated TS classes; no hand-rolled decoding here.

import * as flatbuffers from "flatbuffers";
import { UpdateFrame } from "./generated/nmp/transport/update-frame";
import { FrameKind } from "./generated/nmp/transport/frame-kind";
import { OpFeedSnapshot } from "./generated/nmp/nip01/op-feed-snapshot";
import type { RelationCount } from "./generated/nmp/nip01/relation-count";
import type { SnapshotFrame } from "./generated/nmp/transport/snapshot-frame";

/** The typed projection key the home feed uses (mirrors OP_FEED_SNAPSHOT_KEY in Rust). */
export const HOME_FEED_PROJECTION_KEY = "nmp.feed.home";
const NOFS_FILE_IDENTIFIER = "NOFS";

/** Profile refs sidecar projection key (mirrors REFS_PROFILE_KEY). */
export const REFS_PROFILE_PROJECTION_KEY = "refs.profile";
const NRRD_FILE_IDENTIFIER = "NRRD";

/** A single renderable row extracted from the home feed OpFeedSnapshot. */
export type FeedRow = {
  /** Hex event id. */
  id: string;
  /** Nostr event kind. */
  kind: number;
  /** Author hex pubkey (64 chars). */
  authorPubkey: string;
  /** Unix timestamp (seconds). */
  createdAt: number;
  /** Raw note body. */
  content: string;
  /** Content preview (kernel-truncated). May differ from `content`. */
  contentPreview?: string;
  /** Inline author display name from the card (may lag behind the resolved profile). */
  authorDisplayName?: string;
  /** Inline author picture URL from the card (may lag). */
  authorPictureUrl?: string;
  /** True when the root is a repost wrapper. */
  isRepost: boolean;
  /** Pubkey of the reposter (set iff `isRepost`). */
  repostedByPubkey?: string;
  /** Runtime-provided relay URLs that delivered the note. */
  relayProvenance: string[];
  /** Runtime-provided relation counters for the note. */
  relationCounts: FeedRelationCounts;
  /** Rust-emitted reply attribution rows for replies already visible to the feed. */
  replyAttributions: FeedReplyAttribution[];
};

export type FeedReplyAttribution = {
  authorPubkey: string;
  authorDisplayName?: string;
  authorPictureUrl?: string;
  replyEventId: string;
  replyCreatedAt: number;
};

export type FeedRelationCounts = {
  replies: number;
  reactions: number;
  reposts: number;
  zaps: number;
  comments: number;
};

/** Decoded frame — feed rows + optional refs.profile sidecar bytes + identity fields. */
export type DecodedFrame = {
  rows: FeedRow[];
  /** Raw NRRD bytes of the `refs.profile` sidecar, or undefined when absent. */
  refsProfileBytes?: Uint8Array;
  /** Session identity from the SnapshotFrame (used by RefProfileStore for cache rebuild). */
  sessionId: bigint;
  /** Snapshot epoch from the SnapshotFrame (used by RefProfileStore for baseline rebuild). */
  snapshotEpoch: bigint;
};

/** Decode the `nmp.feed.home` projection + optional `refs.profile` sidecar from an
 *  UpdateFrame `latestUpdateBytes`. Returns `undefined` when the bytes are malformed or
 *  the frame is not a Snapshot — the caller keeps the last-good state (D6 fail-closed). */
export function decodeUpdateFrame(bytes: Uint8Array): DecodedFrame | undefined {
  try {
    const bb = new flatbuffers.ByteBuffer(bytes);
    if (!UpdateFrame.bufferHasIdentifier(bb)) return undefined;
    const frame = UpdateFrame.getRootAsUpdateFrame(bb);
    if (frame.kind() !== FrameKind.Snapshot) return undefined;
    const snap = frame.snapshot();
    if (!snap) return undefined;
    return decodeFromSnapshot(snap);
  } catch {
    return undefined;
  }
}

function decodeFromSnapshot(snap: SnapshotFrame): DecodedFrame {
  const result: DecodedFrame = {
    rows: [],
    sessionId: snap.sessionId(),
    snapshotEpoch: snap.snapshotEpoch(),
  };

  for (let i = 0; i < snap.typedProjectionsLength(); i++) {
    const proj = snap.typedProjections(i);
    if (!proj) continue;
    const key = proj.key();
    const payload = proj.payload();
    if (!payload) continue;
    const payloadBytes = payload.payloadArray();
    if (!payloadBytes || payloadBytes.length === 0) continue;

    if (key === HOME_FEED_PROJECTION_KEY && payload.fileIdentifier() === NOFS_FILE_IDENTIFIER) {
      try {
        const feedBb = new flatbuffers.ByteBuffer(payloadBytes);
        if (OpFeedSnapshot.bufferHasIdentifier(feedBb)) {
          const feedSnap = OpFeedSnapshot.getRootAsOpFeedSnapshot(feedBb);
          result.rows = extractFeedRows(feedSnap);
        }
      } catch {
        // Malformed payload — keep empty rows.
      }
    } else if (
      key === REFS_PROFILE_PROJECTION_KEY &&
      payload.fileIdentifier() === NRRD_FILE_IDENTIFIER
    ) {
      // Surface raw sidecar bytes for the RefProfileStore to merge (ADR-0063).
      result.refsProfileBytes = payloadBytes;
    }
  }

  return result;
}

function extractFeedRows(feedSnap: OpFeedSnapshot): FeedRow[] {
  const rows: FeedRow[] = [];
  for (let i = 0; i < feedSnap.cardsLength(); i++) {
    const rootCard = feedSnap.cards(i);
    if (!rootCard) continue;
    const card = rootCard.card();
    if (!card) continue;
    const id = card.id() ?? "";
    const authorPubkey = card.authorPubkey() ?? "";
    if (!id || !authorPubkey) continue;

    const row: FeedRow = {
      id,
      kind: card.kind(),
      authorPubkey,
      createdAt: Number(card.createdAt()),
      content: card.content() ?? "",
      isRepost: false,
      relayProvenance: [],
      relationCounts: {
        replies: 0,
        reactions: 0,
        reposts: 0,
        zaps: 0,
        comments: 0,
      },
      replyAttributions: [],
    };

    if (card.hasAuthorDisplayName()) {
      const n = card.authorDisplayName();
      if (n) row.authorDisplayName = n;
    }
    if (card.hasAuthorPictureUrl()) {
      const u = card.authorPictureUrl();
      if (u) row.authorPictureUrl = u;
    }
    const preview = card.contentPreview();
    if (preview) row.contentPreview = preview;

    const repostAttr = card.repostedBy();
    if (repostAttr) {
      row.isRepost = true;
      const rp = repostAttr.authorPubkey();
      if (rp) row.repostedByPubkey = rp;
    }

    const relationCounts = card.relationCounts();
    if (relationCounts) {
      row.relationCounts = {
        replies: countValue(relationCounts.replies()),
        reactions: countValue(relationCounts.reactions()),
        reposts: countValue(relationCounts.reposts()),
        zaps: countValue(relationCounts.zaps()),
        comments: countValue(relationCounts.comments()),
      };
    }

    for (let j = 0; j < rootCard.attributionLength(); j++) {
      const attribution = rootCard.attribution(j);
      if (!attribution) continue;
      const authorPubkey = attribution.authorPubkey() ?? "";
      const replyEventId = attribution.replyEventId() ?? "";
      if (!authorPubkey || !replyEventId) continue;
      const reply: FeedReplyAttribution = {
        authorPubkey,
        replyEventId,
        replyCreatedAt: Number(attribution.replyCreatedAt()),
      };
      const display = attribution.authorDisplay();
      if (display?.hasName()) {
        const name = display.name();
        if (name) reply.authorDisplayName = name;
      }
      if (display?.hasPictureUrl()) {
        const picture = display.pictureUrl();
        if (picture) reply.authorPictureUrl = picture;
      }
      row.replyAttributions.push(reply);
    }

    const provenance: string[] = [];
    for (let j = 0; j < card.relayProvenanceLength(); j++) {
      const relay = card.relayProvenance(j);
      if (relay) provenance.push(String(relay));
    }
    row.relayProvenance = provenance;

    rows.push(row);
  }
  return rows;
}

function countValue(count: RelationCount | null): number {
  if (!count) return 0;
  const value = count.count();
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) return Number.MAX_SAFE_INTEGER;
  return Number(value);
}
