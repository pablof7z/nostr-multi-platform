import { createSignal, For, Show } from "solid-js";
import type { WorkerEvent } from "@nmp/runtime-web";
import { useNmpClient } from "../../nmp/context";
import type { RuntimeProjection } from "../../nmp/runtimeProjection";
import "./diagnostics.css";

type RoutingSummary = {
  title: string;
  detail: string;
};

function compactUrl(url: string): string {
  return url.replace(/^wss?:\/\//, "");
}

function latestWriteEvent(events: readonly WorkerEvent[]): WorkerEvent | undefined {
  return events.find((event) =>
    event.type === "sign_request" ||
    event.type === "sign_completed" ||
    event.type === "sign_failed" ||
    event.type === "capability_failure" ||
    event.type === "action_accepted"
  );
}

function writeLabel(event: WorkerEvent | undefined): string {
  if (!event) return "No write attempted";
  switch (event.type) {
    case "sign_request":
      return "Awaiting signer approval";
    case "sign_completed":
      return "Signature completed";
    case "sign_failed":
      return "Signing failed";
    case "capability_failure":
      return "Runtime rejected action";
    case "action_accepted":
      return "Runtime accepted action";
    default:
      return "No write attempted";
  }
}

function writeDetail(event: WorkerEvent | undefined): string {
  if (!event) return "Publish a note to see signer and outbox activity here.";
  switch (event.type) {
    case "sign_request":
      return `Signer request ${event.correlation_id} is pending.`;
    case "sign_completed":
      return `Signer request ${event.correlation_id} returned a signed event.`;
    case "sign_failed":
      return event.reason;
    case "capability_failure":
      return event.reason;
    case "action_accepted":
      return `${event.action_type} accepted with ${event.correlation_id}.`;
    default:
      return "Publish a note to see signer and outbox activity here.";
  }
}

function summarizeRouting(json: string): RoutingSummary {
  try {
    const parsed = JSON.parse(json) as unknown;
    if (Array.isArray(parsed)) {
      return { title: `${parsed.length} routing records`, detail: JSON.stringify(parsed.slice(0, 2)) };
    }
    if (parsed && typeof parsed === "object") {
      const keys = Object.keys(parsed);
      return { title: "Routing trace available", detail: keys.slice(0, 6).join(", ") || json };
    }
  } catch {
    return { title: "Routing trace", detail: json };
  }
  return { title: "Routing trace", detail: json };
}

export function DiagnosticsPanel(props: {
  diagnostics?: RuntimeProjection;
  events: readonly WorkerEvent[];
}) {
  const { client } = useNmpClient();
  const [routing, setRouting] = createSignal<RoutingSummary | null>(null);
  const [refreshing, setRefreshing] = createSignal(false);
  const [refreshError, setRefreshError] = createSignal<string | null>(null);

  const refreshRouting = async () => {
    setRefreshing(true);
    setRefreshError(null);
    try {
      const snap = await client.refreshRoutingDecisions();
      const event = snap.events.find((item) => item.type === "routing_decisions");
      if (event?.type === "routing_decisions") {
        setRouting(summarizeRouting(event.json));
      } else {
        setRefreshError("Routing diagnostics did not return a trace.");
      }
    } catch (error) {
      setRefreshError(error instanceof Error ? error.message : String(error));
    } finally {
      setRefreshing(false);
    }
  };

  const relays = () => props.diagnostics?.relays ?? [];
  const interests = () => props.diagnostics?.interests ?? [];
  const wireSubscriptions = () => props.diagnostics?.wireSubscriptions ?? [];
  const writeEvent = () => latestWriteEvent(props.events);

  return (
    <section class="diagnostics-panel" aria-label="Runtime diagnostics">
      <div class="diagnostics-header">
        <div>
          <p class="panel-kicker">Diagnostics</p>
          <h2>Relay and outbox state</h2>
        </div>
        <button
          class="diagnostics-refresh"
          type="button"
          disabled={refreshing()}
          onClick={() => void refreshRouting()}
        >
          {refreshing() ? "Checking" : "Routing"}
        </button>
      </div>

      <div class="outbox-state" data-state={writeEvent()?.type ?? "idle"}>
        <strong>{writeLabel(writeEvent())}</strong>
        <span>{writeDetail(writeEvent())}</span>
      </div>

      <Show when={props.diagnostics?.storeOpenFailure}>
        {(failure) => (
          <p class="diagnostics-error" role="alert">
            Storage: {failure()}
          </p>
        )}
      </Show>

      <div class="relay-list" data-testid="relay-diagnostics">
        <Show
          when={relays().length > 0}
          fallback={<p class="diagnostics-empty">Relay status has not reached the UI yet.</p>}
        >
          <For each={relays().slice(0, 5)}>
            {(relay) => (
              <div class="relay-row" data-connection={relay.connection}>
                <div>
                  <strong title={relay.url}>{compactUrl(relay.url)}</strong>
                  <span>{relay.role}</span>
                </div>
                <div class="relay-metrics">
                  <span>{relay.connection}</span>
                  <span>{relay.eventsRx} events</span>
                </div>
              </div>
            )}
          </For>
        </Show>
      </div>

      <Show when={interests().length > 0}>
        <div class="interest-strip" aria-label="Active interests">
          <For each={interests().slice(0, 3)}>
            {(interest) => (
              <span title={interest.key}>
                {interest.state} · {interest.relayCount} relays
              </span>
            )}
          </For>
        </div>
      </Show>

      <Show when={wireSubscriptions().length > 0}>
        <div class="wire-strip" aria-label="Wire subscriptions" data-testid="wire-subscriptions">
          <For each={wireSubscriptions().slice(0, 4)}>
            {(wire) => (
              <span title={`${wire.relayUrl} · ${wire.filterSummary}`}>
                {wire.state} · {wire.eventsRx} events · {wire.logicalConsumerCount} consumers
              </span>
            )}
          </For>
        </div>
      </Show>

      <Show when={routing()}>
        {(trace) => (
          <div class="routing-trace" data-testid="routing-trace">
            <strong>{trace().title}</strong>
            <span>{trace().detail}</span>
          </div>
        )}
      </Show>
      <Show when={refreshError()}>
        {(error) => (
          <p class="diagnostics-error" role="alert">
            {error()}
          </p>
        )}
      </Show>
    </section>
  );
}
