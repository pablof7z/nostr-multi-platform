/**
 * NostrHighlightCard — NIP-84 highlight (kind:9802) embed card for the web.
 *
 * Pure renderer (D7): the host hydrates a `NostrHighlightCardModel` from a
 * resolved `claimed_events` entry. Renders the highlighted text as a pull-quote
 * in a yellow-accented box, an optional surrounding `context` line, and a source
 * footer that branches on the highlight's `r` (URL), `e` (event id), or `a`
 * (addressable event) tag — in that priority order. Mirrors the SwiftUI/TUI
 * `HighlightEmbed`.
 */
import type { JSX } from "solid-js";
import { Show } from "solid-js";

export type NostrHighlightCardModel = {
  /** The highlighted text (event content). */
  text: string;
  /** Optional surrounding `context` tag. */
  context?: string;
  /** Source URL (`r` tag), if present. */
  sourceUrl?: string;
  /** Source event id (`e` tag), if present. */
  sourceEventId?: string;
  /** Source addressable coordinate (`a` tag), if present. */
  sourceEventAddr?: string;
};

function sourceLabel(m: NostrHighlightCardModel): string | undefined {
  if (m.sourceUrl) return m.sourceUrl;
  if (m.sourceEventId) return `nostr:${m.sourceEventId.slice(0, 12)}…`;
  if (m.sourceEventAddr) return m.sourceEventAddr;
  return undefined;
}

export function NostrHighlightCard(props: { highlight: NostrHighlightCardModel }): JSX.Element {
  return (
    <figure class="nostr-highlight-card">
      <Show when={props.highlight.context}>
        {(c) => <figcaption class="nostr-highlight-card__context">{c()}</figcaption>}
      </Show>
      <blockquote class="nostr-highlight-card__quote">{props.highlight.text}</blockquote>
      <Show when={sourceLabel(props.highlight)}>
        {(label) => <footer class="nostr-highlight-card__source">— {label()}</footer>}
      </Show>
    </figure>
  );
}
