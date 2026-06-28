/**
 * NostrKindRegistry — kind-dispatch for embedded Nostr events on the web.
 *
 * `NostrEmbeddedEvent` takes a fully *resolved* `EmbeddedEventEnvelope` —
 * produced by the Rust kernel/composition root (`nmp-content`'s
 * `resolve_embed_projection`) from authoritative `refs.event` rows. The envelope's
 * `projection` is already kind-dispatched in Rust (`{ variant, data }`), so the
 * web registry only chooses the renderer and maps the pre-resolved projection
 * fields into each card's typed model — it NEVER re-parses raw NIP-23 / NIP-84
 * tags. This is the web twin of the SwiftUI/TUI `NostrKindRegistry` +
 * `EmbeddedEvent` dispatch table, consuming the same `nmp-content` resolver
 * output decoded from the Rust-owned `refs.event.envelopes` / NEMB sidecar.
 *
 * Pure (D7): the host owns the claim/resolve lifecycle and passes a fully
 * resolved envelope; the registry only chooses the renderer and projects the
 * Rust-resolved fields into each card's model.
 */
import type { JSX } from "solid-js";
import { createContext, Match, Switch, useContext } from "solid-js";
import { NostrArticleCard, type NostrArticleCardModel } from "../content-kind-30023/NostrArticleCard";
import { NostrHighlightCard, type NostrHighlightCardModel } from "../content-kind-9802/NostrHighlightCard";
import { NostrQuoteCard, type NostrQuoteCardModel } from "../content-quote-card/NostrQuoteCard";

/**
 * The kernel-resolved per-kind projection. Mirrors the Rust
 * `nmp-content` `EmbedKindProjection` serde *enum* shape exactly:
 * `{ "variant": …, "data": … }` (serde `tag = "variant", content = "data",
 * rename_all = "camelCase"`) after the web runtime decodes the Rust-generated
 * `refs.event.envelopes` NEMB FlatBuffer.
 */
export type EmbedKindProjection =
  | { variant: "shortNote"; data: ShortNoteProjection }
  | { variant: "article"; data: ArticleProjection }
  | { variant: "highlight"; data: HighlightProjection }
  | { variant: "profile"; data: ProfileProjection }
  | { variant: "unknown"; data: UnknownProjection };

/**
 * One node of the Rust-parsed content tree (`nmp-content`'s `ContentTreeWire`),
 * in its serde-JSON shape: a tagged union `{ "kind": "text", "text": … }`
 * (`tag = "kind", rename_all = "snake_case"`). Only the fields the web preview
 * walker reads are typed; renderer-complete trees are decoded from NFCT bytes
 * carried by `refs.event` rows or feed projections (`NostrContentView`).
 */
export type WireNode = {
  kind: string;
  text?: string;
  tag?: string;
  url?: string;
  code?: string;
};

export type ContentTreeWire = {
  nodes: WireNode[];
  roots?: number[];
};

export type ShortNoteProjection = {
  id: string;
  authorPubkey: string;
  authorDisplayName?: string | null;
  authorPictureUrl?: string | null;
  createdAt: number;
  contentTree: ContentTreeWire;
  mediaUrls: string[];
};

export type ArticleProjection = {
  id: string;
  authorPubkey: string;
  authorDisplayName?: string | null;
  authorPictureUrl?: string | null;
  createdAt: number;
  title?: string | null;
  summary?: string | null;
  heroImageUrl?: string | null;
  dTag: string;
};

export type HighlightProjection = {
  id: string;
  authorPubkey: string;
  authorDisplayName?: string | null;
  createdAt: number;
  highlightedText: string;
  sourceEventId?: string | null;
  sourceEventAddr?: string | null;
  sourceUrl?: string | null;
  context?: string | null;
};

export type ProfileProjection = {
  pubkey: string;
  displayName?: string | null;
  pictureUrl?: string | null;
  about?: string | null;
  nip05?: string | null;
  lud16?: string | null;
  bannerUrl?: string | null;
};

export type UnknownProjection = {
  kind: number;
  authorPubkey: string;
  authorDisplayName?: string | null;
  authorPictureUrl?: string | null;
  createdAt: number;
  content: string;
  tags: string[][];
  altText?: string | null;
};

/**
 * A fully resolved embedded-event envelope. Hosts receive this render envelope
 * from Rust-owned composition over authoritative `refs.event` rows (browser and
 * native typed-frame shells decode the `refs.event.envelopes` / NEMB sidecar).
 */
export type EmbeddedEventModel = {
  uri: string;
  primaryId: string;
  /** The Rust-dispatched per-kind projection (drives renderer choice). */
  projection: EmbedKindProjection;
  collapsed: boolean;
  collapseReason?: string | null;
};

export type NostrEmbeddedEventProps = {
  event: EmbeddedEventModel;
  /** Current unix-seconds, forwarded to the quote card's relative-time label. */
  nowSeconds: number;
  /**
   * Host-resolved author byline (name + picture). The kernel-resolved
   * projection carries `None` for the author on the non-Profile variants by
   * design — the displaying host resolves the byline from its live
   * `refs.profile` store and threads it here. Omit it for the highlight card,
   * which has no byline. See {@link EmbedAuthor}.
   */
  author?: EmbedAuthor;
};

export interface NostrKindRegistry {
  renderEvent(props: NostrEmbeddedEventProps): JSX.Element;
}

const DEFAULT_NOSTR_KIND_REGISTRY: NostrKindRegistry = {
  renderEvent: (props) => <DefaultNostrEmbeddedEvent {...props} />,
};

const NostrKindRegistryContext = createContext<NostrKindRegistry>();

