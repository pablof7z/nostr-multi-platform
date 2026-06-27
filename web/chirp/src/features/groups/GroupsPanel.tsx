import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import {
  CHIRP_PUBLIC_GROUP_RELAY_URL,
  chirpGroupRelayUrlFromSearch,
} from "../../chirpConfig";
import { decodeGroupDiscoveryFrame, type DiscoveredGroupRow } from "../../nmp/groupDecoder";
import { useNmpClient } from "../../nmp/context";
import "./groups.css";

const SESSION_ID = "chirp-web-groups";

export function GroupsPanel() {
  const { client, snapshot } = useNmpClient();
  const [opened, setOpened] = createSignal(false);
  const [opening, setOpening] = createSignal(false);
  const [rows, setRows] = createSignal<DiscoveredGroupRow[]>([]);
  const relayUrl = chirpGroupRelayUrlFromSearch(window.location.search) ?? CHIRP_PUBLIC_GROUP_RELAY_URL;

  const decodedFrame = createMemo(() => decodeGroupDiscoveryFrame(snapshot().latestUpdateBytes));
  createEffect(() => {
    const frame = decodedFrame();
    if (frame) setRows(frame.rows);
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
  });

  const relayLabel = () => relayUrl.replace(/^wss?:\/\//, "");
  const visibleRows = () => rows();

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
        <span>timeline pending</span>
        <span>membership pending</span>
      </div>

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
          <For each={visibleRows()}>{(row) => <GroupCard row={row} />}</For>
        </Show>
      </div>
    </section>
  );
}

function GroupCard(props: { row: DiscoveredGroupRow }) {
  const title = () => props.row.name || props.row.groupId;
  const subtitle = () => props.row.about || "No group description published yet.";
  const relay = () => props.row.hostRelayUrl.replace(/^wss?:\/\//, "");
  return (
    <article class="group-card" data-testid="group-card" data-group-id={props.row.groupId}>
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
        <button type="button" disabled title="Group timelines require the Rust-owned web open path.">
          Timeline
        </button>
        <button type="button" disabled title="Join and leave actions are not wired for Chirp Web yet.">
          Join
        </button>
      </div>
    </article>
  );
}

function GroupFlags(props: { row: DiscoveredGroupRow }) {
  return (
    <div class="group-flags">
      <span>{props.row.public ? "public" : "private"}</span>
      <span>{props.row.open ? "open" : "closed"}</span>
    </div>
  );
}
