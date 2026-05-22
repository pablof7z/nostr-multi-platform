import { For, Show, type JSX } from "solid-js";
import { CheckCircle2, Database, HardDrive, Radio, RefreshCw, Settings, Signal } from "lucide-solid";
import { runtimeConnection, type RuntimeSnapshot } from "../nmp/client";
import { labelRuntimeStatus, protocolVersion, type WorkerEvent } from "../nmp/protocol";
import type { FeatureSnapshot } from "../nmp/snapshot";

export function RuntimePanel(props: {
  snapshot: RuntimeSnapshot;
  feature: FeatureSnapshot;
  starting: boolean;
  onStart: () => void;
}) {
  return (
    <aside class="inspector" aria-label="Runtime inspector">
      <section class="runtime-card connection-card">
        <div class="card-heading"><Settings size={19} /><h2>Connection</h2></div>
        <StatusLine icon={<Signal size={17} />} label="Runtime" value={labelRuntimeStatus(props.snapshot.status)} />
        <StatusLine icon={<Database size={17} />} label="Database" value={runtimeConnection.databaseName} />
        <StatusLine icon={<HardDrive size={17} />} label="Protocol" value={`worker v${protocolVersion}`} />
        <button type="button" onClick={props.onStart} disabled={props.starting}>
          {props.starting ? <RefreshCw size={18} /> : <CheckCircle2 size={18} />}
          {props.starting ? "Starting" : "Start worker"}
        </button>
      </section>
      <section class="runtime-card">
        <div class="card-heading"><Radio size={18} /><h2>Relays</h2></div>
        <Show when={props.feature.relayDiagnostics.length > 0} fallback={<p>Waiting for Rust relay diagnostics.</p>}>
          <For each={props.feature.relayDiagnostics}>
            {(relay) => <div class="relay-row"><span>{relay.url}</span><small>{relay.role} · {relay.status}</small></div>}
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

function StatusLine(props: { icon: JSX.Element; label: string; value: string }) {
  return <div class="status-line"><span class="status-icon">{props.icon}</span><span>{props.label}</span><strong>{props.value}</strong></div>;
}

function EventLog(props: { events: WorkerEvent[] }) {
  return (
    <section class="runtime-card events">
      <h2>Runtime events</h2>
      <Show when={props.events.length > 0} fallback={<p>No runtime events yet.</p>}>
        <For each={props.events}>{(event) => <pre>{JSON.stringify(event, null, 2)}</pre>}</For>
      </Show>
    </section>
  );
}
