import { For, Show, createMemo } from "solid-js";
import type { FeedRow } from "../../nmp/feedDecoder";
import { NostrAvatar } from "@nmp/components-web/src/user-avatar/NostrAvatar";
import { displayLabel, shortHex } from "@nmp/components-web/src/user-avatar/ProfileWire";
import { useNostrProfileHost } from "@nmp/components-web/src/user-avatar/NostrProfileHost";
import { RichContent } from "./RichContent";

export function ProfileDetail(props: {
  row: FeedRow;
  rows: FeedRow[];
  consumerId: string;
  following: boolean;
  canPublish: boolean;
  followBusy: boolean;
  onFollowToggle: (next: boolean) => void;
}) {
  const host = useNostrProfileHost();
  const profile = () => host.profile(props.row.authorPubkey);
  const authorRows = createMemo(() =>
    props.rows.filter((row) => row.authorPubkey === props.row.authorPubkey),
  );
  const relays = createMemo(() =>
    [...new Set(authorRows().flatMap((row) => row.relayProvenance))].slice(0, 4),
  );
  const authorLabel = () => {
    const resolved = profile();
    if (!resolved && props.row.authorDisplayName) return props.row.authorDisplayName;
    return displayLabel(resolved, props.row.authorPubkey);
  };

  return (
    <div class="profile-detail" data-testid="profile-detail">
      <div class="profile-preview">
        <NostrAvatar pubkey={props.row.authorPubkey} size={56} consumerId={props.consumerId} />
        <div class="profile-preview-body">
          <strong>{authorLabel()}</strong>
          <code>{shortHex(props.row.authorPubkey)}</code>
          <Show when={profile()?.about}>
            <p>{profile()!.about}</p>
          </Show>
          <Show when={profile()?.nip05}>
            <span>{profile()!.nip05}</span>
          </Show>
        </div>
        <button
          class="detail-primary"
          data-testid="profile-follow-toggle"
          aria-label={props.following ? "Unfollow" : "Follow"}
          aria-pressed={props.following ? "true" : "false"}
          disabled={!props.canPublish || props.followBusy}
          title={
            props.canPublish
              ? props.following
                ? "Publish unfollow"
                : "Publish follow"
              : "Sign in to follow"
          }
          onClick={() => props.onFollowToggle(!props.following)}
        >
          {props.followBusy ? "Publishing..." : props.following ? "Following" : "Follow"}
        </button>
      </div>

      <div class="profile-context-grid">
        <div class="profile-context-card">
          <span>Hydrated posts</span>
          <strong>{authorRows().length}</strong>
        </div>
        <div class="profile-context-card">
          <span>Follow state</span>
          <strong>{props.following ? "Following" : "Not following"}</strong>
        </div>
      </div>

      <div class="profile-relays" data-testid="profile-relay-provenance">
        <span>Seen via</span>
        <Show when={relays().length > 0} fallback={<code>Relay provenance unavailable</code>}>
          <For each={relays()}>
            {(relay) => <code title={relay}>{relay.replace(/^wss?:\/\//, "")}</code>}
          </For>
        </Show>
      </div>

      <section class="profile-authored" aria-label="Hydrated posts by this author">
        <div class="profile-section-heading">
          <strong>Posts in this session</strong>
          <span>from Rust feed projection</span>
        </div>
        <Show
          when={authorRows().length > 0}
          fallback={<p class="profile-empty">No hydrated posts from this author yet.</p>}
        >
          <For each={authorRows().slice(0, 3)}>
            {(row) => (
              <article class="profile-authored-post" data-testid="profile-authored-post">
                <time>
                  {new Date(row.createdAt * 1000).toLocaleString(undefined, {
                    month: "short",
                    day: "numeric",
                    hour: "2-digit",
                    minute: "2-digit",
                  })}
                </time>
                <RichContent content={row.contentPreview || row.content} />
              </article>
            )}
          </For>
        </Show>
      </section>
    </div>
  );
}
