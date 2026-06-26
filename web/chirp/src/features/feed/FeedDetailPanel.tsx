import { For, Show, createMemo } from "solid-js";
import type { FeedRow } from "../../nmp/feedDecoder";
import { useNmpClient } from "../../nmp/context";
import { followCommand } from "../../nmp/actions";
import { displayLabel, shortHex } from "@nmp/components-web/src/user-avatar/ProfileWire";
import { useNostrProfileHost } from "@nmp/components-web/src/user-avatar/NostrProfileHost";
import { NostrAvatar } from "@nmp/components-web/src/user-avatar/NostrAvatar";
import "./feed-detail.css";

export type FeedDetailSelection = {
  kind: "profile" | "thread";
  row: FeedRow;
};

export function FeedDetailPanel(props: {
  selection: FeedDetailSelection;
  canPublish: boolean;
  onClose: () => void;
}) {
  const host = useNostrProfileHost();
  const { client } = useNmpClient();
  const profile = () => host.profile(props.selection.row.authorPubkey);
  const authorLabel = () => {
    const resolved = profile();
    if (!resolved && props.selection.row.authorDisplayName) {
      return props.selection.row.authorDisplayName;
    }
    return displayLabel(resolved, props.selection.row.authorPubkey);
  };
  const consumerId = createMemo(() => `feed-detail.${props.selection.kind}.${props.selection.row.id}`);

  const publishFollow = () => {
    if (!props.canPublish) return;
    void client.dispatchCommand(followCommand(props.selection.row.authorPubkey, true));
  };

  return (
    <aside
      class="feed-detail"
      data-testid="feed-detail-panel"
      data-kind={props.selection.kind}
    >
      <div class="feed-detail-header">
        <strong>{props.selection.kind === "profile" ? "Profile" : "Thread"}</strong>
        <button class="icon-btn" aria-label="Close detail" onClick={props.onClose}>
          x
        </button>
      </div>

      <Show
        when={props.selection.kind === "profile"}
        fallback={<ThreadPreview row={props.selection.row} />}
      >
        <div class="profile-preview">
          <NostrAvatar
            pubkey={props.selection.row.authorPubkey}
            size={56}
            consumerId={consumerId()}
          />
          <div class="profile-preview-body">
            <strong>{authorLabel()}</strong>
            <code>{shortHex(props.selection.row.authorPubkey)}</code>
            <Show when={profile()?.about}>
              <p>{profile()!.about}</p>
            </Show>
            <Show when={profile()?.nip05}>
              <span>{profile()!.nip05}</span>
            </Show>
          </div>
          <button
            class="detail-primary"
            disabled={!props.canPublish}
            title={props.canPublish ? "Publish follow" : "Sign in to follow"}
            onClick={publishFollow}
          >
            Follow
          </button>
        </div>
      </Show>
    </aside>
  );
}

function ThreadPreview(props: { row: FeedRow }) {
  const stats = () => [
    { label: "Replies", value: props.row.relationCounts.replies },
    { label: "Likes", value: props.row.relationCounts.reactions },
    { label: "Reposts", value: props.row.relationCounts.reposts },
    { label: "Zaps", value: props.row.relationCounts.zaps },
    { label: "Comments", value: props.row.relationCounts.comments },
  ];

  return (
    <div class="thread-preview">
      <p>{props.row.content}</p>
      <div class="thread-stats">
        <For each={stats()}>
          {(item) => (
            <span>
              <strong>{item.value}</strong>
              {item.label}
            </span>
          )}
        </For>
      </div>
      <div class="thread-relays" data-testid="detail-relay-provenance">
        <Show
          when={props.row.relayProvenance.length > 0}
          fallback={<span>Relay provenance unavailable</span>}
        >
          <For each={props.row.relayProvenance}>
            {(relay) => <span title={relay}>{relay.replace(/^wss?:\/\//, "")}</span>}
          </For>
        </Show>
      </div>
      <button class="detail-secondary" disabled>
        Reply requires runtime thread publishing
      </button>
    </div>
  );
}
