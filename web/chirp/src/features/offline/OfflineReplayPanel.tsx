import { For, Show } from "solid-js";
import type {
  InterestRuntimeRow,
  PublishOutboxRuntimeItem,
  RuntimeProjection,
  WireSubscriptionRuntimeRow,
} from "../../nmp/runtimeProjection";
import "./offline.css";

function compactUrl(url: string): string {
  return url.replace(/^wss?:\/\//, "");
}

function pendingPublishLabel(items: readonly PublishOutboxRuntimeItem[]): string {
  if (items.length === 0) return "No pending publishes";
  if (items.length === 1) return "1 pending publish";
  return `${items.length} pending publishes`;
}

function storageLabel(diagnostics?: RuntimeProjection): string {
  if (!diagnostics) return "Waiting for runtime snapshot";
  if (diagnostics.storeOpenFailure) return "Storage degraded";
  return "Store available";
}

function replayCoverage(interests: readonly InterestRuntimeRow[]): string {
  const covered = interests.filter((interest) => interest.cacheCoverage).length;
  if (interests.length === 0) return "No active interests";
  return `${covered}/${interests.length} report cache coverage`;
}

function eoseCoverage(wires: readonly WireSubscriptionRuntimeRow[]): string {
  const eose = wires.filter((wire) => wire.eoseObserved).length;
  if (wires.length === 0) return "No wire subscriptions";
  return `${eose}/${wires.length} reached EOSE`;
}

function relayCoverage(diagnostics?: RuntimeProjection): string {
  const relays = diagnostics?.relays ?? [];
  if (relays.length === 0) return "No relays visible";
  const connected = relays.filter((relay) => relay.connection === "connected").length;
  return `${connected}/${relays.length} relays connected`;
}

function interestTitle(interest: InterestRuntimeRow): string {
  return [
    interest.key,
    interest.state,
    `${interest.relayCount} relays`,
    interest.cacheCoverage,
  ]
    .filter(Boolean)
    .join(" · ");
}

function wireTitle(wire: WireSubscriptionRuntimeRow): string {
  return [
    wire.wireId,
    wire.relayUrl,
    wire.state,
    wire.filterSummary,
  ].join(" · ");
}

export function OfflineReplayPanel(props: { diagnostics?: RuntimeProjection }) {
  const relays = () => props.diagnostics?.relays ?? [];
  const interests = () => props.diagnostics?.interests ?? [];
  const wires = () => props.diagnostics?.wireSubscriptions ?? [];
  const outbox = () => props.diagnostics?.publishOutbox ?? [];

  return (
    <section class="offline-panel" aria-label="Storage and replay" data-testid="offline-panel">
      <div class="offline-header">
        <div>
          <p class="panel-kicker">Storage</p>
          <h2>Replay health</h2>
          <p>
            This surface renders runtime diagnostics only. Durable offline
            publish replay remains blocked until Rust owns the persisted queue.
          </p>
        </div>
        <span
          class="offline-state"
          data-state={props.diagnostics?.storeOpenFailure ? "degraded" : "ready"}
          data-testid="offline-storage-state"
        >
          {storageLabel(props.diagnostics)}
        </span>
      </div>

      <div class="offline-proof-grid" aria-label="Replay proof">
        <div class="offline-proof">
          <span>Relays</span>
          <strong>{relayCoverage(props.diagnostics)}</strong>
        </div>
        <div class="offline-proof">
          <span>Interests</span>
          <strong>{replayCoverage(interests())}</strong>
        </div>
        <div class="offline-proof">
          <span>Subscriptions</span>
          <strong>{eoseCoverage(wires())}</strong>
        </div>
        <div class="offline-proof">
          <span>Outbox</span>
          <strong>{pendingPublishLabel(outbox())}</strong>
        </div>
      </div>

      <Show when={props.diagnostics?.storeOpenFailure}>
        {(failure) => (
          <p class="offline-warning" role="alert" data-testid="offline-store-failure">
            {failure()}
          </p>
        )}
      </Show>

      <div class="offline-section">
        <div class="offline-section-header">
          <strong>Active replay interests</strong>
          <span>{interests().length}</span>
        </div>
        <Show
          when={interests().length > 0}
          fallback={<p class="offline-empty">No runtime interests are active yet.</p>}
        >
          <div class="offline-list" data-testid="offline-interests">
            <For each={interests().slice(0, 8)}>
              {(interest) => (
                <article class="offline-row" title={interestTitle(interest)}>
                  <strong>{interest.key}</strong>
                  <span>
                    {interest.state} · {interest.relayCount} relays
                    {interest.cacheCoverage ? ` · ${interest.cacheCoverage}` : ""}
                  </span>
                </article>
              )}
            </For>
          </div>
        </Show>
      </div>

      <div class="offline-section">
        <div class="offline-section-header">
          <strong>Wire subscriptions</strong>
          <span>{wires().length}</span>
        </div>
        <Show
          when={wires().length > 0}
          fallback={<p class="offline-empty">No relay subscriptions are open.</p>}
        >
          <div class="offline-list" data-testid="offline-wires">
            <For each={wires().slice(0, 8)}>
              {(wire) => (
                <article class="offline-row" title={wireTitle(wire)}>
                  <strong>{compactUrl(wire.relayUrl)}</strong>
                  <span>
                    {wire.state} · {wire.eventsRx} events ·{" "}
                    {wire.eoseObserved ? "EOSE" : "waiting for EOSE"}
                  </span>
                  <small>{wire.filterSummary}</small>
                </article>
              )}
            </For>
          </div>
        </Show>
      </div>

      <div class="offline-section">
        <div class="offline-section-header">
          <strong>Relay inventory</strong>
          <span>{relays().length}</span>
        </div>
        <Show
          when={relays().length > 0}
          fallback={<p class="offline-empty">No relay inventory is visible yet.</p>}
        >
          <div class="offline-list" data-testid="offline-relays">
            <For each={relays().slice(0, 8)}>
              {(relay) => (
                <article class="offline-row" data-state={relay.connection}>
                  <strong title={relay.url}>{compactUrl(relay.url)}</strong>
                  <span>
                    {relay.role} · {relay.connection} · {relay.eventsRx} events
                  </span>
                  <Show when={relay.lastError || relay.lastNotice}>
                    <small>{relay.lastError ?? relay.lastNotice}</small>
                  </Show>
                </article>
              )}
            </For>
          </div>
        </Show>
      </div>
    </section>
  );
}
