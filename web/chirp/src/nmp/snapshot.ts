/** ADR-0035 reply-attribution badge attached to a feed item. */
import type { FeedItem, FeedCountState } from "./feedProjection";
import type { ContentTreeWire } from "./generated/nmp/content/content-tree-wire";

export type AttributionBadge = {
  authorPubkey: string;
  authorDisplayName?: string;
  authorPictureUrl?: string;
  replyEventId: string;
  replyCreatedAt: number;
};

/** NIP-18 repost badge — who surfaced this note via kind:6. */
export type RepostBadge = {
  authorPubkey: string;
  authorDisplayName?: string;
};

export type TimelineItem = {
  id: string;
  authorPubkey?: string;
  pubkey?: string;
  displayName?: string;
  handle?: string;
  content?: string;
  contentTree?: ContentTreeWire;
  createdAt?: number;
  relativeTime?: string;
  relationCounts?: RelationCounts;
  /** ADR-0035 attribution badges decoded from the nmp.feed.home projection. */
  attribution?: AttributionBadge[];
  /** NIP-18 repost attribution — absent for plain notes. */
  repostedBy?: RepostBadge;
};

export type RelationCounts = { replies?: CountState; reactions?: CountState; reposts?: CountState };
export type CountState = { status?: string; count?: number };
export type ChirpEventCard = {
  id: string;
  author_pubkey?: string;
  authorPubkey?: string;
  author_display?: { name?: string };
  authorDisplay?: { name?: string };
  content?: string;
  created_at?: number;
  createdAt?: number;
  relation_counts?: RelationCounts;
  relationCounts?: RelationCounts;
};
export type ChirpTimelineSnapshot = { blocks: unknown[]; cards: ChirpEventCard[] };
export type AccountLine = { id: string; display: string; npub: string; signer: string; active: boolean };
export type OutboxLine = { handle: string; title: string; statusLabel: string; preview: string; canRetry: boolean };
export type RelayEditLine = { url: string; role: string };
export type RelayDiagnosticLine = { url: string; role: string; status: string };
export type WalletLine = { status: string; relayUrl: string; walletNpub: string; balanceMsats?: number };
export type SummaryLine = { title: string; subtitle: string };
export type ProfileLine = { pubkey: string; display: string; about: string; noteCount: string; actionLabel: string };
export type ThreadLine = { focusedEventId: string; state: string; previousLabel: string; nextLabel: string; itemCount: number };
export type DmConversationLine = { peerPubkey: string; peerDisplay: string; latest: string; messages: MessageLine[] };
export type MessageLine = { id: string; author: string; content: string; outgoing: boolean };
export type GroupLine = { hostRelayUrl: string; groupId: string; name: string; about: string; memberCount: number; open: boolean };
export type FeatureSnapshot = {
  accounts: AccountLine[];
  activeAccount: string;
  outbox: OutboxLine[];
  outboxSummary: SummaryLine;
  configuredRelays: RelayEditLine[];
  relayDiagnostics: RelayDiagnosticLine[];
  wallet: WalletLine;
  dmConversations: DmConversationLine[];
  groupMessages: MessageLine[];
  discoveredGroups: GroupLine[];
  followCount: number;
  settingsHub: SummaryLine;
  authorProfile?: ProfileLine;
  thread?: ThreadLine;
};
// The generic JSON `projections` map was removed in PR #1515 (escape hatch #2
// eliminated). `KernelSnapshot` no longer carries a `projections` field — all
// projection data arrives through typed FlatBuffers sidecars.
export type KernelSnapshot = { rev?: number };

export function kernelSnapshotFromEnvelope(envelope: unknown): KernelSnapshot | undefined {
  const root = objectRecord(envelope);
  if (!root) {
    return undefined;
  }
  const inner = root.t === "snapshot" ? root.v : root;
  const snapshot = objectRecord(inner);
  return snapshot ? (snapshot as KernelSnapshot) : undefined;
}

