import { For, Show } from "solid-js";
import type { DecodedLogicalInterest, DecodedWireSub } from "../../nmp/updateFrame";
import { interestStateTone, wireSubStateTone } from "../../nmp/relayDiagnosticsTone";

function LogicalRow(props: { interest: DecodedLogicalInterest }) {
  const i = props.interest;
  // #1768 — derive the hue shell-side from the raw `state` token.
  const tone = interestStateTone(i.state);
  return (
    <div class="ins-sub-row">
      <div class="ins-sub-header">
        <span class={`ins-dot ins-dot-${tone}`} />
        <span class="ins-sub-key mono">{i.key || "—"}</span>
        <span class="ins-chip ins-chip-muted">×{i.refcount}</span>
      </div>
      <div class="ins-sub-meta">
        <span class={`ins-chip ins-chip-${tone}`}>{i.state || "unknown"}</span>
        <Show when={i.cacheCoverage}>
          <span class="ins-chip ins-chip-muted mono">cache:{i.cacheCoverage}</span>
        </Show>
      </div>
      <Show when={i.relayUrls.length > 0}>
        <div class="ins-sub-relays mono">{i.relayUrls.join(", ")}</div>
      </Show>
    </div>
  );
}

function WireRow(props: { sub: DecodedWireSub }) {
  const s = props.sub;
  // #1768 — derive the hue shell-side from the raw `state` token.
  const tone = wireSubStateTone(s.state);
  return (
    <div class="ins-sub-row">
      <div class="ins-sub-header">
        <span class={`ins-dot ins-dot-${tone}`} />
        <span class="ins-sub-key mono">{s.wireId || "—"}</span>
        <span class="ins-chip ins-chip-muted">logical×{s.logicalConsumerCount}</span>
      </div>
      <div class="ins-sub-meta">
        <span class={`ins-chip ins-chip-${tone}`}>{s.state || "unknown"}</span>
        <span class="ins-chip ins-chip-muted mono">ev:{s.eventsRx.toString()}</span>
      </div>
      <div class="ins-sub-relays mono">{s.relayUrl}</div>
      <Show when={s.filterSummary}>
        <div class="ins-sub-filter mono">{s.filterSummary}</div>
      </Show>
    </div>
  );
}

export function PanelSubs(props: {
  logicalInterests: DecodedLogicalInterest[];
  wireSubscriptions: DecodedWireSub[];
}) {
  return (
    <div class="ins-panel">
      <div class="ins-section-title">
        Logical interests ({props.logicalInterests.length})
      </div>
      <Show when={props.logicalInterests.length > 0} fallback={
        <div class="ins-placeholder">No logical interests reported yet.</div>
      }>
        <For each={props.logicalInterests}>
          {(interest) => <LogicalRow interest={interest} />}
        </For>
      </Show>

      <div class="ins-section-title" style="margin-top:12px">
        Wire subscriptions ({props.wireSubscriptions.length})
      </div>
      <Show when={props.wireSubscriptions.length > 0} fallback={
        <div class="ins-placeholder">No active wire subscriptions. The kernel opens REQ frames when logical interests are active and relays are connected.</div>
      }>
        <For each={props.wireSubscriptions}>
          {(sub) => <WireRow sub={sub} />}
        </For>
      </Show>
    </div>
  );
}
