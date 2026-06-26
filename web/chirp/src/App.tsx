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
// Item D: import and render signing/onboarding UI inside [data-slot="signing"].

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

  const unsubscribe = client.subscribe(setSnapshot);
  onCleanup(unsubscribe);

  // Boot the runtime on mount: parse relay_bootstrap from URL and send `start`.
  onMount(() => void start());

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
            <a class="rail-link rail-link--active" href="#feed" aria-current="page">Home</a>
            <a class="rail-link" href="#signing">Signer</a>
            <a class="rail-link" href="#profile">Profile</a>
            <a class="rail-link" href="#relays">Relays</a>
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
              <p class="topbar-kicker">Home feed</p>
              <h1>Real relay timeline</h1>
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

          <div class="content-grid">
            {/*
              MOUNT POINT — Item C: feed / profile / publish UI.
              Item C imports its panel components and renders them here via
              NmpClientContext. Do not add logic to this slot — zero protocol TS.
            */}
            <section id="feed" data-slot="feed" aria-label="Feed">
              <FeedPanel canPublish={signerConnected()} />
            </section>

            {/*
              MOUNT POINT — Item D: signing / onboarding UI.
              SigningPanel renders the NIP-07 + local-key onboarding flow, the active
              signer status, and the pending-sign overlay. All signing logic lives in
              features/signing/ and reads the runtime via NmpClientContext.
            */}
            <section id="signing" class="side-stack" data-slot="signing" aria-label="Signing">
              <OnboardingPanel
                state={{
                  runtimeConnected: isConnected(),
                  signerConnected: signerConnected(),
                  feedReady: feedReady(),
                  feedCount: feedCount(),
                  runtimeMode: bridgeKind(),
                  diagnostics: runtimeProjection(),
                }}
              />
              <SigningPanel onConnectionChange={setSignerConnected} />
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
