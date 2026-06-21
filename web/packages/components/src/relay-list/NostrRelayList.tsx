import { For, Show } from "solid-js";

// One relay row, folded from the kernel snapshot: `url` + canonical `role`
// string from `configured_relays`, and `connection` from the top-level
// `relay_statuses` field. `connection` is one of the closed token set
// `connected | connecting | disconnected | error` (case-insensitive). All raw
// protocol data — the component owns only presentation (dot color, role label).
export interface RelayRow {
  url: string;
  role: string;
  connection: string;
}

// Relay list — shows configured relays with a live connection-status dot and
// role badges. Mirrors the SwiftUI / NDK `NostrRelayList`: data comes straight
// from the NMP snapshot; the host folds `relay_statuses` into each row's
// `connection`. Render-only — the host owns fetching and relay config.
export function NostrRelayList(props: {
  relays: RelayRow[];
  onRelayTap?: (relay: RelayRow) => void;
}) {
  return (
    <Show
      when={props.relays.length > 0}
      fallback={<div class="nostr-relay-list__empty">No relays configured</div>}
    >
      <ul class="nostr-relay-list" role="list">
        <For each={props.relays}>
          {(relay) => (
            <li class="nostr-relay-row">
              <button
                type="button"
                class="nostr-relay-row__btn"
                onClick={() => props.onRelayTap?.(relay)}
                disabled={!props.onRelayTap}
              >
                <span
                  class="nostr-relay-row__dot"
                  data-state={connectionState(relay.connection)}
                  style={{ "background-color": connectionColor(relay.connection) }}
                  title={relay.connection}
                  aria-label={`connection: ${relay.connection}`}
                />
                <span class="nostr-relay-row__url">{displayUrl(relay.url)}</span>
                <span class="nostr-relay-row__roles">
                  <For each={roleTokens(relay.role)}>
                    {(token) => (
                      <span
                        class="nostr-relay-row__badge"
                        style={{ color: roleTint(token), "border-color": roleTint(token) }}
                      >
                        {roleLabel(token)}
                      </span>
                    )}
                  </For>
                </span>
              </button>
            </li>
          )}
        </For>
      </ul>
    </Show>
  );
}

/** Strip the `wss://` / `ws://` scheme and any trailing slash for display. */
export function displayUrl(url: string): string {
  return url.replace(/^wss?:\/\//, "").replace(/\/$/, "");
}

function connectionState(connection: string): string {
  return connection.toLowerCase();
}

// Closed token set → semantic color. Edit to match your theme.
function connectionColor(connection: string): string {
  switch (connection.toLowerCase()) {
    case "connected":
      return "rgb(46, 194, 126)";
    case "connecting":
      return "rgb(229, 165, 10)";
    case "error":
      return "rgb(224, 79, 79)";
    default:
      return "rgb(120, 120, 132)";
  }
}

/** A canonical role string is comma-separated, e.g. `both,indexer`. */
function roleTokens(role: string): string[] {
  return role
    .split(",")
    .map((t) => t.trim())
    .filter((t) => t.length > 0);
}

function roleLabel(token: string): string {
  switch (token) {
    case "both":
      return "read+write";
    case "read":
      return "read";
    case "write":
      return "write";
    case "indexer":
      return "indexer";
    default:
      return token;
  }
}

function roleTint(token: string): string {
  switch (token) {
    case "indexer":
      return "rgb(124, 92, 255)";
    case "both":
      return "rgb(46, 194, 126)";
    case "write":
      return "rgb(229, 165, 10)";
    default:
      return "rgb(120, 120, 132)";
  }
}
