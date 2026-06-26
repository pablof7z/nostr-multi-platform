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
    <article
      class="post-card"
      data-event-id={props.row.id}
      style={{
        display: "flex",
        gap: "12px",
        padding: "12px 16px",
        "border-bottom": "1px solid rgba(0,0,0,0.08)",
      }}
    >
      {/* Avatar */}
      <div style={{ "flex-shrink": "0" }}>
        <NostrAvatar pubkey={props.row.authorPubkey} size={40} consumerId={consumerId} />
      </div>

      {/* Body */}
      <div style={{ flex: "1", "min-width": "0" }}>
        {/* Header row */}
        <div
          style={{
            display: "flex",
            gap: "6px",
            "align-items": "baseline",
            "flex-wrap": "wrap",
          }}
        >
          <span
            class="post-author"
            style={{ "font-weight": "600", "font-size": "0.9rem", color: "#111" }}
          >
            {authorLabel()}
          </span>
          <Show when={props.row.isRepost && props.row.repostedByPubkey}>
            <span style={{ "font-size": "0.8rem", color: "#666" }}>
              reposted by {shortHex(props.row.repostedByPubkey!)}
            </span>
          </Show>
          <span
            class="post-timestamp"
            style={{ "font-size": "0.8rem", color: "#888", "margin-left": "auto" }}
          >
            {timestamp()}
          </span>
        </div>

        {/* Content */}
        <p
          class="post-content"
          style={{
            margin: "4px 0 8px",
            "font-size": "0.9rem",
            "line-height": "1.45",
            color: "#222",
            "word-break": "break-word",
            "white-space": "pre-wrap",
          }}
        >
          {props.row.content}
        </p>

        {/* Actions */}
        <div
          class="post-actions"
          style={{ display: "flex", gap: "16px", "align-items": "center" }}
        >
          <button
            class="action-btn"
            aria-label="Like"
            onClick={handleReact}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              color: "#888",
              "font-size": "0.8rem",
              padding: "2px 6px",
            }}
          >
            ♥
          </button>
        </div>
      </div>
    </article>
  );
}
