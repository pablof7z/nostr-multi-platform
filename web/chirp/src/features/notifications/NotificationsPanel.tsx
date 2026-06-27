import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import { useNmpClient } from "../../nmp/context";
import { decodeNotificationsFrame, type NotificationItem } from "../../nmp/notificationsDecoder";
import { decodeRuntimeProjection } from "../../nmp/runtimeProjection";
import "./notifications.css";

const SESSION_ID = "chirp-web-notifications";

export function NotificationsPanel() {
  const { client, snapshot } = useNmpClient();
  const [openedFor, setOpenedFor] = createSignal<string | null>(null);
  const [failedFor, setFailedFor] = createSignal<string | null>(null);
  const [opening, setOpening] = createSignal(false);
  const [rows, setRows] = createSignal<NotificationItem[]>([]);
  const [unreadCount, setUnreadCount] = createSignal(0);
  const runtime = createMemo(() => decodeRuntimeProjection(snapshot().latestUpdateBytes));
  const accountPubkey = createMemo(() => runtime()?.activeAccountPubkey);

  const decodedFrame = createMemo(() =>
    decodeNotificationsFrame(snapshot().latestUpdateBytes, SESSION_ID),
  );
  createEffect(() => {
    const account = accountPubkey();
    const frame = decodedFrame();
    if (account && frame?.viewerPubkey === account) {
      setRows(frame.rows);
      setUnreadCount(frame.unreadCount);
    } else {
      setRows([]);
      setUnreadCount(0);
    }
  });

  createEffect(() => {
    const account = accountPubkey();
    if (!account || opening() || openedFor() === account || failedFor() === account) return;
    if (snapshot().status !== "running") return;
    setOpening(true);
    void client
      .openNotifications({ sessionId: SESSION_ID, accountPubkey: account })
      .then(() => {
        setFailedFor(null);
        setOpenedFor(account);
      })
      .catch(() => setFailedFor(account))
      .finally(() => setOpening(false));
  });

  onCleanup(() => {
    void client.closeNotifications(SESSION_ID);
  });

  const markAllRead = () => {
    const eventIds = rows()
      .filter((row) => !row.read)
      .map((row) => row.eventId)
      .filter(Boolean);
    if (eventIds.length === 0) return;
    void client.markNotificationsRead({ sessionId: SESSION_ID, eventIds, allVisible: false });
  };

  return (
    <section class="notifications-panel" aria-label="Notifications" data-testid="notifications-panel">
      <div class="notifications-header">
        <div>
          <p class="panel-kicker">Inbox</p>
          <h2>Notifications</h2>
        </div>
        <span class="notifications-source" data-testid="notifications-source">
          {accountPubkey()
            ? `${unreadCount()} unread / ${rows().length} interactions`
            : "sign in required"}
        </span>
      </div>

      <div class="notifications-actions" aria-label="Notification workspace status">
        <span data-state={openedFor() ? "live" : "pending"}>
          {openedFor()
            ? "live p-tag inbox"
            : failedFor()
              ? "runtime unavailable"
              : opening()
                ? "opening"
                : "waiting for identity"}
        </span>
        <span>source relays visible</span>
        <button
          type="button"
          class="notifications-mark-read"
          data-testid="notifications-mark-read"
          disabled={unreadCount() === 0}
          onClick={markAllRead}
        >
          Mark all read
        </button>
      </div>

      <Show
        when={accountPubkey()}
        fallback={
          <div class="notifications-empty" data-testid="notifications-signed-out">
            <strong>Connect a signer</strong>
            <span>Notifications open for the active account after identity is installed.</span>
          </div>
        }
      >
        <div class="notifications-list" data-testid="notifications-list">
          <Show
            when={rows().length > 0}
            fallback={
              <div class="notifications-empty" data-testid="notifications-empty">
                <strong>{openedFor() ? "No interactions yet" : "Opening notification inbox"}</strong>
                <span>
                  Replies, mentions, reactions, reposts, comments, and zaps that p-tag this
                  account appear here.
                </span>
              </div>
            }
          >
            <For each={rows()}>{(row) => <NotificationCard row={row} />}</For>
          </Show>
        </div>
      </Show>
    </section>
  );
}

function NotificationCard(props: { row: NotificationItem }) {
  const relays = () =>
    props.row.sourceRelays.map((relay) => relay.replace(/^wss?:\/\//, "")).join(", ") ||
    "unknown relay";
  return (
    <article
      class="notification-card"
      data-testid="notification-card"
      data-kind={props.row.notificationKind}
      data-read={props.row.read ? "true" : "false"}
    >
      <div class="notification-kind" aria-hidden="true">
        {kindInitial(props.row.notificationKind)}
      </div>
      <div class="notification-copy">
        <div class="notification-title-row">
          <strong>{kindLabel(props.row.notificationKind)}</strong>
          <span>kind {props.row.eventKind}</span>
        </div>
        <p>{props.row.content || "No content payload."}</p>
        <div class="notification-meta">
          <span title={props.row.actorPubkey}>{shortHex(props.row.actorPubkey)}</span>
          <Show when={props.row.targetEventId}>
            {(target) => <span title={target()}>target {shortHex(target())}</span>}
          </Show>
          <span title={props.row.sourceRelays.join(", ")}>{relays()}</span>
        </div>
      </div>
    </article>
  );
}

function kindLabel(kind: string): string {
  switch (kind) {
    case "reply":
      return "Reply";
    case "mention":
      return "Mention";
    case "reaction":
      return "Reaction";
    case "repost":
      return "Repost";
    case "zap":
      return "Zap";
    case "comment":
      return "Comment";
    default:
      return "Interaction";
  }
}

function kindInitial(kind: string): string {
  return kindLabel(kind).slice(0, 1);
}

function shortHex(value: string): string {
  return value.length > 10 ? `${value.slice(0, 6)}...${value.slice(-4)}` : value;
}
