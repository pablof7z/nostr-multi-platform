import { For, Show, type JSX } from "solid-js";
import { CheckCircle2, Database, HardDrive, Radio, RefreshCw, Settings, Signal } from "lucide-solid";
import { runtimeConnection, type RuntimeSnapshot } from "../nmp/client";
import { labelRuntimeStatus, protocolVersion, type WorkerEventSummary } from "@nmp/runtime-web";
import type { FeatureSnapshot } from "../nmp/snapshot";

export function RuntimePanel(props: {
  snapshot: RuntimeSnapshot;
  feature: FeatureSnapshot;
  starting: boolean;
  onStart: () => void;
}) {
  return (
    <aside class="inspector" aria-label="Runtime inspector">
      {/* PR-W3 smoke-test observable: present iff the wasm emitted at least one
          binary snapshot frame (update_bytes). DegradedRuntime NEVER emits
          update_bytes, so this element's presence proves real wasm is running.
          visually-hidden but accessible — consumed by Playwright assertions. */}
      <Show when={props.snapshot.latestUpdateBytes !== undefined}>
        <span
          data-testid="nmp-has-snapshot"
          style="position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap"
        >has-snapshot</span>
      </Show>
      <section class="runtime-card connection-card">
        <div class="card-heading"><Settings size={19} /><h2>Connection</h2></div>
        {/* nmp-runtime-status testid: asserted by boot.spec.ts post-#1209
            (TS bindings regenerated, decode reads Tier-3 running field). */}
        <StatusLine icon={<Signal size={17} />} label="Runtime" value={labelRuntimeStatus(props.snapshot.status)} testId="nmp-runtime-status" />
        <StatusLine icon={<Database size={17} />} label="Database" value={runtimeConnection.databaseName} />
        <StatusLine
          icon={<HardDrive size={17} />}
          label="Bridge"
          value={props.snapshot.clientRuntime === "in_process_fallback" ? "in-process fallback" : `worker v${protocolVersion}`}
          testId="nmp-bridge-kind"
        />
        <button type="button" onClick={props.onStart} disabled={props.starting}>
          {props.starting ? <RefreshCw size={18} /> : <CheckCircle2 size={18} />}
          {props.starting ? "Starting" : "Start worker"}
        </button>
      </section>
      <section class="runtime-card">
        <div class="card-heading"><Radio size={18} /><h2>Relays</h2></div>
        <Show when={props.feature.relayDiagnostics.length > 0} fallback={<p>Waiting for relay data.</p>}>
          <For each={props.feature.relayDiagnostics}>
            {(relay) => <div class="relay-row" data-testid="relay-row"><span>{relay.url}</span><small>{relay.role} · {relay.status}</small></div>}
          </For>
        </Show>
      </section>
      <Show when={props.feature.authorProfile}>
        {(profile) => (
          <section class="runtime-card">
            <h2>Profile</h2>
            <strong>{profile().display || profile().pubkey}</strong>
            <p>{profile().about}</p>
            <small>{profile().noteCount} {profile().actionLabel}</small>
          </section>
        )}
      </Show>
      <Show when={props.feature.thread}>
        {(thread) => (
          <section class="runtime-card">
            <h2>Thread</h2>
            <p>{thread().state || thread().focusedEventId}</p>
            <small>{thread().itemCount} items · {thread().previousLabel} {thread().nextLabel}</small>
          </section>
        )}
      </Show>
      <EventLog events={props.snapshot.events} />
    </aside>
  );
}

function StatusLine(props: { icon: JSX.Element; label: string; value: string; testId?: string }) {
  return <div class="status-line"><span class="status-icon">{props.icon}</span><span>{props.label}</span><strong data-testid={props.testId}>{props.value}</strong></div>;
}

function EventLog(props: { events: WorkerEventSummary[] }) {
  return (
    <section class="runtime-card events">
      <h2>Runtime events</h2>
      <Show when={props.events.length > 0} fallback={<p>No runtime events yet.</p>}>
        <For each={props.events}>{(event) => <pre>{JSON.stringify(event, null, 2)}</pre>}</For>
      </Show>
    </section>
  );
}
