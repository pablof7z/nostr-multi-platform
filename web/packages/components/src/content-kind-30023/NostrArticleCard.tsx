/**
 * NostrArticleCard — NIP-23 long-form (kind:30023) embed card for the web.
 *
 * Pure renderer (D7): the host hydrates a `NostrArticleCardModel` from a
 * resolved `claimed_events` entry (the kernel decodes the kind:30023 event and
 * enriches the author's kind:0) and passes it in. The card never parses, fetches,
 * or mocks — it renders the `image` tag as a 16:9 hero, the `title` as the
 * headline, an optional `summary`, then an author byline (avatar + display name
 * + `article · kind:30023`). Mirrors the SwiftUI `ArticleEmbed` / Compose
 * `NostrArticleCard` layout.
 */
import type { JSX } from "solid-js";
import { Show } from "solid-js";

export type NostrArticleCardModel = {
  /** `title` tag. */
  title: string;
  /** `image` tag (hero), if present. */
  image?: string;
  /** `summary` tag, if present. */
  summary?: string;
  /** Resolved author display name (kind:0), if present. */
  authorName?: string;
  /** Resolved author picture URL (kind:0), if present. */
  authorPicture?: string;
};

export function NostrArticleCard(props: { article: NostrArticleCardModel }): JSX.Element {
  return (
    <article class="nostr-article-card">
      <Show when={props.article.image}>
        {(src) => (
          <div class="nostr-article-card__hero">
            <img src={src()} alt="" loading="lazy" />
          </div>
        )}
      </Show>
      <div class="nostr-article-card__body">
        <h3 class="nostr-article-card__title">{props.article.title}</h3>
        <Show when={props.article.summary}>
          {(s) => <p class="nostr-article-card__summary">{s()}</p>}
        </Show>
        <div class="nostr-article-card__byline">
          <Show
            when={props.article.authorPicture}
            fallback={<span class="nostr-article-card__avatar nostr-article-card__avatar--empty" />}
          >
            {(pic) => <img class="nostr-article-card__avatar" src={pic()} alt="" loading="lazy" />}
          </Show>
          <span class="nostr-article-card__author">{props.article.authorName ?? "unknown"}</span>
          <span class="nostr-article-card__kind">article · kind:30023</span>
        </div>
      </div>
    </article>
  );
}
