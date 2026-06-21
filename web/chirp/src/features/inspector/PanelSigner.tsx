/**
 * Signer panel.
 *
 * The FlatBuffers snapshot does not yet carry per-signer capability flags,
 * key metadata, or publish-queue state. Those live in the `accounts` typed
 * projection (GAP-6). This panel renders the bridge kind (observable today)
 * and an honest placeholder for the rest.
 */
import type { RuntimeSnapshot } from "../../nmp/client";
import { protocolVersion } from "@nmp/runtime-web";

export function PanelSigner(props: { snapshot: RuntimeSnapshot }) {
  return (
    <div class="ins-panel">
      <div class="ins-section-title">Bridge</div>
      <div class="ins-row">
        <span class="ins-label">Kind</span>
        <span class="ins-value mono">
          {props.snapshot.clientRuntime === "in_process_fallback"
            ? "in-process fallback"
            : `worker v${protocolVersion}`}
        </span>
      </div>

      <div class="ins-section-title" style="margin-top:12px">NIP-07 Signer</div>
      <div class="ins-placeholder ins-placeholder-gap">
        <strong>GAP-6:</strong> Per-signer capability flags (which write actions
        are wired, key metadata, publish queue state) are not yet exposed through
        the FlatBuffers snapshot envelope. They will appear here once the
        <code>accounts</code> typed projection is wired into the browser snapshot
        builder.
      </div>
    </div>
  );
}
