import { Match, Switch, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { publishNoteAction, type RuntimeCommand } from "./nmp/actions";
import { createNmpClient, type RuntimeSnapshot } from "./nmp/client";
import {
  chirpTimelineFromEnvelope,
  displayRows,
  featureSnapshotFromEnvelope,
  kernelSnapshotFromEnvelope,
} from "./nmp/snapshot";
import { ChatsPanel, GroupsPanel, SettingsPanel, WalletPanel } from "./features/FeaturePanels";
import { HomePanel } from "./features/HomePanel";
import { RuntimePanel } from "./features/RuntimePanel";
import { Sidebar, type AppTab } from "./features/Sidebar";

const client = createNmpClient();

export default function App() {
  const [snapshot, setSnapshot] = createSignal<RuntimeSnapshot>(client.snapshot());
  const [tab, setTab] = createSignal<AppTab>("home");
  const [starting, setStarting] = createSignal(false);
  const unsubscribe = client.subscribe(setSnapshot);
  const kernel = createMemo(() => kernelSnapshotFromEnvelope(snapshot().latestUpdate));
  const chirp = createMemo(() => chirpTimelineFromEnvelope(snapshot().latestUpdate));
  const feature = createMemo(() => {
    const snap = snapshot();
    const base = featureSnapshotFromEnvelope(snap.latestUpdate);
    // Tier-3 relay_statuses are decoded directly from the FlatBuffers
    // envelope (PR-B: the deprecated payload:Value is zeroed). Prefer them
    // over the empty relay_diagnostics the old Value-tree path would yield.
    const tierThreeRelays = snap.latestRelayStatuses;
    if (tierThreeRelays && tierThreeRelays.length > 0) {
      return { ...base, relayDiagnostics: tierThreeRelays };
    }
    return base;
  });
  const rows = createMemo(() => displayRows(kernel(), chirp()));

  onCleanup(unsubscribe);
  onMount(() => void start());

  const start = async () => {
    setStarting(true);
    // Allow the Playwright smoke test (and local dev) to inject a custom relay
    // via ?relay=ws://... query params. Multiple ?relay= values are collected.
    // Production deployments omit the param; the wasm uses its built-in defaults.
    const params = new URLSearchParams(window.location.search);
    const relays = params.getAll("relay").filter(Boolean);
    setSnapshot(await client.start(relays.length > 0 ? relays : undefined));
    setStarting(false);
  };
  const publish = async (content: string, replyToId: string | null) => {
    setSnapshot(await client.dispatchChirp(publishNoteAction(content, replyToId)));
  };
  const dispatch = async (command: RuntimeCommand) => {
    setSnapshot(await client.dispatchCommand(command));
  };

  return (
    <main class="app-shell">
      <Sidebar active={tab()} onSelect={setTab} feature={feature()} />
      <section class="workspace" aria-label="Chirp workspace">
        <Switch>
          <Match when={tab() === "home"}>
            <HomePanel rows={rows()} revision={kernel()?.rev} onPublish={publish} onCommand={dispatch} />
          </Match>
          <Match when={tab() === "chats"}>
            <ChatsPanel feature={feature()} onCommand={dispatch} />
          </Match>
          <Match when={tab() === "groups"}>
            <GroupsPanel feature={feature()} onCommand={dispatch} />
          </Match>
          <Match when={tab() === "wallet"}>
            <WalletPanel feature={feature()} onCommand={dispatch} />
          </Match>
          <Match when={tab() === "settings"}>
            <SettingsPanel feature={feature()} onCommand={dispatch} onStart={start} />
          </Match>
        </Switch>
      </section>
      <RuntimePanel snapshot={snapshot()} feature={feature()} starting={starting()} onStart={start} />
    </main>
  );
}
