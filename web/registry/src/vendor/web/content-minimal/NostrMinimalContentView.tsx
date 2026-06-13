/**
 * NostrMinimalContentView — compact inline Nostr content renderer (web / SolidJS).
 *
 * Renders only text-flow nodes: plain text, mentions, hashtags, URLs, emoji,
 * and soft/hard breaks. Block-level structure (headings, lists, code blocks,
 * media) is omitted — the whole note collapses to a single inline run, suitable
 * for feed-row previews or notification summaries.
 *
 * Install: nmp add web/content-minimal
 * Dependencies: solid-js
 */
import type { JSX } from "solid-js";
import { For, Show } from "solid-js";

// ── Shared content-tree types (mirrors NFCT wire format) ─────────────────────

const NK = {
  Text: 0, Mention: 1, EventRef: 2, Hashtag: 3, Url: 4,
  Emoji: 6, SoftBreak: 19, HardBreak: 20,
} as const;

export type NostrUri = { uri: string; kind: 0 | 1 | 2; primaryId?: string };

export type MinimalNode = {
  kind: number;
  text?: string;
  url?: string;
  tag?: string;
  shortcode?: string;
  emojiUrl?: string;
  nostrUri?: NostrUri;
  children?: number[];
};

export type ContentTree = {
  nodes: MinimalNode[];
  roots: number[];
};

// ── Component props ───────────────────────────────────────────────────────────

export type NostrMinimalContentViewProps = {
  /** Decoded content tree. When absent, `fallback` is rendered verbatim. */
  tree?: ContentTree;
  /** Plain-text fallback for when no NFCT tree is available. */
  fallback?: string;
  /** Optional line-clamp CSS class applied to the wrapper span. */
  clampClass?: string;
};

// ── Public component ──────────────────────────────────────────────────────────

/**
 * Inline-only content renderer — no block structure, no media.
 * Renders the tree as a flat span of text/mention/hashtag/url runs.
 */
export function NostrMinimalContentView(props: NostrMinimalContentViewProps): JSX.Element {
  return (
    <Show
      when={props.tree && props.tree.roots.length > 0}
      fallback={<span class={props.clampClass}>{props.fallback ?? ""}</span>}
    >
      <span class={props.clampClass}>
        <InlineRoots tree={props.tree!} />
      </span>
    </Show>
  );
}

// ── Internal: inline-only tree walker ────────────────────────────────────────

function InlineRoots(p: { tree: ContentTree }): JSX.Element {
  return (
    <For each={p.tree.roots}>
      {(idx) => {
        const node = p.tree.nodes[idx];
        return node ? <InlineNode node={node} tree={p.tree} /> : null;
      }}
    </For>
  );
}

function InlineChildren(p: { node: MinimalNode; tree: ContentTree }): JSX.Element {
  return (
    <For each={p.node.children ?? []}>
      {(idx) => {
        const child = p.tree.nodes[idx];
        return child ? <InlineNode node={child} tree={p.tree} /> : null;
      }}
    </For>
  );
}

/**
 * Renders a node as inline. Block-level node kinds (Paragraph, Heading, etc.)
 * fall through to rendering their children inline — structure is flattened.
 */
function InlineNode(p: { node: MinimalNode; tree: ContentTree }): JSX.Element {
  switch (p.node.kind) {
    case NK.Text:
      return <>{p.node.text ?? ""}</>;
    case NK.SoftBreak:
      return <>{" "}</>;
    case NK.HardBreak:
      return <br />;
    case NK.Mention: {
      const uri = p.node.nostrUri?.uri ?? p.node.text ?? "";
      const label = p.node.text ?? uri;
      return <a class="nostr-mention" href={uri} rel="noopener noreferrer">{label}</a>;
    }
    case NK.EventRef: {
      const uri = p.node.nostrUri?.uri ?? p.node.text ?? "";
      return <a class="nostr-event-ref" href={uri} rel="noopener noreferrer">{uri}</a>;
    }
    case NK.Hashtag:
      return <span class="nostr-hashtag">#{p.node.tag ?? p.node.text ?? ""}</span>;
    case NK.Url: {
      const url = p.node.url ?? p.node.text ?? "";
      return <a class="nostr-url" href={url} rel="noopener noreferrer" target="_blank">{url}</a>;
    }
    case NK.Emoji: {
      const code = p.node.shortcode ?? p.node.text ?? "";
      return (
        <Show when={p.node.emojiUrl} fallback={<span class="nostr-emoji-text">:{code}:</span>}>
          {(url) => <img class="nostr-emoji" src={url()} alt={`:${code}:`} loading="lazy" />}
        </Show>
      );
    }
    default:
      // Block nodes (Paragraph, Heading, List, …) — render children inline.
      return p.node.children && p.node.children.length > 0
        ? <InlineChildren node={p.node} tree={p.tree} />
        : <>{p.node.text ?? ""}</>;
  }
}
