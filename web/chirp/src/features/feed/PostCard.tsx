// PostCard.tsx — render a single home-feed post card.
//
// Pure presentation: receives a `FeedRow` (decoded from the kernel's nmp.feed.home
// OpFeedSnapshot) and renders it. Zero Nostr protocol logic — no event construction,
// no signing, no relay framing. Profile resolution is delegated to the ambient
// NostrProfileHost (which the kernel resolves via the refs.profile projection).

import { Show, createUniqueId } from "solid-js";
import type { FeedRow } from "../../nmp/feedDecoder";
import { NostrAvatar } from "@nmp/components-web/src/user-avatar/NostrAvatar";
import { displayLabel, shortHex } from "@nmp/components-web/src/user-avatar/ProfileWire";
import { useNostrProfileHost } from "@nmp/components-web/src/user-avatar/NostrProfileHost";
import { useNmpClient } from "../../nmp/context";
import { reactCommand } from "../../nmp/actions";

export function PostCard(props: { row: FeedRow }) {
  const host = useNostrProfileHost();
  const { client } = useNmpClient();
  const consumerId = `post-card.${createUniqueId()}`;

  const profile = () => host.profile(props.row.authorPubkey);
  const authorLabel = () => {
    const p = profile();
    // displayLabel falls back to short hex when no profile is resolved.
    return p ? displayLabel(p, props.row.authorPubkey) : shortHex(props.row.authorPubkey);
  };

  const timestamp = () => {
    const d = new Date(props.row.createdAt * 1000);
    return d.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  };

  const handleReact = () => {
    void client.dispatchCommand(reactCommand(props.row.id));
  };

  return (
    <article class="post-card" data-event-id={props.row.id}>
      {/* Avatar */}
      <div class="post-avatar">
        <NostrAvatar pubkey={props.row.authorPubkey} size={40} consumerId={consumerId} />
      </div>

      {/* Body */}
      <div class="post-body">
        {/* Header row */}
        <div class="post-header">
          <span class="post-author">
            {authorLabel()}
          </span>
          <Show when={props.row.isRepost && props.row.repostedByPubkey}>
            <span class="post-context">
              reposted by {shortHex(props.row.repostedByPubkey!)}
            </span>
          </Show>
          <span class="post-timestamp">
            {timestamp()}
          </span>
        </div>

        {/* Content */}
        <p class="post-content">
          {props.row.content}
        </p>

        {/* Actions */}
        <div class="post-actions">
          <button
            class="action-btn"
            aria-label="Like"
            onClick={handleReact}
          >
            Like
          </button>
        </div>
      </div>
    </article>
  );
}
