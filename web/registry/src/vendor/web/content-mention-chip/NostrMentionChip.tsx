/**
 * NostrMentionChip — inline mention chip for a Nostr profile reference (web / SolidJS).
 *
 * Renders a compact `@displayName` or `@npub1…` chip for a Mention node.
 *
 * Stage 0: display-name resolution via the kernel's profile-claim flow is the
 * app's responsibility. Pass `displayName` when resolved; the component falls
 * back to `npubLabel` (abbreviated npub) when absent. Never blocks rendering.
 *
 * Install: nmp add web/content-mention-chip
 * Dependencies: solid-js
 */
import type { JSX } from "solid-js";
import { Show } from "solid-js";

// ── Props ─────────────────────────────────────────────────────────────────────

export type NostrMentionChipProps = {
  /**
   * Full `nostr:npub1…` URI — used as the anchor href.
   * The component renders this as-is; the caller provides the bech32 URI
   * as decoded from the kernel's NFCT Mention node `nostrUri.uri` field.
   */
  nostrUri: string;
  /**
   * Kind:0 display name — supplied by the app after profile-claim resolution.
   * Absent until the kernel delivers the kind:0 event.
   */
  displayName?: string;
  /**
   * Abbreviated npub for use before `displayName` is resolved.
   * Derive from `nostrUri` at the call site (e.g. `npub1…` → `npub1xxxx…yy`).
   * When omitted the component abbreviates `nostrUri` itself.
   */
  npubLabel?: string;
  /** Optional picture URL for a 16 px avatar. Falls back to initial letter. */
  pictureUrl?: string;
  /** Called when the user taps/clicks the chip. */
  onTap?: (nostrUri: string) => void;
};

// ── Public component ──────────────────────────────────────────────────────────

/**
 * Inline mention chip. Renders `@displayName` when resolved, otherwise
 * `@npub1xxxx…` as an honest fallback. Never blocks or shows a spinner.
 */
export function NostrMentionChip(props: NostrMentionChipProps): JSX.Element {
  const label = (): string =>
    props.displayName
      ? `@${props.displayName}`
      : `@${props.npubLabel ?? abbreviate(props.nostrUri)}`;

  const handleClick = (): void => {
    props.onTap?.(props.nostrUri);
  };

  return (
    <a
      class="nostr-mention-chip"
      href={props.nostrUri}
      rel="noopener noreferrer"
      onClick={(e) => {
        if (props.onTap) {
          e.preventDefault();
          handleClick();
        }
      }}
      aria-label={label()}
    >
      <MentionAvatar pictureUrl={props.pictureUrl} label={label()} />
      <span class="nostr-mention-chip__label">{label()}</span>
    </a>
  );
}

// ── Internal: avatar ──────────────────────────────────────────────────────────

function MentionAvatar(p: { pictureUrl?: string; label: string }): JSX.Element {
  const initial = (): string =>
    (p.label.replace(/^@/, "").slice(0, 1) || "?").toUpperCase();

  return (
    <span class="nostr-mention-chip__avatar" aria-hidden="true">
      <Show
        when={p.pictureUrl}
        fallback={<span class="nostr-mention-chip__avatar-fallback">{initial()}</span>}
      >
        {(url) => (
          <img
            class="nostr-mention-chip__avatar-img"
            src={url()}
            alt=""
            loading="lazy"
            width={16}
            height={16}
          />
        )}
      </Show>
    </span>
  );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Abbreviates a `nostr:npub1…` URI to `npub1xxxx…yyyy` for display. */
function abbreviate(uri: string): string {
  // Strip the `nostr:` scheme prefix when present.
  const id = uri.startsWith("nostr:") ? uri.slice(6) : uri;
  return id.length > 16 ? `${id.slice(0, 8)}…${id.slice(-4)}` : id;
}
