import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type {
  ActionResultRuntimeRow,
  ActionStageRuntimeRow,
  PublishOutboxRuntimeItem,
} from "../../nmp/runtimeProjection";

function shortHex(value: string): string {
  if (value.length <= 14) return value || "unknown";
  return `${value.slice(0, 8)}...${value.slice(-6)}`;
}

function previewContent(content: string): string {
  const trimmed = content.replace(/\s+/g, " ").trim();
  if (!trimmed) return "Empty event content";
  return trimmed.length > 120 ? `${trimmed.slice(0, 117)}...` : trimmed;
}

function compactUrl(url: string): string {
  return url.replace(/^wss?:\/\//, "");
}

type PublishRelayReceipt = {
  kind: "publish_relay_receipt";
  event_id?: string;
  relays: {
    relay_url: string;
    status: string;
    message?: string;
    relay_reason?: string;
  }[];
};

function isStringRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parseRelayReceipt(row: ActionResultRuntimeRow): PublishRelayReceipt | undefined {
  if (!row.result) return undefined;
  try {
    const parsed: unknown = JSON.parse(row.result);
    if (!isStringRecord(parsed) || parsed.kind !== "publish_relay_receipt") return undefined;
    const relays = Array.isArray(parsed.relays)
      ? parsed.relays.filter(isStringRecord).map((relay) => ({
          relay_url: typeof relay.relay_url === "string" ? relay.relay_url : "unknown relay",
          status: typeof relay.status === "string" ? relay.status : "unknown",
          message: typeof relay.message === "string" ? relay.message : undefined,
          relay_reason: typeof relay.relay_reason === "string" ? relay.relay_reason : undefined,
        }))
      : [];
    return {
      kind: "publish_relay_receipt",
      event_id: typeof parsed.event_id === "string" ? parsed.event_id : undefined,
      relays,
    };
  } catch {
    return undefined;
  }
}

function mergeResults(
  existing: ActionResultRuntimeRow[],
  incoming: readonly ActionResultRuntimeRow[],
): ActionResultRuntimeRow[] {
  const merged = [...incoming, ...existing];
  const seen = new Set<string>();
  return merged
    .filter((row) => {
      const key = `${row.correlationId}:${row.status}:${row.eventId ?? ""}:${row.error ?? ""}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .slice(0, 6);
}

function resultDetail(row: ActionResultRuntimeRow): string {
  if (row.error) return row.error;
  if (row.eventId) return `event ${shortHex(row.eventId)}`;
  if (row.result) return row.result;
  return `correlation ${shortHex(row.correlationId)}`;
}

export function OutboxDiagnostics(props: {
  outbox: readonly PublishOutboxRuntimeItem[];
  results: readonly ActionResultRuntimeRow[];
  stages: readonly ActionStageRuntimeRow[];
}) {
  const [recentResults, setRecentResults] = createSignal<ActionResultRuntimeRow[]>([]);
  createEffect(() => {
    if (props.results.length > 0) {
      setRecentResults((previous) => mergeResults(previous, props.results));
    }
  });

  const latestStages = createMemo(() =>
    [...props.stages].sort((left, right) => right.atMs - left.atMs).slice(0, 4),
  );

  return (
    <div class="outbox-diagnostics" data-testid="publish-outbox">
      <div class="outbox-diagnostics-header">
        <strong>Publish outbox</strong>
        <span>
          {props.outbox.length > 0
            ? `${props.outbox.length} in flight`
            : "No in-flight publishes"}
        </span>
      </div>

      <Show
        when={props.outbox.length > 0}
        fallback={<p class="diagnostics-empty">Publish a note to inspect per-relay delivery.</p>}
      >
        <For each={props.outbox.slice(0, 3)}>
          {(item) => (
            <article class="outbox-item" data-status={item.status}>
              <div class="outbox-item-main">
                <div>
                  <strong>{shortHex(item.eventId)}</strong>
                  <span>
                    kind {item.kind} · {item.status} · {item.targetRelays} relays
                  </span>
                </div>
                <span class="outbox-retry-state">{item.canRetry ? "retryable" : "active"}</span>
              </div>
              <p>{previewContent(item.content)}</p>
              <div class="outbox-relays">
                <For each={item.relays.slice(0, 5)}>
                  {(relay) => (
                    <span title={`${relay.relayUrl}${relay.message ? ` · ${relay.message}` : ""}`}>
                      {compactUrl(relay.relayUrl)} · {relay.status}
                      {relay.attempt > 0 ? ` #${relay.attempt}` : ""}
                    </span>
                  )}
                </For>
              </div>
            </article>
          )}
        </For>
      </Show>

      <Show when={latestStages().length > 0}>
        <div class="action-stage-strip" data-testid="action-stages">
          <For each={latestStages()}>
            {(stage) => (
              <span title={stage.detail ?? stage.reason ?? stage.correlationId}>
                {stage.stage} · {shortHex(stage.correlationId)}
              </span>
            )}
          </For>
        </div>
      </Show>

      <Show when={recentResults().length > 0}>
        <div class="action-results" data-testid="action-results">
          <For each={recentResults()}>
            {(row) => (
              <div class="action-result-row" data-status={row.status}>
                <div class="action-result-summary">
                  <strong>{row.status}</strong>
                  <span>{resultDetail(row)}</span>
                </div>
                <Show when={parseRelayReceipt(row)}>
                  {(receipt) => (
                    <div class="action-result-relays" data-testid="relay-verdicts">
                      <For each={receipt().relays}>
                        {(relay) => (
                          <span
                            data-status={relay.status}
                            title={`${relay.relay_url}${relay.message ? ` · ${relay.message}` : ""}${
                              relay.relay_reason ? ` · ${relay.relay_reason}` : ""
                            }`}
                          >
                            {compactUrl(relay.relay_url)} ·{" "}
                            {relay.status === "ok" ? "accepted" : relay.status}
                          </span>
                        )}
                      </For>
                    </div>
                  )}
                </Show>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
