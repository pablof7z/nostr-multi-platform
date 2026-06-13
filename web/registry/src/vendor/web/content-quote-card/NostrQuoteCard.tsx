/**
 * NostrQuoteCard — quoted-note card (web / SolidJS).
 *
 * Renders a quoted Nostr event inline inside a note's content flow.
 *
 * Stage 0 contract: the kernel does NOT yet ship a `resolved_embeds`
 * projection. Pass `model` when the app has resolved the referenced event;
 * omit it (or pass `undefined`) to render a "collapsed" placeholder link.
 * Never renders fake/mocked data — honest degradation only.
 *
 * Later stages will wire `resolved_embeds` → `model` via a kernel projection;
 * this component's props contract is stable across that migration.
 *
 * Install: nmp add web/content-quote-card
 * Dependencies: solid-js
 */
import type { JSX } from "solid-js";
import { Show } from "solid-js";

// ── Model ─────────────────────────────────────────────────────────────────────

/** Resolved data for a quoted note — supplied by the app when available. */
export type NostrQuoteCardModel = {
  /** Raw hex event id of the quoted note. */
  eventId: string;
  /** Full `nostr:nevent1…` or `nostr:naddr1…` URI — used as the link href. */
  nostrUri: string;
  /** Kind:0 display name for the quoted note's author (absent until resolved). */
  authorDisplayName?: string;
  /** Kind:0 picture URL for the quoted note's author (absent until resolved). */
  authorPictureUrl?: string;
  /** Abbreviated pubkey label (e.g. `npub1xxxx…yyyy`) for pre-resolution display. */
  authorLabel: string;
  /** Preview text — a one-line excerpt of the quoted note's content. */
  contentPreview: string;
  /** Unix seconds created_at of the quoted event. */
  createdAt?: number;
};

// ── Props ─────────────────────────────────────────────────────────────────────

export type NostrQuoteCardProps = {
  /**
   * Full `nostr:nevent1…` / `nostr:naddr1…` URI from the EventRef node.
   * Always required — used as the fallback link even when `model` is absent.
   */
  nostrUri: string;
  /**
   * Resolved quote data — absent until the app resolves the referenced event.
   * When absent the card renders a collapsed placeholder with a raw URI link.
   */
  model?: NostrQuoteCardModel;
  /** Called when the user taps the card. */
  onTap?: (nostrUri: string) => void;
};

// ── Public component ──────────────────────────────────────────────────────────

/**
 * Quoted-note card. Two variants:
 * - **Rich** (when `model` is provided): author header + content preview.
 * - **Collapsed** (when `model` is absent): compact link to the raw URI.
 */
export function NostrQuoteCard(props: NostrQuoteCardProps): JSX.Element {
  const handleClick = (): void => {
    props.onTap?.(props.nostrUri);
  };

  return (
    <Show when={props.model} fallback={<CollapsedCard nostrUri={props.nostrUri} onTap={handleClick} />}>
      {(m) => <RichCard model={m()} onTap={handleClick} />}
    </Show>
  );
}

// ── Collapsed variant (Stage 0 — no resolved_embeds) ─────────────────────────

function CollapsedCard(p: { nostrUri: string; onTap: () => void }): JSX.Element {
  return (
    <a
      class="nostr-quote-card nostr-quote-card--collapsed"
      href={p.nostrUri}
      rel="noopener noreferrer"
      onClick={(e) => {
        e.preventDefault();
        p.onTap();
      }}
      aria-label="View quoted note"
    >
      <span class="nostr-quote-card__label">↗ View quote</span>
      <span class="nostr-quote-card__uri">{abbreviate(p.nostrUri)}</span>
    </a>
  );
}

// ── Rich variant (when resolved_embeds is wired) ──────────────────────────────

function RichCard(p: { model: NostrQuoteCardModel; onTap: () => void }): JSX.Element {
  const authorLabel = (): string =>
    p.model.authorDisplayName
      ? `@${p.model.authorDisplayName}`
      : `@${p.model.authorLabel}`;

  const timeLabel = (): string =>
    p.model.createdAt
      ? new Date(p.model.createdAt * 1000).toLocaleString()
      : "";

  return (
    <article
      class="nostr-quote-card nostr-quote-card--rich"
      role="article"
      aria-label={`Quoted note by ${authorLabel()}`}
    >
      <button
        type="button"
        class="nostr-quote-card__inner"
        onClick={p.onTap}
      >
        <header class="nostr-quote-card__header">
          <QuoteAuthorAvatar model={p.model} />
          <span class="nostr-quote-card__author">{authorLabel()}</span>
          <Show when={timeLabel()}>
            {(t) => <time class="nostr-quote-card__time">{t()}</time>}
          </Show>
        </header>
        <p class="nostr-quote-card__preview">{p.model.contentPreview}</p>
      </button>
    </article>
  );
}

function QuoteAuthorAvatar(p: { model: NostrQuoteCardModel }): JSX.Element {
  const initial = (): string =>
    (p.model.authorDisplayName ?? p.model.authorLabel).slice(0, 1).toUpperCase() || "?";
  return (
    <span class="nostr-quote-card__avatar" aria-hidden="true">
      <Show
        when={p.model.authorPictureUrl}
        fallback={
          <span class="nostr-quote-card__avatar-fallback">{initial()}</span>
        }
      >
        {(url) => (
          <img
            class="nostr-quote-card__avatar-img"
            src={url()}
            alt=""
            loading="lazy"
            width={20}
            height={20}
          />
        )}
      </Show>
    </span>
  );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function abbreviate(uri: string): string {
  const id = uri.startsWith("nostr:") ? uri.slice(6) : uri;
  return id.length > 20 ? `${id.slice(0, 10)}…${id.slice(-6)}` : id;
}
