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

import { createSignal, onCleanup, onMount } from "solid-js";
import { createNmpClient, type RuntimeSnapshot } from "./nmp/client";
import { NmpClientProvider } from "./nmp/context";
import { SigningPanel } from "./features/signing/SigningPanel";
import { chirpRelayOverrideFromSearch } from "./chirpConfig";

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
        {/* Status indicator — visible while connecting; updates live. */}
        <div
          class="status-indicator"
          aria-live="polite"
          data-connected={isConnected() ? "true" : "false"}
        >
          {isConnected() ? "connected" : "connecting…"}
          {snapshot().clientRuntime === "in_process_fallback" && (
            <span aria-label="degraded mode"> (degraded)</span>
          )}
        </div>

        {/*
          MOUNT POINT — Item C: feed / profile / publish UI.
          Item C imports its panel components and renders them here via
          NmpClientContext. Do not add logic to this slot — zero protocol TS.
        */}
        <section data-slot="feed" aria-label="Feed" />

        {/*
          MOUNT POINT — Item D: signing / onboarding UI.
          SigningPanel renders the NIP-07 + local-key onboarding flow, the active
          signer status, and the pending-sign overlay. All signing logic lives in
          features/signing/ and reads the runtime via NmpClientContext.
        */}
        <section data-slot="signing" aria-label="Signing">
          <SigningPanel />
        </section>
      </main>
    </NmpClientProvider>
  );
}
