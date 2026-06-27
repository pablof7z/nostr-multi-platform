// SigningPanel — Chirp Web onboarding + signing UI (#2038 item D).
//
// Rendered into <section data-slot="signing"> in App.tsx. Responsibilities:
//   • Onboard an identity via NIP-07 (browser extension).
//   • Show the active signer + connection state (event-derived; see signerStatus).
//   • Surface the pending sign round-trip and honest failures.
//   • Degrade honestly: no extension or unsupported signer path → say so.
//
// Thin-shell contract: ZERO crypto in TS. NIP-07 signing is delegated to the
// extension by the main-thread broker (signBroker.ts).

import { createMemo, createSignal, Show } from "solid-js";
import { encodeNpub, type WorkerEvent } from "@nmp/runtime-web";
import { useNmpClient } from "../../nmp/context";
import type { RuntimeSnapshot } from "../../nmp/client";
import { hasNip07Extension, nip07PublicKey, readNip07Relays } from "./nip07";
import { deriveSignLifecycle, latestCapabilityFailure } from "./signerStatus";
import "./signing.css";

type ActiveIdentity =
  | { kind: "nip07"; pubkey: string }
  | { kind: "local" };

type SigningPanelProps = {
  onConnectionChange?: (connected: boolean) => void;
};

/** Inspect a post-onboarding snapshot for a capability failure that aborted the
 *  identity install. Returns the honest reason, or undefined on success. */
function onboardingFailure(events: readonly WorkerEvent[]): string | undefined {
  return latestCapabilityFailure(events);
}

function humanizeError(e: unknown): string {
  if (e instanceof Error && e.message) return e.message;
  return String(e);
}

function shortHex(hex: string): string {
  return hex.length <= 12 ? hex : `${hex.slice(0, 8)}…${hex.slice(-4)}`;
}

