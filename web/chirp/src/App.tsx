import { For, Show, createMemo, createSignal, onCleanup, type JSX } from "solid-js";
import {
  CheckCircle2,
  Database,
  HardDrive,
  MessageSquare,
  Radio,
  RefreshCw,
  Send,
  Settings,
  Signal,
  UserRound,
} from "lucide-solid";
import { publishNoteAction } from "./nmp/actions";
import { createNmpClient, runtimeConnection, type RuntimeSnapshot } from "./nmp/client";
import { labelRuntimeStatus, protocolVersion, type WorkerEvent } from "./nmp/protocol";
import {
  chirpTimelineFromEnvelope,
  displayRows,
  kernelSnapshotFromEnvelope,
  type TimelineItem,
} from "./nmp/snapshot";

const client = createNmpClient();

export default function App() {
  const [snapshot, setSnapshot] = createSignal<RuntimeSnapshot>(client.snapshot());
  const [starting, setStarting] = createSignal(false);
  const [draft, setDraft] = createSignal("");
  const unsubscribe = client.subscribe(setSnapshot);
  const kernel = createMemo(() => kernelSnapshotFromEnvelope(snapshot().latestUpdate));
  const chirp = createMemo(() => chirpTimelineFromEnvelope(snapshot().latestUpdate));
  const rows = createMemo(() => displayRows(kernel(), chirp()));

  onCleanup(unsubscribe);

  const start = async () => {
    setStarting(true);
    setSnapshot(await client.start());
    setStarting(false);
  };

  const publish = async () => {
    setSnapshot(await client.dispatchChirp(publishNoteAction(draft())));
    setDraft("");
  };

  return (
    <main class="app-shell">
      <Sidebar />
      <FeedPanel
        draft={draft()}
        rows={rows()}
        revision={kernel()?.rev}
        onDraft={setDraft}
        onPublish={publish}
        onStart={start}
      />
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
        <a href="#profile">
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
  rows: TimelineItem[];
  revision?: number;
  onDraft: (value: string) => void;
  onPublish: () => void;
  onStart: () => void;
}) {
  return (
    <section class="feed-panel" id="feed">
      <header class="topbar">
        <div>
          <p class="eyebrow">NMP snapshot {props.revision === undefined ? "pending" : `rev ${props.revision}`}</p>
          <h1>Home timeline</h1>
        </div>
        <button class="icon-button" type="button" aria-label="Start runtime" title="Start runtime" onClick={props.onStart}>
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
          <button type="button" onClick={props.onPublish}>
            <Send size={17} />
            Publish
          </button>
        </div>
      </div>

      <Show when={props.rows.length > 0} fallback={<EmptyTimeline />}>
        <For each={props.rows}>{(item) => <Post item={item} />}</For>
      </Show>
    </section>
  );
}

function EmptyTimeline() {
  return (
    <div class="empty-state">
      <MessageSquare size={22} />
      <p>No Rust snapshot has produced timeline rows yet.</p>
    </div>
  );
}

function Post(props: { item: TimelineItem }) {
  const author = () => props.item.displayName ?? props.item.handle ?? shortKey(props.item.authorPubkey ?? props.item.pubkey);
  return (
    <article class="post">
      <div class="avatar">{author().slice(0, 1).toUpperCase()}</div>
      <div>
        <div class="post-meta">
          <strong>{author()}</strong>
          <span>{props.item.relativeTime ?? labelTime(props.item.createdAt)}</span>
        </div>
        <p>{props.item.content ?? ""}</p>
      </div>
    </article>
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
        <StatusLine icon={<Radio size={17} />} label="Relays" value="Rust defaults" />
        <StatusLine icon={<HardDrive size={17} />} label="Protocol" value={`worker v${protocolVersion}`} />
        <button type="button" onClick={props.onStart} disabled={props.starting}>
          <CheckCircle2 size={18} />
          {props.starting ? "Starting" : "Start worker"}
        </button>
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

function shortKey(value?: string): string {
  if (!value) {
    return "unknown";
  }
  return value.length > 12 ? `${value.slice(0, 8)}..${value.slice(-4)}` : value;
}

function labelTime(epochSeconds?: number): string {
  if (!epochSeconds) {
    return "";
  }
  return new Date(epochSeconds * 1000).toLocaleString();
}
