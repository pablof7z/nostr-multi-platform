/**
 * Routing panel — honest placeholder for GAP-9.
 *
 * The kernel exposes recent routing decisions via a pull method
 * (`recent_routing_decisions()`), but this method is not yet wired into the
 * FlatBuffers snapshot envelope and no worker-protocol request type exists for
 * it. Until GAP-9 is implemented, this panel renders an explicit placeholder
 * rather than fabricating empty data or adding protocol plumbing that is out of
 * scope for this shell-layer feature.
 */
export function PanelRouting() {
  return (
    <div class="ins-panel">
      <div class="ins-section-title">Recent routing decisions</div>
      <div class="ins-placeholder ins-placeholder-gap">
        <strong>GAP-9:</strong> Routing push is not yet wired. The kernel has
        a <code>recent_routing_decisions()</code> method, but it is a pull-only
        wasm call that is not yet exposed via the worker protocol or the
        FlatBuffers snapshot envelope. Routing decisions will appear here once
        the snapshot builder includes the <code>routing_decisions</code> Tier-3
        field.
      </div>
    </div>
  );
}
