import { createSignal, For, Show } from "solid-js";
import type { WorkerEvent } from "@nmp/runtime-web";
import {
  addRelayCommand,
  publishRelayPreferencesCommand,
  removeRelayCommand,
} from "../../nmp/actions";
import { useNmpClient } from "../../nmp/context";
import type { RelayRuntimeRow, RuntimeProjection } from "../../nmp/runtimeProjection";
import "./relays.css";

const ROLE_OPTIONS = ["both,indexer", "both", "read", "write", "indexer"] as const;

function compactUrl(url: string): string {
  return url.replace(/^wss?:\/\//, "");
}

function eventStatus(event: WorkerEvent | undefined): string {
  if (!event) return "";
  if (event.type === "action_accepted") return "Relay settings updated.";
  if (event.type === "sign_request") return "Signer approval requested for relay preferences.";
  if (event.type === "sign_completed") return "Relay preferences signed and sent.";
  if (event.type === "sign_failed") return event.reason;
  if (event.type === "capability_failure") return event.reason;
  if (event.type === "error") return event.message;
  return "";
}

function relayDetail(relay: RelayRuntimeRow): string {
  const bits = [
    relay.auth,
    `${relay.subscriptions} subs`,
    `${relay.eventsRx} events`,
    `${relay.reconnects} reconnects`,
  ];
  return bits.join(" · ");
}

export function RelaySettingsPanel(props: {
  diagnostics?: RuntimeProjection;
  canPublishRelayPreferences: boolean;
}) {
  const { client } = useNmpClient();
  const [url, setUrl] = createSignal("");
  const [role, setRole] = createSignal<(typeof ROLE_OPTIONS)[number]>("both,indexer");
  const [busy, setBusy] = createSignal(false);
  const [status, setStatus] = createSignal("");

  const relays = () => props.diagnostics?.relays ?? [];

  const applyResult = (events: readonly WorkerEvent[]) => {
    const message = eventStatus(events[0]);
    if (message) setStatus(message);
  };

  const addRelay = async () => {
    const relayUrl = url().trim();
    if (!relayUrl) {
      setStatus("Enter a relay URL.");
      return;
    }
    setBusy(true);
    setStatus("");
    try {
      const snapshot = await client.dispatchCommand(addRelayCommand(relayUrl, role()));
      applyResult(snapshot.events);
      setUrl("");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const removeRelay = async (relayUrl: string) => {
    setBusy(true);
    setStatus("");
    try {
      const snapshot = await client.dispatchCommand(removeRelayCommand(relayUrl));
      applyResult(snapshot.events);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const publishPreferences = async () => {
    setBusy(true);
    setStatus("");
    try {
      const snapshot = await client.dispatchCommand(publishRelayPreferencesCommand());
      applyResult(snapshot.events);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section class="relay-settings-panel" aria-label="Relay settings">
      <div class="relay-settings-header">
        <div>
          <p class="panel-kicker">Relays</p>
          <h2>Connection setup</h2>
        </div>
        <div class="relay-header-actions">
          <button
            type="button"
            data-testid="relay-publish-preferences"
            disabled={busy() || !props.canPublishRelayPreferences || relays().length === 0}
            onClick={() => void publishPreferences()}
          >
            Publish preferences
          </button>
          <span class="relay-count" data-testid="relay-count">{relays().length}</span>
        </div>
      </div>

      <form
        class="relay-form"
        onSubmit={(event) => {
          event.preventDefault();
          void addRelay();
        }}
      >
        <label>
          <span>Relay URL</span>
          <input
            data-testid="relay-url-input"
            type="url"
            inputMode="url"
            placeholder="wss://relay.example"
            value={url()}
            onInput={(event) => setUrl(event.currentTarget.value)}
          />
        </label>
        <label>
          <span>Role</span>
          <select
            data-testid="relay-role-select"
            value={role()}
            onInput={(event) => setRole(event.currentTarget.value as (typeof ROLE_OPTIONS)[number])}
          >
            <For each={ROLE_OPTIONS}>{(option) => <option value={option}>{option}</option>}</For>
          </select>
        </label>
        <button data-testid="relay-add-button" type="submit" disabled={busy()}>
          {busy() ? "Saving" : "Add"}
        </button>
      </form>

      <Show when={status()}>
        {(message) => (
          <p class="relay-status" role="status">
            {message()}
          </p>
        )}
      </Show>

      <div class="configured-relays" data-testid="configured-relays">
        <Show
          when={relays().length > 0}
          fallback={<p class="relay-empty">No configured relays are visible yet.</p>}
        >
          <For each={relays()}>
            {(relay) => (
              <div class="configured-relay-row" data-connection={relay.connection}>
                <div>
                  <strong title={relay.url}>{compactUrl(relay.url)}</strong>
                  <span>{relay.role}</span>
                  <small>{relayDetail(relay)}</small>
                  <Show when={relay.lastError || relay.lastNotice}>
                    <small>{relay.lastError ?? relay.lastNotice}</small>
                  </Show>
                </div>
                <button
                  type="button"
                  data-testid="relay-remove-button"
                  disabled={busy()}
                  onClick={() => void removeRelay(relay.url)}
                >
                  Remove
                </button>
              </div>
            )}
          </For>
        </Show>
      </div>
    </section>
  );
}
