import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import {
  CHIRP_PUBLIC_GROUP_RELAY_URL,
  chirpGroupRelayUrlFromSearch,
} from "../../chirpConfig";
import { decodeGroupDiscoveryFrame, type DiscoveredGroupRow } from "../../nmp/groupDecoder";
import {
  decodeGroupEventsFrame,
  type GroupEventsRow,
} from "../../nmp/groupEventsDecoder";
import { useNmpClient } from "../../nmp/context";
import { blockedWorkspaceCommand } from "../../nmp/actions";
import "./groups.css";

const SESSION_ID = "chirp-web-groups";
const TIMELINE_SESSION_ID = "chirp-web-group-timeline";
const JOIN_CAPABILITY = "nmp.nip29.join";

export function GroupsPanel() {
  const { client, snapshot } = useNmpClient();
  const [opened, setOpened] = createSignal(false);
  const [opening, setOpening] = createSignal(false);
  const [rows, setRows] = createSignal<DiscoveredGroupRow[]>([]);
  const [selectedGroup, setSelectedGroup] = createSignal<DiscoveredGroupRow | null>(null);
  const [timelineRows, setTimelineRows] = createSignal<GroupEventsRow[]>([]);
  const [timelineOpening, setTimelineOpening] = createSignal(false);
  const [lastCapability, setLastCapability] = createSignal<string | null>(null);
  const [busyCapability, setBusyCapability] = createSignal<string | null>(null);
  const relayUrl = chirpGroupRelayUrlFromSearch(window.location.search) ?? CHIRP_PUBLIC_GROUP_RELAY_URL;

  const decodedFrame = createMemo(() => decodeGroupDiscoveryFrame(snapshot().latestUpdateBytes));
  createEffect(() => {
    const frame = decodedFrame();
    if (frame) setRows(frame.rows);
  });

  const decodedTimelineFrame = createMemo(() => decodeGroupEventsFrame(snapshot().latestUpdateBytes));
  createEffect(() => {
    const frame = decodedTimelineFrame();
    if (frame) setTimelineRows(frame.rows);
  });

  createEffect(() => {
    if (opened() || opening()) return;
    if (snapshot().status !== "running") return;
    setOpening(true);
    void client
      .openGroupDiscovery({ sessionId: SESSION_ID, relayUrl })
      .then(() => setOpened(true))
      .finally(() => setOpening(false));
  });

  onCleanup(() => {
    void client.closeGroupDiscovery(SESSION_ID);
    void client.closeGroupEvents(TIMELINE_SESSION_ID);
  });

  const relayLabel = () => relayUrl.replace(/^wss?:\/\//, "");
  const visibleRows = () => rows();
  const inspect = async (capability: string) => {
    if (busyCapability()) return;
    setBusyCapability(capability);
    try {
      await client.dispatchCommand(blockedWorkspaceCommand(capability));
      setLastCapability(capability);
    } finally {
      setBusyCapability(null);
    }
  };
  const openTimeline = async (row: DiscoveredGroupRow) => {
    if (timelineOpening()) return;
    setSelectedGroup(row);
    setTimelineRows([]);
    setTimelineOpening(true);
    try {
      await client.openGroupEvents({
        sessionId: TIMELINE_SESSION_ID,
        relayUrl: row.hostRelayUrl,
        groupId: row.groupId,
        // Chat view: kinds 9 (chat) + 11 (thread) — the consumer declares the
        // kind set; NIP-29 owns only the `#h` routing (issue #2187).
        kinds: [9, 11],
      });
    } finally {
      setTimelineOpening(false);
    }
  };

  return (
    <section class="groups-panel" aria-label="Groups" data-testid="groups-panel">
      <div class="groups-header">
        <div>
          <p class="panel-kicker">NIP-29</p>
          <h2>Public groups</h2>
        </div>
        <span class="groups-source" title={relayUrl} data-testid="groups-source">
          {opening() ? "opening" : `${visibleRows().length} groups`} · {relayLabel()}
        </span>
      </div>

      <div class="groups-actions" aria-label="Group workspace status">
        <span data-state={opened() ? "live" : "pending"}>{opened() ? "live discovery" : "opening"}</span>
        <span data-state={selectedGroup() ? "live" : "pending"}>
          {selectedGroup() ? "live timeline" : "timeline ready"}
        </span>
        <span data-state="blocked">membership blocked</span>
      </div>

      <Show when={selectedGroup()}>
        {(group) => (
          <GroupTimelinePanel
            group={group()}
            opening={timelineOpening()}
            rows={timelineRows()}
          />
        )}
      </Show>

      <div class="groups-list" data-testid="groups-list">
        <Show
          when={visibleRows().length > 0}
          fallback={
            <div class="groups-empty" data-testid="groups-empty">
              <strong>{opened() ? "No public groups returned" : "Opening group relay"}</strong>
              <span>{relayLabel()}</span>
            </div>
          }
        >
          <For each={visibleRows()}>
            {(row) => (
              <GroupCard
                row={row}
                selected={selectedGroup()?.groupId === row.groupId}
                busyCapability={busyCapability()}
                timelineOpening={timelineOpening()}
                onOpenTimeline={openTimeline}
                onInspect={inspect}
              />
            )}
          </For>
        </Show>
      </div>

      <Show when={lastCapability()}>
        {(capability) => (
          <p class="groups-diagnostic" role="status" data-testid="groups-diagnostic">
            Recorded diagnostic for <code>{capability()}</code>.
          </p>
        )}
      </Show>
    </section>
  );
}

function GroupCard(props: {
  row: DiscoveredGroupRow;
  selected: boolean;
  busyCapability: string | null;
  timelineOpening: boolean;
  onOpenTimeline: (row: DiscoveredGroupRow) => void;
  onInspect: (capability: string) => void;
}) {
  const title = () => props.row.name || props.row.groupId;
  const subtitle = () => props.row.about || "No group description published yet.";
  const relay = () => props.row.hostRelayUrl.replace(/^wss?:\/\//, "");
  return (
    <article
      class="group-card"
      data-testid="group-card"
      data-selected={props.selected}
      data-group-id={props.row.groupId}
    >
      <div class="group-avatar" aria-hidden="true">
        <Show when={props.row.picture} fallback={<span>{title().slice(0, 1).toUpperCase()}</span>}>
          {(picture) => <img src={picture()} alt="" loading="lazy" />}
        </Show>
      </div>

      <div class="group-copy">
        <div class="group-title-row">
          <div>
            <strong>{title()}</strong>
            <span>{props.row.groupId}</span>
          </div>
          <GroupFlags row={props.row} />
        </div>
        <p>{subtitle()}</p>
        <div class="group-meta">
          <span>{props.row.memberCount} members</span>
          <span>{props.row.adminCount} admins</span>
          <span title={props.row.hostRelayUrl}>{relay()}</span>
        </div>
      </div>

      <div class="group-controls">
        <button
          type="button"
          data-testid="group-timeline-open"
          disabled={props.timelineOpening}
          onClick={() => props.onOpenTimeline(props.row)}
        >
          {props.selected ? "Timeline open" : "Open timeline"}
        </button>
        <button
          type="button"
          data-testid="group-join-inspect"
          disabled={props.busyCapability !== null}
          onClick={() => props.onInspect(JOIN_CAPABILITY)}
        >
          Inspect join
        </button>
      </div>
    </article>
  );
}

function GroupTimelinePanel(props: {
  group: DiscoveredGroupRow;
  opening: boolean;
  rows: GroupEventsRow[];
}) {
  const relay = () => props.group.hostRelayUrl.replace(/^wss?:\/\//, "");
  const title = () => props.group.name || props.group.groupId;
  return (
    <section class="group-timeline" data-testid="group-timeline-panel" aria-label="Group timeline">
      <div class="group-timeline-head">
        <div>
          <p class="panel-kicker">Timeline</p>
          <h3>{title()}</h3>
        </div>
        <span title={props.group.hostRelayUrl}>{relay()}</span>
      </div>
      <Show
        when={props.rows.length > 0}
        fallback={
          <div class="group-timeline-empty" data-testid="group-timeline-empty">
            <strong>{props.opening ? "Opening group timeline" : "No group messages returned"}</strong>
            <span>{props.group.groupId}</span>
          </div>
        }
      >
        <ol class="group-timeline-list">
          <For each={props.rows}>
            {(row) => (
              <li class="group-timeline-row" data-testid="group-timeline-row">
                <div>
                  <strong>{shortPubkey(row.pubkey)}</strong>
                  <span>{formatTimelineTime(row.createdAt)} · kind {row.kind}</span>
                </div>
                <p>{row.content}</p>
              </li>
            )}
          </For>
        </ol>
      </Show>
    </section>
  );
}

function shortPubkey(pubkey: string): string {
  return pubkey.length > 14 ? `${pubkey.slice(0, 8)}...${pubkey.slice(-4)}` : pubkey;
}

function formatTimelineTime(createdAt: number): string {
  if (!Number.isFinite(createdAt) || createdAt <= 0) return "unknown time";
  return new Date(createdAt * 1000).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function GroupFlags(props: { row: DiscoveredGroupRow }) {
  return (
    <div class="group-flags">
      <span>{props.row.public ? "public" : "private"}</span>
      <span>{props.row.open ? "open" : "closed"}</span>
    </div>
  );
}
