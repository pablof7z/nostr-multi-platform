import { Match, Switch, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { publishNoteAction, openContactFeedCommand, claimProfileCommand, releaseProfileCommand, claimEventCommand, releaseEventCommand, type RuntimeCommand } from "./nmp/actions";
import type { NostrProfileHost } from "./components/user-avatar/NostrProfileHost";
import type { NostrEventHost } from "./components/content-kind-registry/NostrEventHost";
import type { EmbeddedEventModel } from "./components/content-kind-registry/NostrKindRegistry";
import type { ProfileWire } from "./components/user-avatar/ProfileWire";
import type { ClaimedEventWire } from "./nmp/feedProjection";
import { createNmpClient, type RuntimeSnapshot } from "./nmp/client";
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

/** Project a resolved `claimed_events` (KCEV) wire entry into the pure
 *  `EmbeddedEventModel` the embed cards render. Author identity prefers the
 *  kernel's KCEV enrichment (display name / picture joined from kind:0) and
 *  falls back to the resolved_profiles (KRPR) card map for the author pubkey. */
function embeddedModelFromWire(
  ev: ClaimedEventWire,
  profileCards: Map<string, ProfileWire>,
): EmbeddedEventModel {
  const card = profileCards.get(ev.authorPubkey);
  return {
    kind: ev.kind,
    content: ev.content,
    createdAt: ev.createdAt,
    tags: ev.tags,
    authorName: ev.authorDisplayName ?? card?.displayName,
    authorPicture: ev.authorPictureUrl ?? card?.pictureUrl,
  };
}

export default function App() {
  const [snapshot, setSnapshot] = createSignal<RuntimeSnapshot>(client.snapshot());
  const [tab, setTab] = createSignal<AppTab>("home");
  const [starting, setStarting] = createSignal(false);
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
  // resolvedProfiles (KRPR) provides the presentation-layer join for author
  // display names (root cards carry no denormalized display copy — GH #920).
  //
  // Two stable sub-memos gate the rows recomputation. SolidJS only propagates
  // a memo when its return value changes by reference. client.ts holds
  // latestFeedItems / latestResolvedProfiles as stable references and replaces
  // them only when new NOFS/KRPR data arrives. Claim/release
  // snapshot frames leave those references unchanged → feedItems() /
  // resolvedProfiles() return the same object → rows does not recompute →
  // Post components are not remounted → the claim/release/churn loop breaks.
  const feedItems = createMemo(() => snapshot().feedItems);
  const resolvedProfiles = createMemo(() => snapshot().resolvedProfiles);
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
      // Open the contact feed (kinds 1 + 6) now that the viewer pubkey is set.
      setSnapshot(await client.dispatchCommand(openContactFeedCommand()));
    } catch {
      // Signer install failed; UI stays disconnected, no crash.
    }
  };

  const publish = async (content: string, replyToId: string | null) => {
    setSnapshot(await client.dispatchChirp(publishNoteAction(content, replyToId)));
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
  // `profile(pubkey)` reads the kernel's resolved_profiles (KRPR) full
  // ProfileCard map — display name + picture URL + nip05, resolved by the
  // kernel now that the production outbox router is installed. claim/release
  // ride the quiet dispatcher so refcount bookkeeping doesn't churn the feed.
  const profileCards = createMemo(
    () => snapshot().resolvedProfileCards ?? new Map<string, ProfileWire>(),
  );
  const profileHost: NostrProfileHost = {
    profile: (pubkey) => profileCards().get(pubkey),
    claimProfile: (pubkey, consumerId) => dispatchQuiet(claimProfileCommand(pubkey, consumerId)),
    releaseProfile: (pubkey, consumerId) => dispatchQuiet(releaseProfileCommand(pubkey, consumerId)),
  };

  // Event host for quoted-event embed cards (content-view EventRef → card).
  // `claimedEvent(primaryId)` reads the kernel's claimed_events (KCEV) map and
  // projects the wire entry into the pure embed model. claim/release ride the
  // quiet dispatcher (same anti-churn discipline as profile claims) so embed
  // claim bookkeeping never remounts feed rows. The KCEV map itself reaches the
  // UI via the worker-pushed snapshot (client.record decodes it on each frame),
  // so resolution updates reactively without a snapshot-mutating dispatch.
  const claimedEvents = createMemo(
    () => snapshot().claimedEvents ?? new Map<string, ClaimedEventWire>(),
  );
  const eventHost: NostrEventHost = {
    claimedEvent: (primaryId) => {
      const ev = claimedEvents().get(primaryId);
      return ev ? embeddedModelFromWire(ev, profileCards()) : undefined;
    },
    claimEvent: (uri, consumerId) => dispatchQuiet(claimEventCommand(uri, consumerId)),
    releaseEvent: (uri, consumerId) => dispatchQuiet(releaseEventCommand(uri, consumerId)),
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
      <NmpInspector snapshot={snapshot()} starting={starting()} onStart={start} />
    </main>
  );
}
