/**
 * NostrContentView — web (SolidJS) Nostr content tree renderer.
 *
 * Renders a decoded `ContentTree` from the NMP kernel's NFCT projection.
 * Install the NMP SDK's FlatBuffers decode layer alongside this component;
 * the decode step converts `ContentTreeWire` bytes into the `ContentTree`
 * interface this component accepts.
 *
 * Stage 0 node coverage: Text, Paragraph, Heading, BlockQuote, CodeBlock,
 * List, Rule, Emphasis, Strong, InlineCode, Link, Mention, EventRef, Hashtag,
 * Url, Emoji, Media, Image, Invoice, Placeholder, SoftBreak, HardBreak.
 *
 * Mention display-name resolution and embed-card rendering (EventRef) are
 * deferred to later stages; Stage 0 renders the raw `nostr:…` URI.
 *
 * Install: nmp add web/content-view
 * Dependencies: solid-js
 */
import type { JSX } from "solid-js";
import { For, Show } from "solid-js";

// ── Content tree shape ────────────────────────────────────────────────────────
// These interfaces mirror the NFCT (nmp-content) FlatBuffers wire format.
// Your decode layer converts ContentTreeWire FlatBuffers objects to this shape.

export const NodeKind = {
  Text: 0, Mention: 1, EventRef: 2, Hashtag: 3, Url: 4, Media: 5,
  Emoji: 6, Invoice: 7, Heading: 8, Paragraph: 9, BlockQuote: 10,
  CodeBlock: 11, List: 12, Rule: 13, Emphasis: 14, Strong: 15,
  InlineCode: 16, Link: 17, Image: 18, SoftBreak: 19, HardBreak: 20,
  Placeholder: 21,
} as const;

export type NodeKindValue = (typeof NodeKind)[keyof typeof NodeKind];

/** Decoded Nostr URI for Mention and EventRef nodes. */
export type NostrUri = {
  /** Full `nostr:…` URI string (npub1…, nevent1…, naddr1…). */
  uri: string;
  /** 0 = Profile, 1 = Event, 2 = Address */
  kind: 0 | 1 | 2;
  primaryId?: string;
};

/** A decoded list item — holds node indices into the parent tree. */
export type ListItemDef = {
  children: number[];
};

/** A single decoded content node from the NFCT arena. */
export type ContentNode = {
  kind: NodeKindValue;
  text?: string;
  url?: string;
  tag?: string;
  href?: string;
  alt?: string;
  imgTitle?: string;
  level?: number;
  shortcode?: string;
  emojiUrl?: string;
  codeInfo?: string;
  /** -1 = unordered; 0+ = ordered list starting at this value. */
  orderedStart?: number;
  nostrUri?: NostrUri;
  mediaUrls?: string[];
  /** Child node indices into `ContentTree.nodes`. */
  children?: number[];
  listItems?: ListItemDef[];
  invoicePayload?: string;
  placeholderText?: string;
};

/** Decoded content tree — flat arena with indexed roots. */
export type ContentTree = {
  nodes: ContentNode[];
  roots: number[];
};

// ── Component props ───────────────────────────────────────────────────────────

export type NostrContentViewProps = {
  /** Decoded content tree. When absent, `fallback` is rendered verbatim. */
  tree?: ContentTree;
  /** Plain-text fallback for when no NFCT tree is available. */
  fallback?: string;
};

// ── Public component ──────────────────────────────────────────────────────────

export function NostrContentView(props: NostrContentViewProps): JSX.Element {
  return (
    <Show
      when={props.tree && props.tree.roots.length > 0}
      fallback={<>{props.fallback ?? ""}</>}
    >
      <TreeRoots tree={props.tree!} />
    </Show>
  );
}

// ── Internal: tree walker ─────────────────────────────────────────────────────

function TreeRoots(p: { tree: ContentTree }): JSX.Element {
  return (
    <For each={p.tree.roots}>
      {(idx) => {
        const node = p.tree.nodes[idx];
        return node ? <RenderNode node={node} tree={p.tree} /> : null;
      }}
    </For>
  );
}

function Children(p: { node: ContentNode; tree: ContentTree }): JSX.Element {
  const idxs = p.node.children ?? [];
  return (
    <For each={idxs}>
      {(idx) => {
        const child = p.tree.nodes[idx];
        return child ? <RenderNode node={child} tree={p.tree} /> : null;
      }}
    </For>
  );
}

