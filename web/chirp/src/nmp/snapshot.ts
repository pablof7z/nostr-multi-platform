export type TimelineItem = {
  id: string;
  authorPubkey?: string;
  pubkey?: string;
  displayName?: string;
  handle?: string;
  content?: string;
  createdAt?: number;
  relativeTime?: string;
};

export type ChirpEventCard = {
  id: string;
  author_pubkey?: string;
  authorPubkey?: string;
  content?: string;
  created_at?: number;
  createdAt?: number;
};

export type ChirpTimelineSnapshot = {
  blocks: unknown[];
  cards: ChirpEventCard[];
};

export type KernelSnapshot = {
  rev?: number;
  projections?: {
    timeline?: TimelineItem[];
    profile?: { displayName?: string; handle?: string; name?: string };
    ["chirp.follow_list"]?: { follows?: unknown[] };
  };
};

export function kernelSnapshotFromEnvelope(envelope: unknown): KernelSnapshot | undefined {
  const root = objectRecord(envelope);
  if (!root) {
    return undefined;
  }
  const inner = root.t === "snapshot" ? root.v : root;
  const snapshot = objectRecord(inner);
  if (!snapshot) {
    return undefined;
  }
  return snapshot as KernelSnapshot;
}

export function timelineFromKernel(snapshot: KernelSnapshot | undefined): TimelineItem[] {
  return Array.isArray(snapshot?.projections?.timeline) ? snapshot.projections.timeline : [];
}

export function chirpTimelineFromEnvelope(envelope: unknown): ChirpTimelineSnapshot | undefined {
  const root = objectRecord(envelope);
  if (!root) {
    return undefined;
  }
  const maybeChirp = objectRecord(root.chirpTimeline ?? root.chirp_timeline ?? root.chirp);
  const candidate = maybeChirp ?? objectRecord(root);
  if (!candidate || !Array.isArray(candidate.blocks) || !Array.isArray(candidate.cards)) {
    return undefined;
  }
  return { blocks: candidate.blocks, cards: candidate.cards as ChirpEventCard[] };
}

export function displayRows(
  kernel: KernelSnapshot | undefined,
  chirp: ChirpTimelineSnapshot | undefined,
): TimelineItem[] {
  const timeline = timelineFromKernel(kernel);
  if (timeline.length > 0) {
    return timeline;
  }
  return chirp?.cards.map(cardFromChirpEvent) ?? [];
}

function cardFromChirpEvent(card: ChirpEventCard): TimelineItem {
  return {
    id: card.id,
    authorPubkey: card.author_pubkey ?? card.authorPubkey,
    content: card.content,
    createdAt: card.created_at ?? card.createdAt,
  };
}

function objectRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : undefined;
}