// Zero-state constant for the FeatureSnapshot — returned by
// `featureSnapshotFromEnvelope` which is always called with `undefined` in
// App.tsx. The generic JSON `projections` map was deleted in PR #1515
// (escape hatch #2 eliminated); all projection data now arrives through the
// typed FlatBuffers sidecar path. dmConversations, groupMessages,
// discoveredGroups etc. stay empty here; callers that need real data must
// decode the typed sidecar.
const ZERO_FEATURE_SNAPSHOT: FeatureSnapshot = {
  accounts: [],
  activeAccount: "",
  outbox: [],
  outboxSummary: { title: "", subtitle: "" },
  configuredRelays: [],
  relayDiagnostics: [],
  wallet: { status: "", relayUrl: "", walletNpub: "", balanceMsats: undefined },
  dmConversations: [],
  groupMessages: [],
  discoveredGroups: [],
  followCount: 0,
  settingsHub: { title: "", subtitle: "" },
  authorProfile: undefined,
  thread: undefined,
};

export function featureSnapshotFromEnvelope(_envelope: unknown): FeatureSnapshot {
  // The generic JSON `projections` map no longer exists on the wire (PR #1515).
  // This function always returns the zero-state FeatureSnapshot. Real projection
  // data arrives through the typed FlatBuffers sidecar path.
  return ZERO_FEATURE_SNAPSHOT;
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

/**
 * Convert decoded `nmp.feed.home` FeedItem[] into the TimelineItem[] shape
 * that HomePanel renders. Used by App.tsx when the real kernel feed projection
 * is available (the latestUpdate JSON path is the dead fallback; this is live).
 *
 * `FeedRelationCounts` uses `{type:"known",count}` / `{type:"loading"}` —
 * map to `CountState` `{count}` / `{status:"loading"}` so HomePanel's
 * `countLabel()` renders correctly.
 *
 * `resolvedProfiles` is the decoded `resolved_profiles` KRPR map (pubkey →
 * display name). Root cards carry no denormalized author display copy (GH #920
 * ADR-0032 raw-data doctrine); the presentation layer joins here instead.
 * When absent (no profile claimed yet) `item.authorDisplayName` stays undefined
 * and `displayAuthor` falls back to `shortKey(authorPubkey)`.
 */
export function feedItemsToRows(items: FeedItem[], resolvedProfiles?: Map<string, string>): TimelineItem[] {
  return items.map((item): TimelineItem => ({
    id: item.id,
    authorPubkey: item.authorPubkey,
    displayName: item.authorDisplayName ?? resolvedProfiles?.get(item.authorPubkey),
    content: item.content,
    contentTree: item.contentTree,
    createdAt: item.createdAt,
    relationCounts: {
      replies: toCountState(item.relationCounts.replies),
      reactions: toCountState(item.relationCounts.reactions),
      reposts: toCountState(item.relationCounts.reposts),
    },
    attribution: item.attribution.map((a) => ({
      authorPubkey: a.authorPubkey,
      authorDisplayName: a.authorDisplayName ?? resolvedProfiles?.get(a.authorPubkey),
      authorPictureUrl: a.authorPictureUrl,
      replyEventId: a.replyEventId,
      replyCreatedAt: a.replyCreatedAt,
    })),
    repostedBy: item.repostedBy
      ? { authorPubkey: item.repostedBy.authorPubkey, authorDisplayName: item.repostedBy.authorDisplayName }
      : undefined,
  }));
}

function toCountState(state: FeedCountState): CountState {
  return state.type === "known" ? { count: state.count } : { status: "loading" };
}

export function displayAuthor(item: TimelineItem): string {
  return item.displayName ?? item.handle ?? shortKey(item.authorPubkey ?? item.pubkey);
}

export function shortKey(value?: string): string {
  if (!value) {
    return "unknown";
  }
  return value.length > 12 ? `${value.slice(0, 8)}..${value.slice(-4)}` : value;
}

function accountFrom(value: unknown): AccountLine {
  const row = objectRecord(value) ?? {};
  return {
    id: str(row.id),
    display: first(row, "display_name", "displayName", "npub"),
    npub: str(row.npub),
    signer: first(row, "signer_label", "signerLabel", "signer_kind"),
    active: bool(row.is_active) || bool(row.isActive),
  };
}

function outboxFrom(value: unknown): OutboxLine {
  const row = objectRecord(value) ?? {};
  return {
    handle: str(row.handle),
    title: str(row.title),
    statusLabel: first(row, "status_label", "statusLabel", "status"),
    preview: str(row.preview),
    canRetry: bool(row.can_retry) || bool(row.canRetry),
  };
}

function relayEditFrom(value: unknown): RelayEditLine {
  const row = objectRecord(value) ?? {};
  return { url: str(row.url), role: str(row.role) };
}

function relayDiagnosticFrom(value: unknown): RelayDiagnosticLine {
  const row = objectRecord(value) ?? {};
  return { url: str(row.url), role: str(row.role), status: str(row.status) };
}

function walletFrom(value: unknown): WalletLine {
  const row = objectRecord(value) ?? {};
  return {
    status: str(row.status),
    relayUrl: first(row, "relay_url", "relayUrl"),
    walletNpub: first(row, "wallet_npub", "walletNpub"),
    balanceMsats: num(row.balance_msats ?? row.balanceMsats),
  };
}

function cardFromChirpEvent(card: ChirpEventCard): TimelineItem {
  // aim.md §2 — display_name is the kind:0 value (may be null until
  // kind:0 arrives). The card's nested `author_display` object's
  // `name` field is now `Option<String>`, surfaced as JSON null when
  // absent — the optional chain handles both shapes.
  const authorDisplay = card.author_display ?? card.authorDisplay;
  return {
    id: card.id,
    authorPubkey: card.author_pubkey ?? card.authorPubkey,
    displayName: authorDisplay?.name ?? undefined,
    content: card.content,
    createdAt: card.created_at ?? card.createdAt,
    relationCounts: card.relation_counts ?? card.relationCounts,
  };
}

function profileFrom(value: unknown): ProfileLine | undefined {
  const wrapper = objectRecord(value);
  if (!wrapper) {
    return undefined;
  }
  const profile = objectRecord(wrapper.profile) ?? wrapper;
  const action = objectRecord(wrapper.primary_action ?? wrapper.primaryAction);
  const pubkey = first(wrapper, "pubkey") || str(profile.pubkey);
  // aim.md §2 — ProfileCard now ships display_name as Option<String>
  // (null when no kind:0). The web shell formats its own fallback
  // (raw hex abbreviation) at display time.
  const displayName = first(profile, "display_name", "displayName");
  const display =
    displayName || (pubkey.length >= 16 ? `${pubkey.slice(0, 8)}…${pubkey.slice(-8)}` : pubkey);
  return {
    pubkey,
    display,
    about: str(profile.about),
    noteCount: first(wrapper, "note_count_display", "noteCountDisplay"),
    actionLabel: str(action?.label),
  };
}

function threadFrom(value: unknown): ThreadLine | undefined {
  const row = objectRecord(value);
  if (!row) {
    return undefined;
  }
  return {
    focusedEventId: first(row, "focused_event_id", "focusedEventId"),
    state: str(row.state),
    previousLabel: first(row, "previous_count_label", "previousCountLabel"),
    nextLabel: first(row, "next_count_label", "nextCountLabel"),
    itemCount: array(row.items).length,
  };
}

function summaryFrom(value: unknown): SummaryLine {
  const row = objectRecord(value) ?? {};
  return { title: str(row.title), subtitle: str(row.subtitle) };
}

function settingsHubFrom(value: unknown): SummaryLine {
  const row = objectRecord(value) ?? {};
  const count = typeof row.relay_count === "number" ? row.relay_count :
                typeof row.relayCount === "number" ? row.relayCount : undefined;
  const subtitle = count === undefined ? "" :
                   count === 0 ? "No relays configured" :
                   count === 1 ? "1 relay" :
                   `${count} relays`;
  return { title: "Settings", subtitle };
}

function first(value: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    const candidate = str(value[key]);
    if (candidate) {
      return candidate;
    }
  }
  return "";
}

function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function str(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function bool(value: unknown): boolean {
  return typeof value === "boolean" ? value : false;
}

function num(value: unknown): number | undefined {
  return typeof value === "number" ? value : undefined;
}

function objectRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : undefined;
}
