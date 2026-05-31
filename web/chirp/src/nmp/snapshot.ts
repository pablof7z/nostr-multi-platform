export type TimelineItem = {
  id: string;
  authorPubkey?: string;
  pubkey?: string;
  displayName?: string;
  handle?: string;
  content?: string;
  createdAt?: number;
  relativeTime?: string;
  relationCounts?: RelationCounts;
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
  relayEditRows: RelayEditLine[];
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
export type KernelSnapshot = { rev?: number; projections?: Record<string, unknown> & { timeline?: TimelineItem[] } };

export function kernelSnapshotFromEnvelope(envelope: unknown): KernelSnapshot | undefined {
  const root = objectRecord(envelope);
  if (!root) {
    return undefined;
  }
  const inner = root.t === "snapshot" ? root.v : root;
  const snapshot = objectRecord(inner);
  return snapshot ? (snapshot as KernelSnapshot) : undefined;
}

export function featureSnapshotFromEnvelope(envelope: unknown): FeatureSnapshot {
  const source = objectRecord(kernelSnapshotFromEnvelope(envelope)?.projections) ?? {};
  return {
    accounts: array(source.accounts).map(accountFrom),
    activeAccount: str(source.active_account),
    outbox: array(source.publish_outbox ?? source.publishOutbox).map(outboxFrom),
    outboxSummary: summaryFrom(source.outbox_summary ?? source.outboxSummary),
    relayEditRows: array(source.relay_edit_rows ?? source.relayEditRows).map(relayEditFrom),
    relayDiagnostics: array(source.relay_diagnostics ?? source.relayDiagnostics).map(relayDiagnosticFrom),
    wallet: walletFrom(source.wallet),
    dmConversations: dmFrom(source),
    groupMessages: messagesFrom(projection(source, "nmp.nip29.group_chat")),
    discoveredGroups: groupsFrom(source),
    followCount: array(objectRecord(projection(source, "nmp.follow_list"))?.follows).length,
    settingsHub: settingsHubFrom(source.settings_hub ?? source.settingsHub),
    authorProfile: profileFrom(source.author_view ?? source.authorView),
    thread: threadFrom(source.thread_view ?? source.threadView),
  };
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

export function displayRows(kernel: KernelSnapshot | undefined, chirp: ChirpTimelineSnapshot | undefined): TimelineItem[] {
  const timeline = timelineFromKernel(kernel);
  return timeline.length > 0 ? timeline : (chirp?.cards.map(cardFromChirpEvent) ?? []);
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

function dmFrom(projections: Record<string, unknown>): DmConversationLine[] {
  const inbox = objectRecord(projection(projections, "nmp.nip17.dm_inbox"));
  return array(inbox?.conversations).map((value) => {
    const row = objectRecord(value) ?? {};
    const messages = messagesFrom(row);
    // aim.md §2: backend ships raw hex peer_pubkey; the presentation
    // layer abbreviates locally. Falls back to the raw hex when
    // shorter than 16 chars.
    const peerPubkey = first(row, "peer_pubkey", "peerPubkey");
    const peerDisplay =
      peerPubkey.length >= 16
        ? `${peerPubkey.slice(0, 8)}…${peerPubkey.slice(-8)}`
        : peerPubkey;
    return {
      peerPubkey,
      peerDisplay,
      latest: messages.length > 0 ? messages[messages.length - 1].content : "",
      messages,
    };
  });
}

function messagesFrom(value: unknown): MessageLine[] {
  const row = objectRecord(value);
  return array(row?.messages).map((message) => {
    const item = objectRecord(message) ?? {};
    return {
      id: str(item.id),
      author: first(item, "sender_pubkey", "senderPubkey", "pubkey"),
      content: str(item.content),
      outgoing: bool(item.is_outgoing) || bool(item.isOutgoing),
    };
  });
}

function groupsFrom(projections: Record<string, unknown>): GroupLine[] {
  const groups = objectRecord(projection(projections, "nmp.nip29.discovered_groups"));
  return array(groups?.groups).map((value) => {
    const row = objectRecord(value) ?? {};
    const groupId = first(row, "group_id", "groupId");
    return {
      hostRelayUrl: first(row, "host_relay_url", "hostRelayUrl"),
      groupId,
      name: first(row, "name") || groupId,
      about: str(row.about),
      memberCount: num(row.member_count ?? row.memberCount) ?? 0,
      open: bool(row.open),
    };
  });
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

function projection(value: Record<string, unknown>, key: string): unknown {
  return value[key] ?? value[key.split("_").join("")];
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
