/**
 * NostrMinimalContentView — minimal inline content renderer for the web. The
 * simplest component that gets you a working timeline cell: it walks a decoded
 * `ContentTreeWire` and renders inline runs (text, mentions, event refs,
 * hashtags, URLs, emphasis/strong) as a single flowing line, ignoring block
 * structure (no paragraphs, headings, lists). For full block layout use
 * `content-view`'s `NostrContentView`.
 *
 * Pure renderer (D6/D7): never parses, fetches, or mocks. Falls back to the raw
 * `fallback` string verbatim when no tree is present. Mirrors the SwiftUI/TUI
 * `content-minimal` renderers.
 */
import type { JSX } from "solid-js";
import { For, Show } from "solid-js";
import { type ContentTreeWire, type WireNode, WireNodeKind } from "../content-core/decodeContentTree";

function InlineNode(p: { node: WireNode; tree: ContentTreeWire }): JSX.Element {
  switch (p.node.kind()) {
    case WireNodeKind.Text:
      return <>{p.node.text() ?? ""}</>;
    case WireNodeKind.SoftBreak:
    case WireNodeKind.HardBreak:
      return <>{" "}</>;
    case WireNodeKind.Hashtag:
      return <span class="nostr-hashtag">#{p.node.tag() ?? p.node.text() ?? ""}</span>;
    case WireNodeKind.Url:
    case WireNodeKind.AdCandidateUrl:
      return (
        <a class="nostr-url" href={p.node.url() ?? "#"} rel="noopener noreferrer" target="_blank">
          {p.node.url() ?? p.node.text() ?? ""}
        </a>
      );
    case WireNodeKind.Mention:
    case WireNodeKind.EventRef: {
      const uri = p.node.nostrUri()?.uri() ?? p.node.text() ?? "";
      return <a class="nostr-mention" href={uri} rel="noopener noreferrer">{p.node.text() ?? uri}</a>;
    }
    case WireNodeKind.Emphasis:
      return <em><InlineChildren node={p.node} tree={p.tree} /></em>;
    case WireNodeKind.Strong:
      return <strong><InlineChildren node={p.node} tree={p.tree} /></strong>;
    default:
      // Block containers (Paragraph, Heading, …) flatten to their inline content
      // with a trailing space so the minimal view stays single-flow.
      return (
        <>
          <InlineChildren node={p.node} tree={p.tree} />
          {" "}
        </>
      );
  }
}

function InlineChildren(p: { node: WireNode; tree: ContentTreeWire }): JSX.Element {
  const idxs = Array.from({ length: p.node.childrenLength() }, (_, i) => p.node.children(i) ?? 0);
  return (
    <For each={idxs}>
      {(idx) => {
        const child = p.tree.nodes(idx);
        return child ? <InlineNode node={child} tree={p.tree} /> : null;
      }}
    </For>
  );
}

export function NostrMinimalContentView(props: {
  tree?: ContentTreeWire;
  fallback?: string;
}): JSX.Element {
  return (
    <Show when={props.tree && props.tree.rootsLength() > 0} fallback={<>{props.fallback ?? ""}</>}>
      <span class="nostr-minimal-content">
        <For each={Array.from({ length: props.tree!.rootsLength() }, (_, i) => props.tree!.roots(i) ?? 0)}>
          {(idx) => {
            const node = props.tree!.nodes(idx);
            return node ? <InlineNode node={node} tree={props.tree!} /> : null;
          }}
        </For>
      </span>
    </Show>
  );
}
