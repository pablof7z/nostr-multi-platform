import { For, Show } from "solid-js";
import type { DecodedRelayStatus } from "../../nmp/updateFrame";
import { authTone as deriveAuthTone, connectionTone as deriveConnectionTone } from "../../nmp/relayDiagnosticsTone";

/** Map kernel role string → abbreviated label. */
function roleLabel(role: string): string {
  const r = role.toLowerCase();
  if (r.includes("both")) return "both";
  if (r.includes("read")) return "read";
  if (r.includes("write")) return "write";
  if (r.includes("index")) return "idx";
  return role || "—";
}

function fmtBigint(n: bigint): string {
  return n === BigInt(0) ? "—" : n.toString();
}

function RelayRow(props: { relay: DecodedRelayStatus }) {
  const r = props.relay;
  // #1768 — derive the hue shell-side from the raw connection / auth tokens.
  const connTone = deriveConnectionTone(r.status);
  const authTone = deriveAuthTone(r.auth);
  return (
    <div
      class={`ins-relay-row${r.denied ? " ins-relay-denied" : ""}`}
      data-testid="relay-row"
    >
      <div class="ins-relay-header">
        <span class={`ins-dot ins-dot-${connTone}`} />
        <span class="ins-relay-url mono">{r.url || "—"}</span>
        <span class="ins-relay-role">{roleLabel(r.role)}</span>
      </div>
      <div class="ins-relay-meta">
        <span class={`ins-chip ins-chip-${connTone}`}>{r.status || "unknown"}</span>
        <Show when={r.auth}>
          <span class={`ins-chip ins-chip-${authTone}`}>auth:{r.auth}</span>
        </Show>
        <Show when={r.denied}>
          <span class="ins-chip ins-chip-error">denied</span>
        </Show>
      </div>
      <div class="ins-relay-stats">
        <span class="mono" data-testid="relay-bytes-tx">tx {fmtBigint(r.bytesTx)} B</span>
        <span class="mono" data-testid="relay-bytes-rx">rx {fmtBigint(r.bytesRx)} B</span>
        <span class="mono">ev {fmtBigint(r.eventsRx)}</span>
        <span class="mono">subs {fmtBigint(r.activeWireSubs)}</span>
        <Show when={r.reconnectCount > 0}>
          <span class="ins-chip ins-chip-warn mono">↺{r.reconnectCount}</span>
        </Show>
      </div>
      <Show when={r.negentropyProbe}>
        <div class="ins-relay-probe mono">neg: {r.negentropyProbe}</div>
      </Show>
      <Show when={r.lastError}>
        <div class="ins-relay-error mono">{r.lastError}</div>
      </Show>
    </div>
  );
}

export function PanelRelays(props: { relayStatuses: DecodedRelayStatus[] | undefined }) {
  return (
    <div class="ins-panel">
      <Show when={(props.relayStatuses?.length ?? 0) > 0} fallback={
        <div class="ins-placeholder">Waiting for relay data from kernel.</div>
      }>
        <For each={props.relayStatuses}>
          {(relay) => <RelayRow relay={relay} />}
        </For>
      </Show>
    </div>
  );
}
