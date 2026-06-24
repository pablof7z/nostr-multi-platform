import type { JSX } from "solid-js";
import { Show } from "solid-js";

export type NostrQuoteCardModel = {
  authorName?: string;
  authorPicture?: string;
  content: string;
  createdAt?: number;
};

export function relativeTime(createdAt: number, nowSeconds: number): string {
  const secs = Math.max(0, nowSeconds - createdAt);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo`;
  return `${Math.floor(months / 12)}y`;
}

export function NostrQuoteCard(props: {
  quote: NostrQuoteCardModel;
  nowSeconds: number;
}): JSX.Element {
  return (
    <div class="nostr-quote-card">
      <div class="nostr-quote-card__header">
        <Show
          when={props.quote.authorPicture}
          fallback={<span class="nostr-quote-card__avatar nostr-quote-card__avatar--empty" />}
        >
          {(pic) => <img class="nostr-quote-card__avatar" src={pic()} alt="" loading="lazy" />}
        </Show>
        <span class="nostr-quote-card__author">{props.quote.authorName ?? "unknown"}</span>
        <Show when={props.quote.createdAt}>
          {(ts) => (
            <span class="nostr-quote-card__time">{relativeTime(ts(), props.nowSeconds)}</span>
          )}
        </Show>
      </div>
      <p class="nostr-quote-card__content">{props.quote.content}</p>
    </div>
  );
}
