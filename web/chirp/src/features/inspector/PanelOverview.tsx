import { Show } from "solid-js";
import { labelRuntimeStatus, protocolVersion, type WorkerEvent } from "@nmp/runtime-web";
import { runtimeConnection, type RuntimeSnapshot } from "../../nmp/client";
import { type DecodedSnapshot } from "../../nmp/inspectorSnapshot";

function fmtBytes(n: bigint): string {
  const num = Number(n);
  if (num < 1024) return `${num} B`;
  if (num < 1024 * 1024) return `${(num / 1024).toFixed(1)} KB`;
  return `${(num / (1024 * 1024)).toFixed(1)} MB`;
}

function fmtUs(us: bigint): string {
  const n = Number(us);
  if (n < 1000) return `${n} µs`;
  return `${(n / 1000).toFixed(1)} ms`;
}

function Row(props: { label: string; value: string; testId?: string }) {
  return (
    <div class="ins-row">
      <span class="ins-label">{props.label}</span>
      <span class="ins-value mono" data-testid={props.testId}>{props.value}</span>
    </div>
  );
}

export function PanelOverview(props: {
  snapshot: RuntimeSnapshot;
  decodedSnapshot: DecodedSnapshot | undefined;
}) {
  const ds = (): DecodedSnapshot | undefined => props.decodedSnapshot;

  return (
    <div class="ins-panel">
      <div class="ins-section-title">Kernel</div>
      <Row label="Status" value={labelRuntimeStatus(props.snapshot.status)} />
      <Row label="Bridge" value={props.snapshot.clientRuntime === "in_process_fallback" ? "in-process fallback" : `worker v${protocolVersion}`} />
      <Row label="Database" value={runtimeConnection.databaseName} />
      <Show when={ds()}>
        {(snap) => <>
          <Row label="Rev" value={snap().rev.toString()} />
          <Row label="Schema" value={snap().lastTickMs === BigInt(0) ? "not yet reported" : `last tick ${snap().lastTickMs.toString()} ms`} />
        </>}
      </Show>
      <Show when={ds()?.storeOpenFailure}>
        {(err) => <div class="ins-alert ins-alert-error">Store open failure: {err()}</div>}
      </Show>
      <Show when={ds()?.lastErrorToast}>
        {(msg) => <div class="ins-alert ins-alert-warn">Last error: {msg()}</div>}
      </Show>

      <Show when={ds()?.metrics}>
        {(m) => <>
          <div class="ins-section-title" style="margin-top:10px">Store</div>
          <Row label="Stored events" value={m().storedEvents.toString()} testId="inspector-stored-events" />
          <Row label="Visible items" value={m().visibleItems.toString()} />
          <Row label="Est. store size" value={fmtBytes(m().estimatedStoreBytes)} />
          <Row label="Open views" value={m().openViews.toString()} />
          <div class="ins-section-title" style="margin-top:10px">Network</div>
          <Row label="Events rx" value={m().eventsRx.toString()} testId="inspector-events-rx" />
          <Row label="EOSE rx" value={m().eoseRx.toString()} />
          <Row label="Bytes rx" value={fmtBytes(m().bytesRx)} />
          <Row label="Bytes tx" value={fmtBytes(m().bytesTx)} />
          <div class="ins-section-title" style="margin-top:10px">Performance</div>
          <Row label="Update seq" value={m().updateSequence.toString()} testId="inspector-update-seq" />
          <Row label="make_update" value={fmtUs(m().makeUpdateUs)} />
          <Row label="Actor queue" value={m().actorQueueDepth.toString()} />
        </>}
      </Show>
      <Show when={!ds()?.metrics}>
        <div class="ins-placeholder">Metrics not yet reported by the browser runtime.</div>
      </Show>
    </div>
  );
}

export function PanelFrames(props: { events: WorkerEvent[] }) {
  return (
    <div class="ins-panel">
      <div class="ins-section-title">Worker events (last {props.events.length})</div>
      <Show when={props.events.length > 0} fallback={<div class="ins-placeholder">No events yet.</div>}>
        <div class="ins-frames-list">
          {props.events.map((ev) => (
            <pre class="ins-frame-pre">{JSON.stringify(ev, (_k, v) => typeof v === "bigint" ? v.toString() : v, 2)}</pre>
          ))}
        </div>
      </Show>
    </div>
  );
}
