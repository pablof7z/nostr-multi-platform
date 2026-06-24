import type { JSX } from "solid-js";
import { Show, createUniqueId, onCleanup, onMount } from "solid-js";

import { useNostrEventHost } from "../../components/content-event/NostrEventHost";
import {
  NostrQuoteCard,
  type NostrQuoteCardModel,
} from "../../components/content-quote-card/NostrQuoteCard";
import type { ClaimedEventWire } from "../refEventStore";
import { shortKey } from "../snapshot";
import type { ContentTreeWire } from "../generated/nmp/content/content-tree-wire";
import type { WireNode } from "../generated/nmp/content/wire-node";
import { WireNodeKind } from "../generated/nmp/content/wire-node-kind";

export function EventRefNode(props: { node: WireNode }): JSX.Element {
  const uri = props.node.nostrUri()?.uri() ?? props.node.text() ?? "";
  const primaryId = props.node.nostrUri()?.primaryId() ?? "";
  const author = props.node.nostrUri()?.author() ?? undefined;
  const hints = eventRelayHints(props.node);
  const host = useNostrEventHost();
  if (primaryId) {
    const consumerId = `nostr-event.${createUniqueId()}`;
    onMount(() => host.claimEvent(primaryId, consumerId, hints, author));
    onCleanup(() => host.releaseEvent(primaryId, consumerId));
  }
  const resolved = () => (primaryId ? host.event(primaryId) : undefined);
  return (
    <Show when={resolved()} fallback={<EventRefLink uri={uri} />}>
      {(event) => (
        <NostrQuoteCard
          quote={quoteModel(event())}
          nowSeconds={Math.floor(Date.now() / 1000)}
        />
      )}
    </Show>
  );
}

function EventRefLink(props: { uri: string }): JSX.Element {
  return (
    <a class="nostr-event-ref" href={props.uri} rel="noopener noreferrer">
      {props.uri}
    </a>
  );
}

function eventRelayHints(node: WireNode): string[] {
  const uri = node.nostrUri();
  if (!uri) return [];
  const hints: string[] = [];
  for (let i = 0; i < uri.relaysLength(); i += 1) {
    const relay = uri.relays(i);
    if (relay) hints.push(relay);
  }
  return hints;
}

function quoteModel(event: ClaimedEventWire): NostrQuoteCardModel {
  return {
    authorName: shortKey(event.authorPubkey),
    content: contentPreview(event.contentTree) || event.content,
    createdAt: event.createdAt || undefined,
  };
}

function contentPreview(tree: ContentTreeWire | undefined): string {
  if (!tree) return "";
  const parts: string[] = [];
  for (let i = 0; i < tree.nodesLength(); i += 1) {
    const node = tree.nodes(i);
    if (!node) continue;
    switch (node.kind()) {
      case WireNodeKind.Text: {
        const text = node.text();
        if (text) parts.push(text);
        break;
      }
      case WireNodeKind.Hashtag: {
        const tag = node.tag();
        if (tag) parts.push(`#${tag}`);
        break;
      }
      case WireNodeKind.Url: {
        const url = node.url();
        if (url) parts.push(url);
        break;
      }
      case WireNodeKind.InlineCode: {
        const code = node.text();
        if (code) parts.push(code);
        break;
      }
    }
  }
  return parts.join(" ").trim();
}