export function SigningPanel(props: SigningPanelProps) {
  const { client, snapshot } = useNmpClient();

  const [identity, setIdentity] = createSignal<ActiveIdentity | null>(null);
  const [connecting, setConnecting] = createSignal(false);
  const [importingLocal, setImportingLocal] = createSignal(false);
  const [onboardError, setOnboardError] = createSignal<string | null>(null);
  const [npubShort, setNpubShort] = createSignal<string | null>(null);
  let localKeyInput: HTMLInputElement | undefined;

  const degraded = createMemo(() => snapshot().clientRuntime === "in_process_fallback");
  const connected = createMemo(() => identity() !== null);
  const signLifecycle = createMemo(() => deriveSignLifecycle(snapshot().events));
  const signPending = createMemo(() => signLifecycle().phase === "pending");
  const signFailureReason = createMemo(() => {
    const lifecycle = signLifecycle();
    return lifecycle.phase === "failed" ? lifecycle.reason : undefined;
  });

  const resolveNpub = (pubkey: string) => {
    setNpubShort(shortHex(pubkey));
    void encodeNpub(pubkey).then((res) => {
      if (res) setNpubShort(res.npubShort);
    });
  };

  const connectNip07 = async () => {
    setOnboardError(null);
    setConnecting(true);
    try {
      const pubkey = await nip07PublicKey();
      const relays = await readNip07Relays();
      const snap: RuntimeSnapshot = await client.setSigner(pubkey, relays);
      const failure = onboardingFailure(snap.events);
      if (failure) {
        setOnboardError(failure);
        props.onConnectionChange?.(false);
      } else {
        setIdentity({ kind: "nip07", pubkey });
        props.onConnectionChange?.(true);
        resolveNpub(pubkey);
      }
    } catch (e) {
      setOnboardError(humanizeError(e));
      props.onConnectionChange?.(false);
    } finally {
      setConnecting(false);
    }
  };

  const importLocalKey = async (event: SubmitEvent) => {
    event.preventDefault();
    const secret = localKeyInput?.value.trim() ?? "";
    if (!secret) return;
    setOnboardError(null);
    setImportingLocal(true);
    try {
      const snap: RuntimeSnapshot = await client.setLocalKeySigner(secret);
      if (localKeyInput) localKeyInput.value = "";
      const failure = onboardingFailure(snap.events);
      if (failure) {
        setOnboardError(failure);
        props.onConnectionChange?.(false);
      } else {
        setIdentity({ kind: "local" });
        setNpubShort("session key");
        props.onConnectionChange?.(true);
      }
    } catch (e) {
      setOnboardError(humanizeError(e));
      props.onConnectionChange?.(false);
    } finally {
      setImportingLocal(false);
    }
  };

  return (
    <div class="signing-panel" data-signer-connected={connected() ? "true" : "false"}>
      <Show when={degraded()}>
        <p class="signing-degraded" role="status">
          Signing is unavailable — the nmp runtime is running in degraded
          (in-process) mode, so no signer can be installed. Reload in a browser
          with Web Worker + WebAssembly support.
        </p>
      </Show>

      <Show when={!degraded() && !connected()}>
        <div class="signing-onboarding">
          <div>
            <p class="signing-kicker">Identity</p>
            <h2 class="signing-title">Connect signer</h2>
            <p class="signing-intro">
              Pick a signing method for this browser session. Chirp will publish
              only after the runtime confirms the signer is installed.
            </p>
          </div>

          <div class="signing-methods" aria-label="Signing methods">
            <section class="signing-method" data-method="nip07">
              <div>
                <strong>NIP-07 browser signer</strong>
                <span>Use the account already active in your extension.</span>
              </div>
              <Show
                when={hasNip07Extension()}
                fallback={
                  <p class="signing-hint" role="status">
                    No extension detected in this browser. Install Alby, nos2x,
                    or use the session key option.
                  </p>
                }
              >
                <button
                  class="signing-btn signing-btn--primary connect-btn"
                  data-action="connect-nip07"
                  disabled={connecting()}
                  onClick={() => void connectNip07()}
                >
                  {connecting() ? "Connecting..." : "Connect NIP-07"}
                </button>
              </Show>
            </section>

            <section class="signing-method" data-method="local-key">
              <div>
                <strong>Session nsec</strong>
                <span>Paste an nsec for a memory-only local signer.</span>
              </div>
              <form class="signing-nsec-form" onSubmit={importLocalKey}>
                <label class="signing-label" for="local-nsec">
                  Secret key
                </label>
                <input
                  ref={localKeyInput}
                  id="local-nsec"
                  class="signing-input"
                  data-testid="local-nsec-input"
                  type="password"
                  autocomplete="off"
                  spellcheck={false}
                  placeholder="nsec1..."
                  disabled={importingLocal()}
                />
                <button
                  class="signing-btn signing-btn--ghost"
                  data-testid="local-nsec-submit"
                  type="submit"
                  disabled={importingLocal()}
                >
                  {importingLocal() ? "Importing..." : "Use for this session"}
                </button>
                <p class="signing-hint">
                  Not saved to localStorage, sessionStorage, IndexedDB, OPFS,
                  snapshots, logs, or URL state.
                </p>
              </form>
            </section>
          </div>

          <Show when={onboardError()}>
            <p class="signing-error" role="alert" data-slot="onboard-error">
              {onboardError()}
            </p>
          </Show>
        </div>
      </Show>

      <Show when={!degraded() && connected()}>
        <div class="signing-active" data-slot="active-signer">
          <div class="signing-active-row">
            <span class="signing-badge" data-signer-kind={identity()?.kind}>
              {identity()?.kind === "local" ? "Local key" : "NIP-07 extension"}
            </span>
            <Show when={npubShort()}>
              <span class="signing-npub" title="active account">
                {npubShort()}
              </span>
            </Show>
          </div>

          <Show when={signPending()}>
            <p class="signing-pending" role="status" data-slot="sign-pending">
              Waiting for your signature…
            </p>
          </Show>
          <Show when={signFailureReason()}>
            {(reason) => (
              <p class="signing-error" role="alert" data-slot="sign-error">
                Signing failed: {reason()}
              </p>
            )}
          </Show>
        </div>
      </Show>
    </div>
  );
}
