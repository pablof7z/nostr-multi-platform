// Chirp Web composition root. Keep this a thin renderer: Rust owns Nostr
// protocol/session behavior in crates/nmp-browser-runtime.

import { createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { viewCopy, viewFromHash, type MainView } from "./appNavigation";
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
import { FeedPanel } from "./features/feed/FeedPanel";
import { GroupsPanel } from "./features/groups/GroupsPanel";
import { MessagesPanel } from "./features/messages/MessagesPanel";
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

/** Derive a stable string from the runtime status for data attributes and UI. */
function runtimeStatusLabel(snapshot: RuntimeSnapshot): string {
  const s = snapshot.status;
  if (typeof s === "string") return s;
  return `degraded:${s.degraded}`;
}

export default function App() {
  const [snapshot, setSnapshot] = createSignal<RuntimeSnapshot>(client.snapshot());
  const [signerConnected, setSignerConnected] = createSignal(false);
  const [mainView, setMainView] = createSignal<MainView>(viewFromHash(window.location.hash));

  const unsubscribe = client.subscribe(setSnapshot);
  onCleanup(unsubscribe);

  // Boot the runtime on mount: parse relay_bootstrap from URL and send `start`.
  onMount(() => {
    const syncHash = () => setMainView(viewFromHash(window.location.hash));
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
  const topbar = () => viewCopy(mainView(), signerConnected());
  const setupOrSignerView = () => mainView() === "setup" || mainView() === "signer";
  const focusedToolView = () => mainView() === "profile" || mainView() === "relays";
  const blockedRoute = () => (mainView() === "wallet" || mainView() === "moderation" ? mainView() : undefined);
  const onboardingState = () => ({
    runtimeConnected: isConnected(),
    signerConnected: signerConnected(),
    feedReady: feedReady(),
    feedCount: feedCount(),
    runtimeMode: bridgeKind(),
    diagnostics: runtimeProjection(),
  });
  createEffect(() => {
    if (signerConnected() && mainView() === "setup") setMainView("home");
  });

  return (
    <NmpClientProvider client={client} snapshot={snapshot}>
      {/* Root data attributes are the browser acceptance hooks for runtime health. */}
      <main
        class="app-shell"
        data-bridge-kind={bridgeKind()}
        data-runtime-status={runtimeStatus()}
        data-has-snapshot={hasSnapshot() ? "true" : "false"}
        data-signer-connected={signerConnected() ? "true" : "false"}
        data-main-view={mainView()}
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
              class={mainView() === "setup" ? "rail-link rail-link--active" : "rail-link"}
              href="#setup"
              aria-current={mainView() === "setup" ? "page" : undefined}
              data-testid="nav-setup"
            >
              Setup
            </a>
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
            <a
              class={mainView() === "saved" ? "rail-link rail-link--active" : "rail-link"}
              href="#saved"
              aria-current={mainView() === "saved" ? "page" : undefined}
              data-testid="nav-saved"
            >
              Saved
            </a>
            <a
              class={mainView() === "notifications" ? "rail-link rail-link--active" : "rail-link"}
              href="#notifications"
              aria-current={mainView() === "notifications" ? "page" : undefined}
              data-testid="nav-notifications"
            >
              Notifications
            </a>
            <a
              class={mainView() === "messages" ? "rail-link rail-link--active" : "rail-link"}
              href="#messages"
              aria-current={mainView() === "messages" ? "page" : undefined}
              data-testid="nav-messages"
            >
              Messages
            </a>
            <a class={mainView() === "signer" ? "rail-link rail-link--active" : "rail-link"} href="#signing" aria-current={mainView() === "signer" ? "page" : undefined} data-testid="nav-signer">
              Signer
            </a>
            <a class={mainView() === "profile" ? "rail-link rail-link--active" : "rail-link"} href="#profile" aria-current={mainView() === "profile" ? "page" : undefined} data-testid="nav-profile">
              Profile
            </a>
            <a class={mainView() === "relays" ? "rail-link rail-link--active" : "rail-link"} href="#relays" aria-current={mainView() === "relays" ? "page" : undefined} data-testid="nav-relays">
              Relays
            </a>
            <a
              class={mainView() === "offline" ? "rail-link rail-link--active" : "rail-link"}
              href="#offline"
              aria-current={mainView() === "offline" ? "page" : undefined}
              data-testid="nav-offline"
            >
              Storage
            </a>
            <a
              class={mainView() === "workspaces" || blockedRoute() ? "rail-link rail-link--active" : "rail-link"}
              href="#workspaces"
              aria-current={mainView() === "workspaces" || blockedRoute() ? "page" : undefined}
              data-testid="nav-workspaces"
            >
              More
            </a>
            <a class={mainView() === "diagnostics" ? "rail-link rail-link--active" : "rail-link"} href="#diagnostics" aria-current={mainView() === "diagnostics" ? "page" : undefined} data-testid="nav-diagnostics">
              Diagnostics
            </a>
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
              <p class="topbar-kicker">{topbar().kicker}</p>
              <h1>{topbar().title}</h1>
              <p class="topbar-support">{topbar().support}</p>
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

          <div
            class="content-grid"
            data-first-run={signerConnected() ? "false" : "true"}
            data-main-view={mainView()}
          >
            <section
              id="signing"
              class="first-run-workspace"
              data-slot="signing"
              aria-label="First-run setup"
            >
              <OnboardingPanel state={onboardingState()} />
              <SigningPanel onConnectionChange={setSignerConnected} />
            </section>

            {!setupOrSignerView() && (
              <>
                <section id="feed" data-slot="feed" aria-label="Feed">
                  {mainView() === "search" && <SearchPanel />}
                  {mainView() === "notifications" && <NotificationsPanel />}
                  {mainView() === "messages" && <MessagesPanel />}
                  {mainView() === "groups" && <GroupsPanel />}
                  {mainView() === "profile" && <ProfileEditor canPublish={signerConnected()} />}
                  {mainView() === "relays" && (
                    <div id="relays">
                      <RelaySettingsPanel
                        diagnostics={runtimeProjection()}
                        canPublishRelayPreferences={signerConnected()}
                      />
                    </div>
                  )}
                  {mainView() === "offline" && (
                    <OfflineReplayPanel diagnostics={runtimeProjection()} />
                  )}
                  {(mainView() === "workspaces" || blockedRoute()) && (
                    <BlockedWorkspacesPanel signedIn={signerConnected()} focus={blockedRoute()} />
                  )}
                  {mainView() === "diagnostics" && (
                    <div id="diagnostics">
                      <DiagnosticsPanel diagnostics={runtimeProjection()} events={snapshot().events} />
                    </div>
                  )}
                  {(mainView() === "home" || mainView() === "saved") && (
                    <FeedPanel canPublish={signerConnected()} diagnostics={runtimeProjection()} />
                  )}
                </section>
                {mainView() !== "diagnostics" && (
                  <section class="side-stack" data-slot="session-tools" aria-label="Session tools">
                    {focusedToolView() ? (
                      <DiagnosticsPanel diagnostics={runtimeProjection()} events={snapshot().events} />
                    ) : (
                      <>
                        <ProfileEditor canPublish={signerConnected()} />
                        <div id="relays">
                          <RelaySettingsPanel
                            diagnostics={runtimeProjection()}
                            canPublishRelayPreferences={signerConnected()}
                          />
                        </div>
                        <div id="diagnostics">
                          <DiagnosticsPanel
                            diagnostics={runtimeProjection()}
                            events={snapshot().events}
                          />
                        </div>
                      </>
                    )}
                  </section>
                )}
              </>
            )}
          </div>
        </div>
      </main>
    </NmpClientProvider>
  );
}
