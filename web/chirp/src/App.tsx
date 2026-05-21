import { For, Show, createSignal } from "solid-js";
import {
  Bell,
  CheckCircle2,
  MessageSquare,
  Radio,
  RefreshCw,
  Send,
  Settings,
  Signal,
  UserRound,
} from "lucide-solid";
import { createNmpClient, type RuntimeSnapshot } from "./nmp/client";
import { labelRuntimeStatus } from "./nmp/protocol";

const client = createNmpClient();

const relays = ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.primal.net"];

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

  const start = async () => {
    setStarting(true);
    const next = await client.start();
    setSnapshot(next);
    setStarting(false);
  };

  const dispatch = async () => {
    setSnapshot(await client.dispatch("chirp.compose.submit", { text: "hello web" }));
  };

  return (
    <main class="app-shell">
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
          <a href="#mentions">
            <Bell size={18} />
            Mentions
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

      <section class="feed-panel" id="feed">
        <header class="topbar">
          <div>
            <p class="eyebrow">NMP web proof</p>
            <h1>Home timeline</h1>
          </div>
          <button class="icon-button" type="button" aria-label="Refresh timeline" title="Refresh timeline">
            <RefreshCw size={18} />
          </button>
        </header>

        <div class="composer">
          <textarea value="hello web" aria-label="Compose chirp" />
          <button type="button" onClick={dispatch}>
            <Send size={17} />
            Publish
          </button>
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

      <aside class="inspector" aria-label="Runtime diagnostics">
        <section class="runtime-card">
          <div class="card-heading">
            <Signal size={19} />
            <h2>Runtime</h2>
          </div>
          <div class="status-row">
            <CheckCircle2 size={18} />
            <span>{labelRuntimeStatus(snapshot().status)}</span>
          </div>
          <button type="button" onClick={start} disabled={starting()}>
            Start worker
          </button>
        </section>

        <section class="runtime-card">
          <h2>Relays</h2>
          <For each={relays}>{(relay) => <div class="relay-row">{relay}</div>}</For>
        </section>

        <section class="runtime-card events">
          <h2>Events</h2>
          <Show when={snapshot().events.length > 0} fallback={<p>No runtime events yet.</p>}>
            <For each={snapshot().events}>
              {(event) => <pre>{JSON.stringify(event, null, 2)}</pre>}
            </For>
          </Show>
        </section>
      </aside>
    </main>
  );
}
