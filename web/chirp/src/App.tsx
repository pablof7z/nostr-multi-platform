import { For, Show, createSignal, onCleanup, type JSX } from "solid-js";
import {
  Bell,
  CheckCircle2,
  Database,
  HardDrive,
  MessageSquare,
  Radio,
  RefreshCw,
  Send,
  Server,
  Settings,
  Signal,
  UserRound,
} from "lucide-solid";
import { createNmpClient, runtimeConnection, type RuntimeSnapshot } from "./nmp/client";
import { labelRuntimeStatus, protocolVersion, type WorkerEvent } from "./nmp/protocol";

const client = createNmpClient();

const initialFeed = [
  {
    author: "alice@nostr",
    text: "Chirp Web is now exercising the NMP worker contract.",
    time: "now",
  },
  {
    author: "relay monitor",
    text: "Live relay transport is the next runtime slice.",
    time: "next",
  },
];

export default function App() {
  const [snapshot, setSnapshot] = createSignal<RuntimeSnapshot>(client.snapshot());
  const [starting, setStarting] = createSignal(false);
  const [draft, setDraft] = createSignal("");
  const unsubscribe = client.subscribe(setSnapshot);

  onCleanup(unsubscribe);

  const start = async () => {
    setStarting(true);
    const next = await client.start();
    setSnapshot(next);
    setStarting(false);
  };

  const dispatch = async () => {
    const text = draft().trim();
    if (!text) {
      return;
    }
    setSnapshot(await client.dispatch("chirp.compose.submit", { text }));
  };

  const refresh = async () => {
    setSnapshot(await client.dispatch("chirp.timeline.refresh", {}));
  };

  return (
    <main class="app-shell">
      <Sidebar />
      <FeedPanel draft={draft()} onDraft={setDraft} onPublish={dispatch} onRefresh={refresh} />
      <ConnectionPanel snapshot={snapshot()} starting={starting()} onStart={start} />
    </main>
  );
}

function Sidebar() {
  return (
    <aside class="sidebar" aria-label="Chirp navigation">
      <div class="brand">
        <Radio size={24} />
        <span>Chirp</span>
      </div>
      <nav>
        <a class="active" href="#feed">
          <MessageSquare size={18} />
          Feed
        </a>
        <a href="#settings">
          <Bell size={18} />
          Mentions
        </a>
        <a href="#settings">
          <UserRound size={18} />
          Profile
        </a>
        <a href="#settings">
          <Settings size={18} />
          Settings
        </a>
      </nav>
    </aside>
  );
}

function FeedPanel(props: {
  draft: string;
  onDraft: (value: string) => void;
  onPublish: () => void;
  onRefresh: () => void;
}) {
  return (
    <section class="feed-panel" id="feed">
      <header class="topbar">
        <div>
          <p class="eyebrow">NMP web proof</p>
          <h1>Home timeline</h1>
        </div>
        <button
          class="icon-button"
          type="button"
          aria-label="Refresh timeline"
          title="Refresh timeline"
          onClick={props.onRefresh}
        >
          <RefreshCw size={18} />
        </button>
      </header>

      <div class="composer">
        <textarea
          value={props.draft}
          aria-label="Compose chirp"
          placeholder="What is happening on Nostr?"
          onInput={(event) => props.onDraft(event.currentTarget.value)}
        />
        <div class="composer-actions">
          <span>{props.draft.trim().length}/280</span>
          <button type="button" onClick={props.onPublish} disabled={props.draft.trim().length === 0}>
            <Send size={17} />
            Publish
          </button>
        </div>
      </div>

      <For each={initialFeed}>
        {(item) => (
          <article class="post">
            <div class="avatar">{item.author.slice(0, 1).toUpperCase()}</div>
            <div>
              <div class="post-meta">
                <strong>{item.author}</strong>
                <span>{item.time}</span>
              </div>
              <p>{item.text}</p>
            </div>
          </article>
        )}
      </For>
    </section>
  );
}

function ConnectionPanel(props: {
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
