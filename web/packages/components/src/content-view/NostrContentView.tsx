/**
 * NostrContentView — renders a kernel-decoded `ContentTreeWire` (NFCT) as HTML.
 *
 * The kernel parses raw NIP-01/NIP-23 content into a `ContentTreeWire` (the
 * `nmp-content` tokenizer, behind the kernel's content-parser seam) and ships it
 * in the snapshot. This component is a pure walker over that tree — it never
 * parses, fetches, or mocks anything. When no tree is present it renders the
 * raw `fallback` string verbatim (honest-empty / keep-last-good per D6).
 *
 *   • Text, Paragraph, Heading, BlockQuote, CodeBlock, List, Rule nodes.
 *   • Inline formatting: Emphasis, Strong, InlineCode, Link, SoftBreak, HardBreak.
 *   • Mention / EventRef → `nostr:…` URI rendered as an anchor (no embed card).
 *   • Hashtag → `#tag` styled chip.
 *   • Url → plain anchor.
 *   • Emoji → shortcode text or <img> when emojiUrl is present.
 *   • Media / Image → <img> per URL or fallback link text.
 *   • Invoice / Placeholder → plain labelled text (no payment UI).
 *
 * Embed cards (resolved quoted events, mention chips with profile data) are a
 * separate component layer; this view renders their `nostr:…` URIs as anchors.
 */
import type { JSX } from "solid-js";
import { For, Show } from "solid-js";
import type { ContentTreeWire } from "@nmp/wire-ts/nmp/content/content-tree-wire";
import type { WireNode } from "@nmp/wire-ts/nmp/content/wire-node";
import type { ListItem } from "@nmp/wire-ts/nmp/content/list-item";
import { WireNodeKind } from "@nmp/wire-ts/nmp/content/wire-node-kind";

// ── Public types ──────────────────────────────────────────────────────────────

export type NostrContentViewProps = {
  /** Decoded NFCT tree — when absent `fallback` is rendered verbatim. */
  tree?: ContentTreeWire;
  /** Plain-text fallback (raw NIP-01 content string). */
  fallback?: string;
};

// ── Public component ──────────────────────────────────────────────────────────

/**
 * Renders a `ContentTreeWire` from the kernel. Falls back to the raw content
 * string when no tree is available (keep-last-good / honest-empty per D6).
 *
 * The root element is intentionally absent — callers wrap this in whatever
 * container carries their `data-testid` attribute.
 */
export function NostrContentView(props: NostrContentViewProps): JSX.Element {
  return (
    <Show when={props.tree && props.tree.rootsLength() > 0} fallback={<>{props.fallback ?? ""}</>}>
      <TreeRoots tree={props.tree!} />
    </Show>
  );
}

// ── Tree root walker ──────────────────────────────────────────────────────────

function TreeRoots(props: { tree: ContentTreeWire }): JSX.Element {
  const rootIdxs = Array.from(
    { length: props.tree.rootsLength() },
    (_, i) => props.tree.roots(i) ?? 0,
  );
  return (
    <For each={rootIdxs}>
      {(idx) => {
        const node = props.tree.nodes(idx);
        return node ? <RenderNode node={node} tree={props.tree} /> : null;
      }}
    </For>
  );
}

// ── Children helper ───────────────────────────────────────────────────────────

function Children(p: { node: WireNode; tree: ContentTreeWire }): JSX.Element {
  const idxs = Array.from(
    { length: p.node.childrenLength() },
    (_, i) => p.node.children(i) ?? 0,
  );
  return (
    <For each={idxs}>
      {(idx) => {
        const child = p.tree.nodes(idx);
        return child ? <RenderNode node={child} tree={p.tree} /> : null;
      }}
    </For>
  );
}

// ── Node dispatcher ───────────────────────────────────────────────────────────

