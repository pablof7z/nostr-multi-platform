import { For, Show, type JSX } from "solid-js";
import { CheckCircle2, Database, HardDrive, Server, Settings, Signal } from "lucide-solid";
import { capabilitySurfaces } from "../chirp/model";
import { runtimeConnection, type RuntimeSnapshot } from "../nmp/client";
import { labelRuntimeStatus, protocolVersion, type WorkerEvent } from "../nmp/protocol";

export function Inspector(props: {
  snapshot: RuntimeSnapshot;
  starting: boolean;
  onStart: () => void;
}) {
  return (
    <aside class="inspector" id="settings" aria-label="Connection settings">
      <section class="runtime-card connection-card">
        <div class="card-heading">
          <Settings size={19} />
          <h2>Connection</h2>
        </div>
        <StatusLine icon={<Signal size={17} />} label="Runtime" value={labelRuntimeStatus(props.snapshot.status)} />
        <StatusLine icon={<Database size={17} />} label="Database" value={runtimeConnection.databaseName} />
        <StatusLine icon={<HardDrive size={17} />} label="Protocol" value={`worker v${protocolVersion}`} />
        <button type="button" onClick={props.onStart} disabled={props.starting}>
          <CheckCircle2 size={18} />
          {props.starting ? "Starting" : "Start worker"}
        </button>
      </section>
      <section class="runtime-card">
        <div class="card-heading">
          <Server size={18} />
          <h2>Relays</h2>
        </div>
        <For each={runtimeConnection.relays}>
          {(relay) => (
            <div class="relay-row">
              <span>{relay}</span>
              <small>configured</small>
            </div>
          )}
        </For>
      </section>
      <section class="runtime-card">
        <h2>Capability surfaces</h2>
        <For each={capabilitySurfaces}>
          {(surface) => (
            <div class="capability-row">
              <surface.Icon size={16} />
              <span>{surface.label}</span>
            </div>
          )}
        </For>
      </section>
      <EventLog events={props.snapshot.events} />
    </aside>
  );
}

function StatusLine(props: { icon: JSX.Element; label: string; value: string }) {
  return (
    <div class="status-line">
      <span class="status-icon">{props.icon}</span>
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  );
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
