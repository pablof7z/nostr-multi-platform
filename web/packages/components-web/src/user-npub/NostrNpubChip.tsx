/**
 * NostrNpubChip — copyable short-npub chip for the web.
 *
 * Pure renderer (D7): takes the `npub` (full bech32, from the canonical Rust
 * NIP-19 encoder — never re-derived in the browser, aim.md §6.9) and
 * `npubShort` (a pure string truncation of `npub` the host derives locally
 * via `truncateNpub`, #3098 — a display decision the host owns, not Rust).
 * Renders the short form as a monospace chip; clicking copies the full
 * `npub` to the clipboard. Mirrors the SwiftUI/Compose `NostrNpubChip`.
 */
import type { JSX } from "solid-js";
import { createSignal } from "solid-js";

export function NostrNpubChip(props: { npub: string; npubShort: string }): JSX.Element {
  const [copied, setCopied] = createSignal(false);
  const copy = () => {
    void navigator.clipboard?.writeText(props.npub).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    });
  };
  return (
    <button
      type="button"
      class="nostr-npub-chip"
      classList={{ "nostr-npub-chip--copied": copied() }}
      title={props.npub}
      onClick={copy}
    >
      <span class="nostr-npub-chip__text">{props.npubShort}</span>
      <span class="nostr-npub-chip__copy">{copied() ? "copied ✓" : "copy"}</span>
    </button>
  );
}