function RenderNode(p: { node: WireNode; tree: ContentTreeWire }): JSX.Element {
  switch (p.node.kind()) {
    // ── Leaf inline nodes ────────────────────────────────────────────────────
    case WireNodeKind.Text:
      return <>{p.node.text() ?? ""}</>;
    case WireNodeKind.SoftBreak:
      return <>{" "}</>;
    case WireNodeKind.HardBreak:
      return <br />;
    case WireNodeKind.Rule:
      return <hr class="nostr-rule" />;

    // ── Nostr-aware inline nodes ─────────────────────────────────────────────
    case WireNodeKind.Mention:
      return <MentionNode node={p.node} />;
    case WireNodeKind.EventRef:
      return <EventRefNode node={p.node} />;
    case WireNodeKind.Hashtag:
      return <HashtagNode node={p.node} />;
    case WireNodeKind.Url:
      return <UrlNode node={p.node} />;
    case WireNodeKind.Emoji:
      return <EmojiNode node={p.node} />;
    case WireNodeKind.Invoice:
      return <InvoiceNode node={p.node} />;
    case WireNodeKind.Placeholder:
      return <PlaceholderNode node={p.node} />;

    // ── Inline containers ────────────────────────────────────────────────────
    case WireNodeKind.Emphasis:
      return <em><Children node={p.node} tree={p.tree} /></em>;
    case WireNodeKind.Strong:
      return <strong><Children node={p.node} tree={p.tree} /></strong>;
    case WireNodeKind.InlineCode:
      return <code class="nostr-inline-code"><Children node={p.node} tree={p.tree} /></code>;
    case WireNodeKind.Link:
      return <LinkNode node={p.node} tree={p.tree} />;

    // ── Block containers ─────────────────────────────────────────────────────
    case WireNodeKind.Paragraph:
      return <p class="nostr-p"><Children node={p.node} tree={p.tree} /></p>;
    case WireNodeKind.Heading:
      return <HeadingNode node={p.node} tree={p.tree} />;
    case WireNodeKind.BlockQuote:
      return <blockquote class="nostr-blockquote"><Children node={p.node} tree={p.tree} /></blockquote>;
    case WireNodeKind.CodeBlock:
      return <CodeBlockNode node={p.node} tree={p.tree} />;
    case WireNodeKind.List:
      return <ListNode node={p.node} tree={p.tree} />;

    // ── Media block nodes ────────────────────────────────────────────────────
    case WireNodeKind.Media:
      return <MediaNode node={p.node} />;
    case WireNodeKind.Image:
      return <ImageNode node={p.node} />;

    default:
      // Unknown future nodes: emit text() if present, otherwise nothing.
      return <>{p.node.text() ?? ""}</>;
  }
}

// ── Inline node implementations ───────────────────────────────────────────────

/** Renders the raw `nostr:npub1…` URI as an anchor. Display-name resolution and
 *  avatar chips are a separate embed-component layer. */
function MentionNode(p: { node: WireNode }): JSX.Element {
  const uri = p.node.nostrUri()?.uri() ?? p.node.text() ?? "";
  const label = p.node.text() ?? uri;
  return (
    <a class="nostr-mention" href={uri} rel="noopener noreferrer">
      {label}
    </a>
  );
}

/** Renders the raw `nostr:nevent1…` / `nostr:naddr1…` URI as a link. Embed
 *  cards (resolved quoted events) are a separate component layer. */
function EventRefNode(p: { node: WireNode }): JSX.Element {
  const uri = p.node.nostrUri()?.uri() ?? p.node.text() ?? "";
  return (
    <a class="nostr-event-ref" href={uri} rel="noopener noreferrer">
      {uri}
    </a>
  );
}

function HashtagNode(p: { node: WireNode }): JSX.Element {
  const tag = p.node.tag() ?? p.node.text() ?? "";
  return <span class="nostr-hashtag">#{tag}</span>;
}

function UrlNode(p: { node: WireNode }): JSX.Element {
  const url = p.node.url() ?? p.node.text() ?? "";
  return (
    <a class="nostr-url" href={url} rel="noopener noreferrer" target="_blank">
      {url}
    </a>
  );
}