function RenderNode(p: { node: ContentNode; tree: ContentTree }): JSX.Element {
  switch (p.node.kind) {
    case NodeKind.Text:        return <>{p.node.text ?? ""}</>;
    case NodeKind.SoftBreak:   return <>{" "}</>;
    case NodeKind.HardBreak:   return <br />;
    case NodeKind.Rule:        return <hr class="nostr-rule" />;
    case NodeKind.Mention:     return <MentionNode node={p.node} />;
    case NodeKind.EventRef:    return <EventRefNode node={p.node} />;
    case NodeKind.Hashtag:     return <HashtagNode node={p.node} />;
    case NodeKind.Url:         return <UrlNode node={p.node} />;
    case NodeKind.Emoji:       return <EmojiNode node={p.node} />;
    case NodeKind.Invoice:     return <InvoiceNode node={p.node} />;
    case NodeKind.Placeholder: return <PlaceholderNode node={p.node} />;
    case NodeKind.Emphasis:    return <em><Children node={p.node} tree={p.tree} /></em>;
    case NodeKind.Strong:      return <strong><Children node={p.node} tree={p.tree} /></strong>;
    case NodeKind.InlineCode:  return <code class="nostr-inline-code"><Children node={p.node} tree={p.tree} /></code>;
    case NodeKind.Link:        return <LinkNode node={p.node} tree={p.tree} />;
    case NodeKind.Paragraph:   return <p class="nostr-p"><Children node={p.node} tree={p.tree} /></p>;
    case NodeKind.Heading:     return <HeadingNode node={p.node} tree={p.tree} />;
    case NodeKind.BlockQuote:  return <blockquote class="nostr-blockquote"><Children node={p.node} tree={p.tree} /></blockquote>;
    case NodeKind.CodeBlock:   return <CodeBlockNode node={p.node} tree={p.tree} />;
    case NodeKind.List:        return <ListBlockNode node={p.node} tree={p.tree} />;
    case NodeKind.Media:       return <MediaNode node={p.node} />;
    case NodeKind.Image:       return <ImageNode node={p.node} />;
    default:                   return <>{p.node.text ?? ""}</>;
  }
}

// ── Inline nodes ──────────────────────────────────────────────────────────────

function MentionNode(p: { node: ContentNode }): JSX.Element {
  const uri = p.node.nostrUri?.uri ?? p.node.text ?? "";
  const label = p.node.text ?? uri;
  return <a class="nostr-mention" href={uri} rel="noopener noreferrer">{label}</a>;
}

function EventRefNode(p: { node: ContentNode }): JSX.Element {
  const uri = p.node.nostrUri?.uri ?? p.node.text ?? "";
  return <a class="nostr-event-ref" href={uri} rel="noopener noreferrer">{uri}</a>;
}

function HashtagNode(p: { node: ContentNode }): JSX.Element {
  return <span class="nostr-hashtag">#{p.node.tag ?? p.node.text ?? ""}</span>;
}

function UrlNode(p: { node: ContentNode }): JSX.Element {
  const url = p.node.url ?? p.node.text ?? "";
  return <a class="nostr-url" href={url} rel="noopener noreferrer" target="_blank">{url}</a>;
}

function EmojiNode(p: { node: ContentNode }): JSX.Element {
  const code = p.node.shortcode ?? p.node.text ?? "";
  return (
    <Show when={p.node.emojiUrl} fallback={<span class="nostr-emoji-text">:{code}:</span>}>
      {(url) => <img class="nostr-emoji" src={url()} alt={`:${code}:`} loading="lazy" />}
    </Show>
  );
}

function InvoiceNode(p: { node: ContentNode }): JSX.Element {
  return <span class="nostr-invoice" title={p.node.invoicePayload ?? ""}>⚡ Lightning invoice</span>;
}

function PlaceholderNode(p: { node: ContentNode }): JSX.Element {
  return <span class="nostr-placeholder">{p.node.placeholderText ?? "…"}</span>;
}

function LinkNode(p: { node: ContentNode; tree: ContentTree }): JSX.Element {
  const href = p.node.href ?? p.node.url ?? "#";
  return (
    <a class="nostr-link" href={href} rel="noopener noreferrer" target="_blank">
      <Children node={p.node} tree={p.tree} />
    </a>
  );
}

// ── Block nodes ───────────────────────────────────────────────────────────────

function HeadingNode(p: { node: ContentNode; tree: ContentTree }): JSX.Element {
  const ch = <Children node={p.node} tree={p.tree} />;
  switch (p.node.level) {
    case 1: return <h1 class="nostr-h">{ch}</h1>;
    case 2: return <h2 class="nostr-h">{ch}</h2>;
    case 3: return <h3 class="nostr-h">{ch}</h3>;
    case 4: return <h4 class="nostr-h">{ch}</h4>;
    case 5: return <h5 class="nostr-h">{ch}</h5>;
    default: return <h6 class="nostr-h">{ch}</h6>;
  }
}

function CodeBlockNode(p: { node: ContentNode; tree: ContentTree }): JSX.Element {
  return (
    <pre class="nostr-code-block">
      <Show when={p.node.codeInfo}>
        {(lang) => <span class="nostr-code-lang">{lang()}</span>}
      </Show>
      <code><Children node={p.node} tree={p.tree} /></code>
    </pre>
  );
}

function ListBlockNode(p: { node: ContentNode; tree: ContentTree }): JSX.Element {
  const isOrdered = (p.node.orderedStart ?? -1) >= 0;
  const start = isOrdered ? (p.node.orderedStart ?? 1) : undefined;
  const items = p.node.listItems ?? [];
  const renderItem = (item: ListItemDef): JSX.Element => (
    <li class="nostr-list-item">
      <For each={item.children}>
        {(idx) => {
          const child = p.tree.nodes[idx];
          return child ? <RenderNode node={child} tree={p.tree} /> : null;
        }}
      </For>
    </li>
  );
  return isOrdered
    ? <ol class="nostr-list" start={start}><For each={items}>{renderItem}</For></ol>
    : <ul class="nostr-list"><For each={items}>{renderItem}</For></ul>;
}

function MediaNode(p: { node: ContentNode }): JSX.Element {
  const urls = p.node.mediaUrls ?? [];
  return (
    <div class="nostr-media-grid" data-count={urls.length}>
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

function ImageNode(p: { node: ContentNode }): JSX.Element {
  return <img class="nostr-image" src={p.node.url ?? ""} alt={p.node.alt ?? ""} loading="lazy" />;
}
