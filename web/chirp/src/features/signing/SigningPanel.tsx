// SigningPanel — Chirp Web onboarding + signing UI (#2038 item D).
//
// Rendered into <section data-slot="signing"> in App.tsx. Responsibilities:
//   • Onboard an identity via NIP-07 (browser extension) or a local nsec key.
//   • Show the active signer + connection state (event-derived; see signerStatus).
//   • Surface the pending sign round-trip and honest failures.
//   • Degrade honestly: no extension → local-key only; degraded runtime → say so.
//
// Thin-shell contract: ZERO crypto in TS. NIP-07 signing is delegated to the
// extension by the main-thread broker (signBroker.ts). The local-key nsec is
// handed VERBATIM to the Rust LocalKey provider (client.setLocalKeySigner); it
// is never decoded or signed-with here, and is cleared from component state the
// moment it is dispatched.

import { createMemo, createSignal, Show } from "solid-js";
import { encodeNpub, type WorkerEvent } from "@nmp/runtime-web";
import { useNmpClient } from "../../nmp/context";
import type { RuntimeSnapshot } from "../../nmp/client";
import { hasNip07Extension, nip07PublicKey, readNip07Relays } from "./nip07";
import { checkNsecFormat } from "./nsecInput";
import { deriveSignLifecycle, latestCapabilityFailure } from "./signerStatus";
import "./signing.css";

type ActiveIdentity =
  | { kind: "nip07"; pubkey: string }
  | { kind: "local_key" };

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
  const [onboardError, setOnboardError] = createSignal<string | null>(null);
  const [nsec, setNsec] = createSignal("");
  const [showNsec, setShowNsec] = createSignal(false);
  const [npubShort, setNpubShort] = createSignal<string | null>(null);

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

  const connectLocalKey = async () => {
    const check = checkNsecFormat(nsec());
    if (!check.ok) {
      setOnboardError(check.reason);
      return;
    }
    setOnboardError(null);
    setConnecting(true);
    try {
      // Hand the nsec to the Rust LocalKey provider verbatim and immediately
      // drop it from component state. The runtime derives the pubkey + signs.
      const snap: RuntimeSnapshot = await client.setLocalKeySigner(check.value);
      setNsec("");
      const failure = onboardingFailure(snap.events);
      if (failure) {
        setOnboardError(failure);
        props.onConnectionChange?.(false);
      } else {
        setIdentity({ kind: "local_key" });
        props.onConnectionChange?.(true);
        setShowNsec(false);
      }
    } catch (e) {
      setOnboardError(humanizeError(e));
      props.onConnectionChange?.(false);
    } finally {
      setConnecting(false);
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
          </div>

          <Show
            when={hasNip07Extension()}
            fallback={
              <p class="signing-hint" role="status">
                No NIP-07 browser extension detected. Install one (e.g. Alby,
                nos2x) for the most secure sign-in, or use a secret key below.
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

          <Show
            when={showNsec()}
            fallback={
              <button
                class="signing-btn signing-btn--ghost"
                data-action="reveal-nsec"
                onClick={() => setShowNsec(true)}
              >
                Local key
              </button>
            }
          >
            <form
              class="signing-nsec-form"
              onSubmit={(e) => {
                e.preventDefault();
                void connectLocalKey();
              }}
            >
              <label class="signing-label" for="signing-nsec-input">
                Secret key (nsec)
              </label>
              <input
                id="signing-nsec-input"
                class="signing-input"
                type="password"
                autocomplete="off"
                spellcheck={false}
                placeholder="nsec1…"
                data-action="nsec-input"
                value={nsec()}
                onInput={(e) => setNsec(e.currentTarget.value)}
              />
              <p class="signing-hint">
                This path is visible for runtime validation. The current browser
                runtime reports it as unsupported until the local-key provider lands.
              </p>
              <button
                class="signing-btn signing-btn--primary"
                type="submit"
                data-action="connect-nsec"
                disabled={connecting()}
              >
                {connecting() ? "Connecting..." : "Install local key"}
              </button>
            </form>
          </Show>

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
              {identity()?.kind === "nip07" ? "NIP-07 extension" : "Local key"}
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
