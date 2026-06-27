// App.tsx — Chirp Web composition root (Item B: thin shell).
//
// Responsibilities:
//   1. Boot the NmpWasmRuntime worker via @nmp/runtime-web.
//   2. Parse `?relay_bootstrap=` from the URL and forward it in the `start` request.
//   3. Subscribe to the snapshot/update stream and expose it via NmpClientContext.
//   4. Wire the NIP-07 sign broker (sign_request events handled in client.ts).
//   5. Expose data-attribute test hooks for Item E acceptance tests:
//        data-bridge-kind       = "worker" | "in_process_fallback"
//        data-runtime-status    = status string (e.g. "running", "degraded:…")
//        data-has-snapshot      = "true" | "false"
//   6. Render a minimal status indicator and WELL-DEFINED MOUNT POINTS for
//        Item C (feed/profile UI) — <section data-slot="feed">
//        Item D (signing/onboarding UI) — <section data-slot="signing">
//
// Zero protocol logic in TS. All Nostr behaviour is Rust-owned in
// crates/nmp-browser-runtime. If you see Nostr event construction here it is a
// bug (Chirp is a reusability proof — docs/aim.md).
//
// Item C: import and render your components inside [data-slot="feed"] via
//   the NmpClientContext.Provider tree (useSnapshot / useNmpClient).
// Item D: render signing/onboarding UI inside [data-slot="signing"]. The slot
// stays mounted for the whole browser session so signer-local presentation state
// is not lost after the identity step completes.

import { createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { createNmpClient, type RuntimeSnapshot } from "./nmp/client";
import { NmpClientProvider } from "./nmp/context";
import { SigningPanel } from "./features/signing/SigningPanel";
import { chirpRelayOverrideFromSearch } from "./chirpConfig";
import { OnboardingPanel } from "./features/onboarding/OnboardingPanel";
import { DiagnosticsPanel } from "./features/diagnostics/DiagnosticsPanel";
import { RelaySettingsPanel } from "./features/relays/RelaySettingsPanel";
import { ProfileEditor } from "./features/profile/ProfileEditor";
import { decodeUpdateFrame } from "./nmp/feedDecoder";
import { decodeRuntimeProjection } from "./nmp/runtimeProjection";
// Item C — feed / publish / profile UI (FeedPanel owns its own store + provider).
import { FeedPanel } from "./features/feed/FeedPanel";
import { GroupsPanel } from "./features/groups/GroupsPanel";
import { NotificationsPanel } from "./features/notifications/NotificationsPanel";
import { OfflineReplayPanel } from "./features/offline/OfflineReplayPanel";
import { SearchPanel } from "./features/search/SearchPanel";
import { BlockedWorkspacesPanel } from "./features/workspaces/BlockedWorkspacesPanel";

// NIP-07 browser extension interface (window.nostr — EIP-1193-style extension).
// Shared ambient declaration: signBroker.ts, client.ts, and the signing feature
// (features/signing/nip07.ts) all read `window.nostr` through this type.
declare global {
  interface Window {
    nostr?: {
      getPublicKey(): Promise<string>;
      getRelays?(): Promise<Record<string, { read?: boolean; write?: boolean }>>;
      signEvent(event: Record<string, unknown>): Promise<Record<string, unknown>>;
    };
  }
}

// The client is a module-level singleton: one worker per page load.
const client = createNmpClient();

type MainView = "home" | "search" | "notifications" | "groups" | "offline" | "workspaces";

function viewFromHash(): MainView {
  const hash = window.location.hash;
  if (hash === "#search") return "search";
  if (hash === "#notifications") return "notifications";
  if (hash === "#groups") return "groups";
  if (hash === "#offline") return "offline";
  if (
    hash === "#workspaces" ||
    hash === "#messages" ||
    hash === "#wallet" ||
    hash === "#moderation"
  ) {
    return "workspaces";
  }
  return "home";
}

/** Derive a stable string from the runtime status for data attributes and UI. */
function runtimeStatusLabel(snapshot: RuntimeSnapshot): string {
  const s = snapshot.status;
  if (typeof s === "string") return s;
  return `degraded:${s.degraded}`;
}

export default function App() {
  const [snapshot, setSnapshot] = createSignal<RuntimeSnapshot>(client.snapshot());
  const [signerConnected, setSignerConnected] = createSignal(false);
  const [mainView, setMainView] = createSignal<MainView>(viewFromHash());

  const unsubscribe = client.subscribe(setSnapshot);
  onCleanup(unsubscribe);

  // Boot the runtime on mount: parse relay_bootstrap from URL and send `start`.
  onMount(() => {
    const syncHash = () => setMainView(viewFromHash());
    window.addEventListener("hashchange", syncHash);
    onCleanup(() => window.removeEventListener("hashchange", syncHash));
    void start();
  });

  const start = async () => {
    const override = chirpRelayOverrideFromSearch(window.location.search);
    setSnapshot(await client.start(override));
  };

  // Derived test-hook values (reactive, zero allocations on stable status).
  const bridgeKind = () => snapshot().clientRuntime;
  const runtimeStatus = () => runtimeStatusLabel(snapshot());
  const hasSnapshot = () => snapshot().latestUpdateBytes !== undefined;
  const isConnected = () => hasSnapshot();
  const runtimeModeLabel = () =>
    snapshot().clientRuntime === "worker" ? "worker runtime" : "degraded runtime";
  const runtimeProjection = createMemo(() => decodeRuntimeProjection(snapshot().latestUpdateBytes));
  const feedFrame = createMemo(() => {
    const bytes = snapshot().latestUpdateBytes;
    return bytes ? decodeUpdateFrame(bytes) : undefined;
  });
  const feedReady = () => feedFrame() !== undefined;
  const feedCount = () => feedFrame()?.rows.length ?? 0;
  const topbarKicker = () =>
    mainView() === "search"
      ? "NIP-50 discovery"
      : mainView() === "notifications"
        ? "Notifications"
      : mainView() === "groups"
        ? "NIP-29 groups"
      : mainView() === "offline"
        ? "Storage and replay"
      : mainView() === "workspaces"
        ? "Product coverage"
        : "Home feed";
  const topbarTitle = () => {
    if (mainView() === "search") return "Search relays and cache";
    if (mainView() === "notifications") return "Notifications";
    if (mainView() === "groups") return "Discover public groups";
    if (mainView() === "offline") return "Inspect storage health";
    if (mainView() === "workspaces") return "More Chirp workspaces";
    return signerConnected() ? "Real relay timeline" : "Set up Chirp Web";
  };
  const topbarSupport = () => {
    if (mainView() === "search") {
      return "Find notes, profiles, and long-form posts with relay and cache provenance.";
    }
    if (mainView() === "notifications") {
      return "Review replies, mentions, reactions, reposts, comments, and zaps with source relays.";
    }
    if (mainView() === "groups") {
      return "Browse Rust-projected NIP-29 group metadata from the configured public group relay.";
    }
    if (mainView() === "offline") {
      return "Inspect store health, active replay interests, relay coverage, and pending publish state.";
    }
    if (mainView() === "workspaces") {
      return "Private, value, and moderation surfaces stay disabled until Rust-owned web flows exist.";
    }
    return signerConnected()
      ? "Read, publish, and verify every action through relay diagnostics."
      : "Browse signed out, connect a signer when you are ready to publish.";
  };
  const onboardingState = () => ({
    runtimeConnected: isConnected(),
    signerConnected: signerConnected(),
    feedReady: feedReady(),
    feedCount: feedCount(),
    runtimeMode: bridgeKind(),
    diagnostics: runtimeProjection(),
  });

  return (
    <NmpClientProvider client={client} snapshot={snapshot}>
      {/*
        Root element carries data attributes for Item E acceptance tests:
          data-bridge-kind     — "worker" (real wasm) or "in_process_fallback"
          data-runtime-status  — e.g. "running", "ready", "degraded:browser_bridge_unavailable"
          data-has-snapshot    — "true" once the first UpdateFrame arrives
      */}
      <main
        class="app-shell"
        data-bridge-kind={bridgeKind()}
        data-runtime-status={runtimeStatus()}
        data-has-snapshot={hasSnapshot() ? "true" : "false"}
        data-signer-connected={signerConnected() ? "true" : "false"}
      >
        <aside class="app-rail" aria-label="Chirp navigation">
          <div class="brand-lockup">
            <span class="brand-mark" aria-hidden="true">C</span>
            <div>
              <strong>Chirp</strong>
              <span>NMP Web</span>
            </div>
          </div>
          <nav class="rail-nav" aria-label="Primary">
            <a
              class={mainView() === "home" ? "rail-link rail-link--active" : "rail-link"}
              href="#feed"
              aria-current={mainView() === "home" ? "page" : undefined}
            >
              Home
            </a>
            <a
              class={mainView() === "search" ? "rail-link rail-link--active" : "rail-link"}
              href="#search"
              aria-current={mainView() === "search" ? "page" : undefined}
              data-testid="nav-search"
            >
              Search
            </a>
            <a
              class={mainView() === "groups" ? "rail-link rail-link--active" : "rail-link"}
              href="#groups"
              aria-current={mainView() === "groups" ? "page" : undefined}
              data-testid="nav-groups"
            >
              Groups
            </a>
            <a class="rail-link" href="#saved">Saved</a>
            <a
              class={mainView() === "notifications" ? "rail-link rail-link--active" : "rail-link"}
              href="#notifications"
              aria-current={mainView() === "notifications" ? "page" : undefined}
              data-testid="nav-notifications"
            >
              Notifications
            </a>
            <a class="rail-link" href="#signing">Signer</a>
            <a class="rail-link" href="#profile">Profile</a>
            <a class="rail-link" href="#relays">Relays</a>
            <a
              class={mainView() === "offline" ? "rail-link rail-link--active" : "rail-link"}
              href="#offline"
              aria-current={mainView() === "offline" ? "page" : undefined}
              data-testid="nav-offline"
            >
              Storage
            </a>
            <a
              class={mainView() === "workspaces" ? "rail-link rail-link--active" : "rail-link"}
              href="#workspaces"
              aria-current={mainView() === "workspaces" ? "page" : undefined}
              data-testid="nav-workspaces"
            >
              More
            </a>
            <a class="rail-link" href="#diagnostics">Diagnostics</a>
          </nav>
          <div class="rail-status" aria-live="polite">
            <span class="status-dot" data-connected={isConnected() ? "true" : "false"} />
            <div>
              <strong>{isConnected() ? "Connected" : "Connecting"}</strong>
              <span>{runtimeModeLabel()}</span>
            </div>
          </div>
        </aside>

        <div class="app-main">
          <header class="topbar">
            <div>
              <p class="topbar-kicker">{topbarKicker()}</p>
              <h1>{topbarTitle()}</h1>
              <p class="topbar-support">{topbarSupport()}</p>
            </div>
            <div
              class="status-indicator"
              aria-live="polite"
              data-connected={isConnected() ? "true" : "false"}
            >
              <span class="status-dot" data-connected={isConnected() ? "true" : "false"} />
              <span>{isConnected() ? "connected" : "connecting"}</span>
              {snapshot().clientRuntime === "in_process_fallback" && (
                <span aria-label="degraded mode">degraded</span>
              )}
            </div>
          </header>

          <div class="content-grid" data-first-run={signerConnected() ? "false" : "true"}>
            <section
              id="signing"
              class="first-run-workspace"
              data-slot="signing"
              aria-label="First-run setup"
            >
              <OnboardingPanel state={onboardingState()} />
              <SigningPanel onConnectionChange={setSignerConnected} />
            </section>

            {/*
              MOUNT POINT — Item C: feed / profile / publish UI.
              Item C imports its panel components and renders them here via
              NmpClientContext. Do not add logic to this slot — zero protocol TS.
            */}
            <section id="feed" data-slot="feed" aria-label="Feed">
              {mainView() === "search" && <SearchPanel />}
              {mainView() === "notifications" && <NotificationsPanel />}
              {mainView() === "groups" && <GroupsPanel />}
              {mainView() === "offline" && (
                <OfflineReplayPanel diagnostics={runtimeProjection()} />
              )}
              {mainView() === "workspaces" && (
                <BlockedWorkspacesPanel signedIn={signerConnected()} />
              )}
              {mainView() === "home" && (
                <FeedPanel canPublish={signerConnected()} diagnostics={runtimeProjection()} />
              )}
            </section>

            {/*
              MOUNT POINT — Item D: signing / onboarding UI.
              SigningPanel renders the NIP-07 + local-key onboarding flow, the active
              signer status, and the pending-sign overlay. All signing logic lives in
              features/signing/ and reads the runtime via NmpClientContext.
            */}
            <section class="side-stack" data-slot="session-tools" aria-label="Session tools">
              <ProfileEditor canPublish={signerConnected()} />
              <div id="relays">
                <RelaySettingsPanel
                  diagnostics={runtimeProjection()}
                  canPublishRelayPreferences={signerConnected()}
                />
              </div>
              <div id="diagnostics">
                <DiagnosticsPanel diagnostics={runtimeProjection()} events={snapshot().events} />
              </div>
            </section>
          </div>
        </div>
      </main>
    </NmpClientProvider>
  );
}
