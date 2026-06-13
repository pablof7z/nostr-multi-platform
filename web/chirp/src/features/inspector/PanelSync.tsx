import { For, Show } from "solid-js";
import type { DecodedRelayStatus } from "../../nmp/updateFrame";

/**
 * Sync / NIP-77 panel.
 *
 * What is available today: the `negentropyProbe` field from each RelayStatus,
 * which reflects the probe state string emitted by the kernel's negentropy
 * subsystem (e.g. "Idle", "Probing", "Done").
 *
 * What is NOT yet available: per-session negentropy stats (items sent, items
 * received, sync duration) — GAP-5. These live in a separate kernel data
 * structure not yet exposed through the FlatBuffers snapshot envelope. An
 * honest placeholder is rendered until GAP-5 lands.
 */
export function PanelSync(props: { relayStatuses: DecodedRelayStatus[] | undefined }) {
  const relaysWithProbe = (): DecodedRelayStatus[] =>
    (props.relayStatuses ?? []).filter((r) => r.negentropyProbe != null);

  return (
    <div class="ins-panel">
      <div class="ins-section-title">Negentropy probe state (per relay)</div>
      <Show when={relaysWithProbe().length > 0} fallback={
        <div class="ins-placeholder">No negentropy probe state reported yet.</div>
      }>
        <For each={relaysWithProbe()}>
          {(relay) => (
            <div class="ins-sub-row">
              <div class="ins-sub-header">
                <span class="ins-relay-url mono">{relay.url}</span>
              </div>
              <div class="ins-sub-meta">
                <span class="ins-chip ins-chip-muted mono">{relay.negentropyProbe}</span>
              </div>
            </div>
          )}
        </For>
      </Show>

      <div class="ins-section-title" style="margin-top:12px">Session stats</div>
      <div class="ins-placeholder ins-placeholder-gap">
        <strong>GAP-5:</strong> Per-session negentropy statistics (items offered, items
        needed, round-trip count, sync duration) are not yet exposed through the
        FlatBuffers snapshot envelope. They will appear here once the kernel adds
        the <code>negentropy_session_stats</code> Tier-3 field.
      </div>
    </div>
  );
}
