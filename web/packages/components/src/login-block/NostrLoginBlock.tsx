import { Show, createSignal, onMount } from "solid-js";

// Identity record for a detected web signer. On the web the canonical signer is
// a NIP-07 browser extension (Alby, nos2x, …) exposed as `window.nostr`. The
// extension is generic — the page cannot enumerate which one — so detection
// yields a single `nip07` entry when `window.nostr` is present.
export interface NostrSignerInfo {
  kind: "nip07";
  /** Human-readable label shown on the card. */
  displayName: string;
}

// Minimal NIP-07 provider shape the block touches. The host's sign-in flow
// owns the rest (getPublicKey → setSigner).
interface Nip07Provider {
  getPublicKey(): Promise<string>;
}

function detectNip07(): Nip07Provider | undefined {
  if (typeof window === "undefined") return undefined;
  const w = window as unknown as { nostr?: Nip07Provider };
  return w.nostr && typeof w.nostr.getPublicKey === "function" ? w.nostr : undefined;
}

// Login block — detects a NIP-07 browser-signer and surfaces it as a one-click
// sign-in card, falling back to a manual key-entry row (and an install hint)
// when none is present. Mirrors the SwiftUI / Compose `NostrLoginBlock` signer
// detection + manual fallback. Detection runs lazily in `onMount` (never at
// module load) so `window.nostr` is fully injected by the time it runs.
//
// The host wires `onSignerSelected` to its sign-in flow (call
// `window.nostr.getPublicKey()` then install the signer) and `onManualKey` to
// its key-import flow.
export function NostrLoginBlock(props: {
  onSignerSelected?: (info: NostrSignerInfo, provider: Nip07Provider) => void;
  onManualKey?: (key: string) => void;
}) {
  const [provider, setProvider] = createSignal<Nip07Provider | undefined>(undefined);
  const [key, setKey] = createSignal("");

  // Lazy detection — NIP-07 extensions inject `window.nostr` asynchronously, so
  // probe on mount (and once more on the next tick) rather than at import time.
  onMount(() => {
    setProvider(detectNip07());
    if (!detectNip07()) {
      const t = setTimeout(() => setProvider(detectNip07()), 300);
      return () => clearTimeout(t);
    }
  });

  return (
    <div class="nostr-login-block" data-has-nip07={provider() ? "true" : "false"}>
      <Show
        when={provider()}
        fallback={
          <div class="nostr-login-block__hint">
            No NIP-07 browser signer detected. Install a signer extension (e.g. Alby or
            nos2x), or paste a key below.
          </div>
        }
      >
        {(p) => (
          <button
            type="button"
            class="nostr-login-block__signer"
            onClick={() =>
              props.onSignerSelected?.({ kind: "nip07", displayName: "Browser extension (NIP-07)" }, p())
            }
          >
            <span class="nostr-login-block__signer-icon" aria-hidden="true">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                <path d="M12 1l3 6 6 .9-4.5 4.3 1 6.3L12 16.5 6.5 18.5l1-6.3L3 7.9 9 7z" />
              </svg>
            </span>
            <span class="nostr-login-block__signer-label">Sign in with browser extension (NIP-07)</span>
          </button>
        )}
      </Show>

      <form
        class="nostr-login-block__manual"
        onSubmit={(e) => {
          e.preventDefault();
          const v = key().trim();
          if (v.length > 0) props.onManualKey?.(v);
        }}
      >
        <input
          class="nostr-login-block__input"
          type="text"
          inputmode="text"
          autocomplete="off"
          spellcheck={false}
          placeholder="npub1… (read-only) or nsec1…"
          value={key()}
          onInput={(e) => setKey(e.currentTarget.value)}
          aria-label="Nostr key"
        />
        <button type="submit" class="nostr-login-block__submit" disabled={key().trim().length === 0}>
          Use key
        </button>
      </form>
    </div>
  );
}