export function createDefaultNostrKindRegistry(): NostrKindRegistry {
  return DEFAULT_NOSTR_KIND_REGISTRY;
}

export function NostrKindRegistryProvider(props: {
  registry: NostrKindRegistry;
  children: JSX.Element;
}): JSX.Element {
  return (
    <NostrKindRegistryContext.Provider value={props.registry}>
      {props.children}
    </NostrKindRegistryContext.Provider>
  );
}

export function useOptionalNostrKindRegistry(): NostrKindRegistry | undefined {
  return useContext(NostrKindRegistryContext);
}

export function useNostrKindRegistry(): NostrKindRegistry {
  return useOptionalNostrKindRegistry() ?? DEFAULT_NOSTR_KIND_REGISTRY;
}

/** Coalesce nullable optional strings to `undefined` for the card models. */
function opt(value: string | null | undefined): string | undefined {
  return value ?? undefined;
}

/**
 * Flatten a Rust-parsed content tree into a plain-text preview for the quote
 * card. The kernel already tokenized the note (`nmp-content`); we only collect
 * the literal text runs (and hashtag/url/code leaves) in arena order — the rich
 * tree itself is rendered elsewhere via the typed `NostrContentView`. This is
 * the web twin of the SwiftUI quote card's plain-text fallback.
 */
function contentTreePreview(tree: ContentTreeWire | undefined): string {
  if (!tree || !Array.isArray(tree.nodes)) return "";
  const parts: string[] = [];
  for (const node of tree.nodes) {
    if (node.kind === "text" && node.text) parts.push(node.text);
    else if (node.kind === "hashtag" && node.tag) parts.push(`#${node.tag}`);
    else if (node.kind === "url" && node.url) parts.push(node.url);
    else if (node.kind === "inline_code" && node.code) parts.push(node.code);
  }
  return parts.join(" ").trim();
}

/**
 * Host-resolved author byline. The kernel-resolved projection intentionally
 * carries `None` for author name/picture on ShortNote/Article/Highlight (only
 * the kind:0 Profile variant carries a name), because the byline is resolved by
 * the *displaying* host against its live `refs.profile` store — NOT baked into
 * the projection's static field. The host threads it in here; the registry
 * prefers it and only falls back to the projection's field (so the Profile
 * variant, which does carry a name, stays correct).
 */
export type EmbedAuthor = { name?: string; picture?: string };

function toArticle(p: ArticleProjection, author: EmbedAuthor | undefined): NostrArticleCardModel {
  return {
    title: opt(p.title) ?? "(untitled)",
    image: opt(p.heroImageUrl),
    summary: opt(p.summary),
    authorName: author?.name ?? opt(p.authorDisplayName),
    authorPicture: author?.picture ?? opt(p.authorPictureUrl),
  };
}

function toHighlight(p: HighlightProjection): NostrHighlightCardModel {
  return {
    text: p.highlightedText,
    context: opt(p.context),
    sourceUrl: opt(p.sourceUrl),
    sourceEventId: opt(p.sourceEventId),
    sourceEventAddr: opt(p.sourceEventAddr),
  };
}

/**
 * Map a non-article/highlight projection onto the generic quote card. For a
 * ShortNote the body comes from the Rust-parsed content tree (flattened to a
 * text preview — the kernel already tokenized it); an Unknown carries its raw
 * `content` verbatim. A Profile renders its `about` text.
 */
function toQuote(
  projection: EmbedKindProjection,
  author: EmbedAuthor | undefined,
): NostrQuoteCardModel {
  switch (projection.variant) {
    case "shortNote":
      return {
        authorName: author?.name ?? opt(projection.data.authorDisplayName),
        authorPicture: author?.picture ?? opt(projection.data.authorPictureUrl),
        content: contentTreePreview(projection.data.contentTree),
        createdAt: projection.data.createdAt,
      };
    case "unknown":
      return {
        authorName: author?.name ?? opt(projection.data.authorDisplayName),
        authorPicture: author?.picture ?? opt(projection.data.authorPictureUrl),
        content: projection.data.content,
        createdAt: projection.data.createdAt,
      };
    case "profile":
      // The kind:0 Profile variant carries its own name/picture in the
      // projection; the host author override is for embeds whose byline is
      // resolved separately, so the projection's own fields win here.
      return {
        authorName: opt(projection.data.displayName) ?? author?.name,
        authorPicture: opt(projection.data.pictureUrl) ?? author?.picture,
        content: opt(projection.data.about) ?? "",
      };
    default:
      return { content: "" };
  }
}

export function NostrEmbeddedEvent(props: NostrEmbeddedEventProps): JSX.Element {
  return useNostrKindRegistry().renderEvent(props);
}

export function DefaultNostrEmbeddedEvent(props: NostrEmbeddedEventProps): JSX.Element {
  // `Match` narrows on its `when`, so the typed accessors below return the
  // correct projection payload (or `undefined` for the other variants, which
  // `<Show keyed>` gates the render on).
  const article = (): ArticleProjection | undefined =>
    props.event.projection.variant === "article" ? props.event.projection.data : undefined;
  const highlight = (): HighlightProjection | undefined =>
    props.event.projection.variant === "highlight" ? props.event.projection.data : undefined;
  return (
    <Switch
      fallback={
        <NostrQuoteCard
          quote={toQuote(props.event.projection, props.author)}
          nowSeconds={props.nowSeconds}
        />
      }
    >
      <Match when={article()} keyed>
        {(p) => <NostrArticleCard article={toArticle(p, props.author)} />}
      </Match>
      <Match when={highlight()} keyed>
        {(p) => <NostrHighlightCard highlight={toHighlight(p)} />}
      </Match>
    </Switch>
  );
}
