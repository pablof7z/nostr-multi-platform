// PostCard.tsx — render a single home-feed post card.
//
// Pure presentation: receives a `FeedRow` (decoded from the kernel's nmp.feed.home
// OpFeedSnapshot) and renders it. Zero Nostr protocol logic — no event construction,
// no signing, no relay framing. Profile resolution is delegated to the ambient
// NostrProfileHost (which the kernel resolves via the refs.profile projection).

import { For, Show, createUniqueId } from "solid-js";
import type { FeedRow } from "../../nmp/feedDecoder";
import { NostrAvatar } from "@nmp/components-web/src/user-avatar/NostrAvatar";
import { displayLabel, shortHex } from "@nmp/components-web/src/user-avatar/ProfileWire";
import { useNostrProfileHost } from "@nmp/components-web/src/user-avatar/NostrProfileHost";
import { useNmpClient } from "../../nmp/context";
import { bookmarkCommand, reactCommand, repostCommand } from "../../nmp/actions";
import { RichContent } from "./RichContent";

export type FeedSelection = { kind: "profile" | "thread"; row: FeedRow };

export function PostCard(props: {
  row: FeedRow;
  canPublish: boolean;
  activeAccountPubkey?: string;
  bookmarked: boolean;
  onSelect: (selection: FeedSelection) => void;
  onQuote: (row: FeedRow) => void;
}) {
  const host = useNostrProfileHost();
  const { client } = useNmpClient();
  const consumerId = `post-card.${createUniqueId()}`;

  const profile = () => host.profile(props.row.authorPubkey);
  const authorLabel = () => {
    const p = profile();
    if (!p && props.row.authorDisplayName) return props.row.authorDisplayName;
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
    if (!props.canPublish) return;
    void client.dispatchCommand(reactCommand(props.row.id));
  };
  const handleRepost = () => {
    if (!props.canPublish) return;
    void client.dispatchCommand(
      repostCommand(
        props.row.id,
        props.row.kind,
        props.row.authorPubkey,
        props.row.relayProvenance[0] ?? null,
      ),
    );
  };
  const handleBookmark = () => {
    if (!props.canPublish || !props.activeAccountPubkey) return;
    void client.dispatchCommand(
      bookmarkCommand(
        props.activeAccountPubkey,
        props.row.id,
        !props.bookmarked,
        props.row.relayProvenance[0] ?? null,
      ),
    );
  };

  const counts = () => [
    { label: "replies", value: props.row.relationCounts.replies },
    { label: "likes", value: props.row.relationCounts.reactions },
    { label: "reposts", value: props.row.relationCounts.reposts },
    { label: "zaps", value: props.row.relationCounts.zaps },
  ].filter((item) => item.value > 0);
  const displayContent = () => props.row.contentPreview || props.row.content;

  return (
    <article class="post-card" data-testid="post-card" data-event-id={props.row.id}>
      {/* Avatar */}
      <div class="post-avatar">
        <NostrAvatar pubkey={props.row.authorPubkey} size={40} consumerId={consumerId} />
      </div>

      {/* Body */}
      <div class="post-body">
        {/* Header row */}
        <div class="post-header">
          <button
            type="button"
            class="post-author"
            aria-label={`Open profile for ${authorLabel()}`}
            onClick={() => props.onSelect({ kind: "profile", row: props.row })}
          >
            {authorLabel()}
          </button>
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
        <RichContent content={displayContent()} />

        <Show when={counts().length > 0 || props.row.relayProvenance.length > 0}>
          <div class="post-meta-row">
            <Show when={counts().length > 0}>
              <div class="post-counts" data-testid="post-counts">
                <For each={counts()}>
                  {(item) => (
                    <span>
                      <strong>{item.value}</strong> {item.label}
                    </span>
                  )}
                </For>
              </div>
            </Show>
            <Show when={props.row.relayProvenance.length > 0}>
              <div class="post-provenance" data-testid="relay-provenance">
                <For each={props.row.relayProvenance.slice(0, 2)}>
                  {(relay) => <span title={relay}>{relay.replace(/^wss?:\/\//, "")}</span>}
                </For>
              </div>
            </Show>
          </div>
        </Show>

        {/* Actions */}
        <div class="post-actions">
          <button
            class="action-btn"
            aria-label="Open thread"
            onClick={() => props.onSelect({ kind: "thread", row: props.row })}
          >
            Thread
          </button>
          <button
            class="action-btn"
            aria-label="Like"
            title={props.canPublish ? "Like" : "Sign in to like"}
            disabled={!props.canPublish}
            onClick={handleReact}
          >
            Like
          </button>
          <button
            class="action-btn"
            aria-label="Repost"
            title={props.canPublish ? "Repost" : "Sign in to repost"}
            disabled={!props.canPublish}
            onClick={handleRepost}
          >
            Repost
          </button>
          <button
            class="action-btn"
            aria-label="Quote"
            title={props.canPublish ? "Quote" : "Sign in to quote"}
            disabled={!props.canPublish}
            onClick={() => props.onQuote(props.row)}
          >
            Quote
          </button>
          <button
            class="action-btn"
            data-state={props.bookmarked ? "saved" : "idle"}
            aria-pressed={props.bookmarked ? "true" : "false"}
            aria-label={props.bookmarked ? "Remove bookmark" : "Bookmark"}
            title={
              props.canPublish && props.activeAccountPubkey
                ? props.bookmarked
                  ? "Remove bookmark"
                  : "Bookmark"
                : "Sign in to bookmark"
            }
            disabled={!props.canPublish || !props.activeAccountPubkey}
            onClick={handleBookmark}
          >
            {props.bookmarked ? "Saved" : "Save"}
          </button>
        </div>
      </div>
    </article>
  );
}
