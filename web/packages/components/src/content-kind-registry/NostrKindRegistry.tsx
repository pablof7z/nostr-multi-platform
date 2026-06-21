/**
 * NostrKindRegistry — kind-dispatch for embedded Nostr events on the web.
 *
 * `NostrEmbeddedEvent` takes a resolved event envelope (hydrated by the host from
 * a `claimed_events` entry: kind + content + tags + kernel-enriched author) and
 * dispatches to the matching per-kind card: kind:30023 → `NostrArticleCard`,
 * kind:9802 → `NostrHighlightCard`, everything else → `NostrQuoteCard` (the
 * generic short-note fallback). This is the web twin of the SwiftUI/TUI
 * `NostrKindRegistry` + `EmbeddedEvent` dispatch table.
 *
 * Pure (D7): the host owns the claim/resolve lifecycle and passes a fully
 * resolved envelope; the registry only chooses the renderer and projects the
 * raw tags into each card's typed model.
 */
import type { JSX } from "solid-js";
import { Match, Switch } from "solid-js";
import { NostrArticleCard, type NostrArticleCardModel } from "../content-kind-30023/NostrArticleCard";
import { NostrHighlightCard, type NostrHighlightCardModel } from "../content-kind-9802/NostrHighlightCard";
import { NostrQuoteCard, type NostrQuoteCardModel } from "../content-quote-card/NostrQuoteCard";

/** A resolved event envelope, host-hydrated from a `claimed_events` entry. */
export type EmbeddedEventModel = {
  kind: number;
  content: string;
  createdAt?: number;
  /** Raw event tags (array of tag rows). */
  tags: string[][];
  authorName?: string;
  authorPicture?: string;
};

/** First value of the first tag row whose name (index 0) matches `name`. */
function tag(model: EmbeddedEventModel, name: string): string | undefined {
  const row = model.tags.find((t) => t[0] === name);
  return row && row.length > 1 ? row[1] : undefined;
}

function toArticle(m: EmbeddedEventModel): NostrArticleCardModel {
  return {
    title: tag(m, "title") ?? "(untitled)",
    image: tag(m, "image"),
    summary: tag(m, "summary"),
    authorName: m.authorName,
    authorPicture: m.authorPicture,
  };
}

function toHighlight(m: EmbeddedEventModel): NostrHighlightCardModel {
  return {
    text: m.content,
    context: tag(m, "context"),
    sourceUrl: tag(m, "r"),
    sourceEventId: tag(m, "e"),
    sourceEventAddr: tag(m, "a"),
  };
}

function toQuote(m: EmbeddedEventModel): NostrQuoteCardModel {
  return {
    authorName: m.authorName,
    authorPicture: m.authorPicture,
    content: m.content,
    createdAt: m.createdAt,
  };
}

export function NostrEmbeddedEvent(props: {
  event: EmbeddedEventModel;
  /** Current unix-seconds, forwarded to the quote card's relative-time label. */
  nowSeconds: number;
}): JSX.Element {
  return (
    <Switch fallback={<NostrQuoteCard quote={toQuote(props.event)} nowSeconds={props.nowSeconds} />}>
      <Match when={props.event.kind === 30023}>
        <NostrArticleCard article={toArticle(props.event)} />
      </Match>
      <Match when={props.event.kind === 9802}>
        <NostrHighlightCard highlight={toHighlight(props.event)} />
      </Match>
    </Switch>
  );
}