function EmojiNode(p: { node: WireNode }): JSX.Element {
  const emojiUrl = p.node.emojiUrl();
  const shortcode = p.node.shortcode() ?? p.node.text() ?? "";
  return (
    <Show when={emojiUrl} fallback={<span class="nostr-emoji-text">:{shortcode}:</span>}>
      {(url) => (
        <img
          class="nostr-emoji"
          src={url()}
          alt={`:${shortcode}:`}
          title={`:${shortcode}:`}
          loading="lazy"
        />
      )}
    </Show>
  );
}

function InvoiceNode(p: { node: WireNode }): JSX.Element {
  const payload = p.node.invoicePayload();
  return (
    <span class="nostr-invoice" title={payload ?? ""}>
      ⚡ Lightning invoice
    </span>
  );
}

function PlaceholderNode(p: { node: WireNode }): JSX.Element {
  const text = p.node.text() ?? "…";
  return <span class="nostr-placeholder">{text}</span>;
}

function LinkNode(p: { node: WireNode; tree: ContentTreeWire }): JSX.Element {
  const href = p.node.href() ?? p.node.url() ?? "#";
  return (
    <a class="nostr-link" href={href} rel="noopener noreferrer" target="_blank">
      <Children node={p.node} tree={p.tree} />
    </a>
  );
}

// ── Block node implementations ────────────────────────────────────────────────

function HeadingNode(p: { node: WireNode; tree: ContentTreeWire }): JSX.Element {
  const children = <Children node={p.node} tree={p.tree} />;
  switch (p.node.level()) {
    case 1: return <h1 class="nostr-h">{children}</h1>;
    case 2: return <h2 class="nostr-h">{children}</h2>;
    case 3: return <h3 class="nostr-h">{children}</h3>;
    case 4: return <h4 class="nostr-h">{children}</h4>;
    case 5: return <h5 class="nostr-h">{children}</h5>;
    default: return <h6 class="nostr-h">{children}</h6>;
  }
}

function CodeBlockNode(p: { node: WireNode; tree: ContentTreeWire }): JSX.Element {
  const lang = p.node.codeInfo();
  return (
    <pre class="nostr-code-block">
      <Show when={lang}>
        {(l) => <span class="nostr-code-lang">{l()}</span>}
      </Show>
      <code><Children node={p.node} tree={p.tree} /></code>
    </pre>
  );
}

function ListNode(p: { node: WireNode; tree: ContentTreeWire }): JSX.Element {
  const orderedStart = p.node.orderedStart();
  const isOrdered = orderedStart >= BigInt(0);
  const items: (ListItem | null)[] = Array.from(
    { length: p.node.listItemsLength() },
    (_, i) => p.node.listItems(i),
  );
  const renderItem = (item: ListItem | null): JSX.Element => {
    if (!item) return null;
    const childIdxs = Array.from(
      { length: item.childrenLength() },
      (_, i) => item.children(i) ?? 0,
    );
    return (
      <li class="nostr-list-item">
        <For each={childIdxs}>
          {(idx) => {
            const child = p.tree.nodes(idx);
            return child ? <RenderNode node={child} tree={p.tree} /> : null;
          }}
        </For>
      </li>
    );
  };
  return isOrdered
    ? (
      <ol class="nostr-list" start={Number(orderedStart)}>
        <For each={items}>{renderItem}</For>
      </ol>
    )
    : (
      <ul class="nostr-list">
        <For each={items}>{renderItem}</For>
      </ul>
    );
}

function MediaNode(p: { node: WireNode }): JSX.Element {
  const count = p.node.mediaUrlsLength();
  const urls = Array.from({ length: count }, (_, i) => p.node.mediaUrls(i) as string);
  return (
    <div class="nostr-media-grid" data-count={count}>
      <For each={urls}>
        {(url) => (
          <a class="nostr-media-item" href={url} rel="noopener noreferrer" target="_blank">
            <img class="nostr-media-img" src={url} alt="" loading="lazy" />
          </a>
        )}
      </For>
    </div>
  );
}

function ImageNode(p: { node: WireNode }): JSX.Element {
  const url = p.node.url() ?? "";
  const alt = p.node.alt() ?? "";
  return <img class="nostr-image" src={url} alt={alt} loading="lazy" />;
}
