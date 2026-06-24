import { createMemo, For, Show } from "solid-js";
import "./PanelRouting.css";

type RoutingLaneSource = {
  kind?: string;
  direction?: string;
  category?: string;
  class?: unknown;
  via?: string;
  mode?: string;
};

type RoutingAttempt = {
  lane?: string;
  outcome?: {
    kind?: string;
    count?: number;
  };
};

type RoutedUrl = {
  url?: string;
  lanes?: RoutingLaneSource[];
};

type PublishDecision = {
  at_ms?: number;
  kind?: number;
  author?: string;
  event_id_short?: string;
  lane_attempts?: RoutingAttempt[];
  urls?: RoutedUrl[];
};

type SubscriptionDecision = {
  at_ms?: number;
  interest_id?: string;
  kinds?: number[];
  authors_count?: number;
  lane_attempts?: RoutingAttempt[];
  urls?: RoutedUrl[];
};

type RoutingDecisions = {
  schema_version?: number;
  capacity?: number;
  publishes?: PublishDecision[];
  subscriptions?: SubscriptionDecision[];
};

function parseDecisions(json: string | undefined): { value?: RoutingDecisions; error?: string } {
  if (!json) return {};
  try {
    const parsed = JSON.parse(json) as RoutingDecisions;
    return { value: parsed };
  } catch {
    return { error: "Routing diagnostics JSON did not parse." };
  }
}

function fmtMs(at: number | undefined): string {
  if (typeof at !== "number" || at <= 0) return "—";
  return `${at}ms`;
}

function laneLabel(lane: RoutingLaneSource): string {
  switch (lane.kind) {
    case "Nip65":
      return `NIP-65:${lane.direction ?? "?"}`;
    case "UserConfigured":
      return `User:${lane.category ?? "?"}`;
    case "ClassRouted":
      return `Class:${lane.via ?? "?"}`;
    case "AppRelay":
      return `App:${lane.mode ?? "?"}`;
    case undefined:
      return "unknown";
    default:
      return lane.kind;
  }
}

function attemptLabel(attempt: RoutingAttempt): string {
  const lane = attempt.lane ?? "unknown";
  const outcome = attempt.outcome;
  if (outcome?.kind === "Matched") {
    return `${lane}:${outcome.count ?? 0}`;
  }
  return `${lane}:empty`;
}

function UrlList(props: { urls: RoutedUrl[] | undefined }) {
  return (
    <Show when={(props.urls?.length ?? 0) > 0} fallback={<span class="ins-muted">no relays</span>}>
      <div class="ins-routing-url-list">
        <For each={props.urls}>
          {(row) => (
            <div class="ins-routing-url">
              <span class="mono ins-routing-url-text">{row.url ?? "—"}</span>
              <div class="ins-routing-lanes">
                <For each={row.lanes ?? []}>
                  {(lane) => <span class="ins-chip ins-chip-muted">{laneLabel(lane)}</span>}
                </For>
              </div>
            </div>
          )}
        </For>
      </div>
    </Show>
  );
}

function AttemptList(props: { attempts: RoutingAttempt[] | undefined }) {
  return (
    <Show when={(props.attempts?.length ?? 0) > 0}>
      <div class="ins-routing-attempts">
        <For each={props.attempts ?? []}>
          {(attempt) => <span class="ins-chip ins-chip-muted mono">{attemptLabel(attempt)}</span>}
        </For>
      </div>
    </Show>
  );
}

function PublishRow(props: { decision: PublishDecision }) {
  return (
    <div class="ins-routing-row" data-testid="routing-publish-row">
      <div class="ins-routing-head">
        <span class="mono">kind {props.decision.kind ?? "—"}</span>
        <span class="mono">{props.decision.event_id_short ?? "—"}</span>
        <span class="mono">{fmtMs(props.decision.at_ms)}</span>
      </div>
      <Show when={props.decision.author}>
        <div class="ins-routing-sub mono">{props.decision.author}</div>
      </Show>
      <AttemptList attempts={props.decision.lane_attempts} />
      <UrlList urls={props.decision.urls} />
    </div>
  );
}

function SubscriptionRow(props: { decision: SubscriptionDecision }) {
  return (
    <div class="ins-routing-row" data-testid="routing-subscription-row">
      <div class="ins-routing-head">
        <span class="mono">{props.decision.interest_id ?? "—"}</span>
        <span class="mono">kinds {(props.decision.kinds ?? []).join(",") || "—"}</span>
        <span class="mono">authors {props.decision.authors_count ?? 0}</span>
      </div>
      <div class="ins-routing-sub mono">{fmtMs(props.decision.at_ms)}</div>
      <AttemptList attempts={props.decision.lane_attempts} />
      <UrlList urls={props.decision.urls} />
    </div>
  );
}

export function PanelRouting(props: {
  decisionsJson: string | undefined;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  const parsed = createMemo(() => parseDecisions(props.decisionsJson));
  const decisions = () => parsed().value;
  const publishes = () => decisions()?.publishes ?? [];
  const subscriptions = () => decisions()?.subscriptions ?? [];

  return (
    <div class="ins-panel">
      <div class="ins-panel-toolbar">
        <div>
          <div class="ins-section-title">Recent routing decisions</div>
          <Show when={decisions()}>
            <div class="ins-routing-meta mono">
              schema {decisions()?.schema_version ?? "—"} · cap {decisions()?.capacity ?? "—"}
            </div>
          </Show>
        </div>
        <button
          class="ins-mini-btn"
          type="button"
          onClick={props.onRefresh}
          disabled={props.refreshing}
          data-testid="routing-refresh"
        >
          {props.refreshing ? "refreshing" : "refresh"}
        </button>
      </div>

      <Show when={!parsed().error} fallback={<div class="ins-placeholder">{parsed().error}</div>}>
        <Show when={decisions()} fallback={<div class="ins-placeholder">No routing snapshot loaded.</div>}>
          <div class="ins-section-title" style="margin-top:12px">Publishes</div>
          <Show when={publishes().length > 0} fallback={<div class="ins-placeholder">No publish decisions recorded.</div>}>
            <For each={publishes()}>{(decision) => <PublishRow decision={decision} />}</For>
          </Show>

          <div class="ins-section-title" style="margin-top:12px">Subscriptions</div>
          <Show when={subscriptions().length > 0} fallback={<div class="ins-placeholder">No subscription decisions recorded.</div>}>
            <For each={subscriptions()}>{(decision) => <SubscriptionRow decision={decision} />}</For>
          </Show>
        </Show>
      </Show>
    </div>
  );
}
