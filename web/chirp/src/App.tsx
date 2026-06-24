import { Match, Switch, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import {
  publishNoteAction,
  releaseEventCommand,
  releaseProfileCommand,
  resolveEventCommand,
  resolveProfileCommand,
  type RuntimeCommand,
} from "./nmp/actions";
import type { NostrEventHost } from "./components/content-event/NostrEventHost";
import type { NostrProfileHost } from "./components/user-avatar/NostrProfileHost";
import type { ProfileWire } from "./components/user-avatar/ProfileWire";
import { createNmpClient, type RuntimeSnapshot } from "./nmp/client";
import type { ClaimedEventWire } from "./nmp/refEventStore";
import {
  featureSnapshotFromEnvelope,
  feedItemsToRows,
} from "./nmp/snapshot";
import { ChatsPanel, GroupsPanel, SettingsPanel, WalletPanel } from "./features/FeaturePanels";
import { HomePanel } from "./features/HomePanel";
import { NmpInspector } from "./features/inspector/Inspector";
import { Sidebar, type AppTab } from "./features/Sidebar";

// NIP-07 browser extension interface (window.nostr — EIP-1193-style extension).
declare global {
  interface Window {
    nostr?: {
      getPublicKey(): Promise<string>;
      signEvent(event: Record<string, unknown>): Promise<Record<string, unknown>>;
    };
  }
}

const client = createNmpClient();

export default function App() {
  const [snapshot, setSnapshot] = createSignal<RuntimeSnapshot>(client.snapshot());
  const [tab, setTab] = createSignal<AppTab>("home");
  const [starting, setStarting] = createSignal(false);
  const [routingRefreshing, setRoutingRefreshing] = createSignal(false);
  const [signerConnected, setSignerConnected] = createSignal(false);

  const unsubscribe = client.subscribe(setSnapshot);

  const feature = createMemo(() => {
    const snap = snapshot();
    // featureSnapshotFromEnvelope reads the JSON projection path (latestUpdate,
    // which is zeroed in production since PR-B). Feed content now comes from
    // feedItems (the nmp.feed.home typed projection). Relay diagnostics still
    // use the Tier-3 relay_statuses field directly.
    const base = featureSnapshotFromEnvelope(undefined);
    const tierThreeRelays = snap.latestRelayStatuses;
    if (tierThreeRelays && tierThreeRelays.length > 0) {
      return { ...base, relayDiagnostics: tierThreeRelays };
    }
    return base;
  });

  // Real home feed: decoded nmp.feed.home typed projection from each snapshot.
  // Keep-last-good: feedItems is undefined until the first projection arrives;
  // feedItemsToRows returns [] for undefined/empty — honest empty state.
  // The `refs.profile` map provides the presentation-layer join for author
  // display names (root cards carry no denormalized display copy — GH #920).
  //
  // Two stable sub-memos gate the rows recomputation. SolidJS only propagates a
  // memo when its return value changes by reference. client.ts holds
  // latestFeedItems / latestProfileCards as stable references and replaces them
  // only when new NOFS / refs.profile data arrives. Resolve/release snapshot
  // frames leave those references unchanged → feedItems() / profileCards()
  // return the same object → rows does not recompute → Post components are not
  // remounted → the resolve/release churn loop breaks.
  const feedItems = createMemo(() => snapshot().feedItems);
  // refs.profile materialised map (pubkey → ProfileWire). client.ts keeps the
  // reference stable across no-op frames so these memos do not churn the feed.
  const profileCards = createMemo(() => snapshot().profileCards);
  const eventCards = createMemo(() => snapshot().eventCards);
  // Name-only join map for the feed rows, derived from the same ProfileWire set.
  const resolvedProfiles = createMemo(() => {
    const cards = profileCards();
    if (!cards) return undefined;
    const names = new Map<string, string>();
    for (const [pubkey, wire] of cards) {
      if (wire.displayName) names.set(pubkey, wire.displayName);
    }
    return names;
  });
  const rows = createMemo(() => feedItemsToRows(feedItems() ?? [], resolvedProfiles()));

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

  const connect = async () => {
    if (!window.nostr) {
      return;
    }
    try {
      const pubkeyHex = await window.nostr.getPublicKey();
      const snap = await client.setSigner(pubkeyHex);
      setSnapshot(snap);
      // Check if signer was accepted (not a capability_failure).
      const lastEvent = snap.events[0];
      if (lastEvent && lastEvent.type === "capability_failure") {
        return;
      }
      setSignerConnected(true);
    } catch {
      // Signer install failed; UI stays disconnected, no crash.
    }
  };

  const publish = async (content: string, replyToId: string | null) => {
    setSnapshot(await client.dispatchChirp(publishNoteAction(content, replyToId)));
  };
  const refreshRoutingDecisions = async () => {
    setRoutingRefreshing(true);
    try {
      setSnapshot(await client.refreshRoutingDecisions());
    } finally {
      setRoutingRefreshing(false);
    }
  };
  const dispatch = async (command: RuntimeCommand) => {
    setSnapshot(await client.dispatchCommand(command));
  };
  // dispatchQuiet — fire-and-forget dispatch that does NOT update the snapshot
  // signal. Used for profile claim/release commands so that the refcount bookkeeping
  // in the kernel does not trigger a reactive re-render that would unmount and
  // remount Post components (which in turn releases + re-claims profiles, causing
  // an infinite churn loop that prevents Bob's display name from ever stabilising).
  const dispatchQuiet = (command: RuntimeCommand): void => {
    void client.dispatchCommand(command);
  };

  // Profile host for the registry user-* components (NostrAvatar / ProfileName).
  // `profile(pubkey)` reads the kernel's `refs.profile` row cache — display name
  // + picture URL + nip05, merged per-key by the client's RefProfileStore.
  // claimProfile/releaseProfile route through the unified resolve_ref/release_ref
  // seam (ADR-0063) and ride the quiet dispatcher so refcount bookkeeping doesn't
  // churn the feed.
  const profileCardMap = createMemo(() => profileCards() ?? new Map<string, ProfileWire>());
  const eventCardMap = createMemo(() => eventCards() ?? new Map<string, ClaimedEventWire>());
  const profileHost: NostrProfileHost = {
    profile: (pubkey) => profileCardMap().get(pubkey),
    claimProfile: (pubkey, consumerId) => dispatchQuiet(resolveProfileCommand(pubkey, consumerId)),
    releaseProfile: (pubkey, consumerId) => dispatchQuiet(releaseProfileCommand(pubkey, consumerId)),
  };
  const eventHost: NostrEventHost = {
    event: (primaryId) => eventCardMap().get(primaryId),
    claimEvent: (primaryId, consumerId, hints, author) =>
      dispatchQuiet(resolveEventCommand(primaryId, consumerId, hints, author)),
    releaseEvent: (primaryId, consumerId) => dispatchQuiet(releaseEventCommand(primaryId, consumerId)),
  };

  return (
    <main class="app-shell">
      <Sidebar active={tab()} onSelect={setTab} feature={feature()} />
      <section class="workspace" aria-label="Chirp workspace">
        <Switch>
          <Match when={tab() === "home"}>
            <HomePanel
              rows={rows()}
              profileHost={profileHost}
              eventHost={eventHost}
              onPublish={publish}
              onCommand={dispatch}
              onClaimCommand={dispatchQuiet}
              onConnect={connect}
              signerConnected={signerConnected()}
            />
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
      <NmpInspector
        snapshot={snapshot()}
        starting={starting()}
        onStart={start}
        onRoutingRefresh={refreshRoutingDecisions}
        routingRefreshing={routingRefreshing()}
      />
    </main>
  );
}
