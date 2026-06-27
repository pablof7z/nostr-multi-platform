import { For, Show, createEffect, createMemo, createSignal } from "solid-js";
import type { FeedRow } from "../../nmp/feedDecoder";
import { useNmpClient } from "../../nmp/context";
import { followCommand, publishNoteAction } from "../../nmp/actions";
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
  followPubkeys: string[];
  onClose: () => void;
}) {
  const host = useNostrProfileHost();
  const { client } = useNmpClient();
  const profile = () => host.profile(props.selection.row.authorPubkey);
  const [followBusy, setFollowBusy] = createSignal(false);
  const authorLabel = () => {
    const resolved = profile();
    if (!resolved && props.selection.row.authorDisplayName) {
      return props.selection.row.authorDisplayName;
    }
    return displayLabel(resolved, props.selection.row.authorPubkey);
  };
  const consumerId = createMemo(() => `feed-detail.${props.selection.kind}.${props.selection.row.id}`);
  const targetPubkey = () => props.selection.row.authorPubkey;
  const following = () => props.followPubkeys.includes(targetPubkey());

  createEffect(() => {
    targetPubkey();
    setFollowBusy(false);
  });

  const publishFollow = async (next: boolean) => {
    if (!props.canPublish || followBusy()) return;
    setFollowBusy(true);
    try {
      await client.dispatchCommand(followCommand(targetPubkey(), next));
    } finally {
      setFollowBusy(false);
    }
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
        fallback={<ThreadPreview row={props.selection.row} canPublish={props.canPublish} />}
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
            data-testid="profile-follow-toggle"
            aria-label={following() ? "Unfollow" : "Follow"}
            aria-pressed={following() ? "true" : "false"}
            disabled={!props.canPublish || followBusy()}
            title={
              props.canPublish
                ? following()
                  ? "Publish unfollow"
                  : "Publish follow"
                : "Sign in to follow"
            }
            onClick={() => void publishFollow(!following())}
          >
            {followBusy() ? "Publishing..." : following() ? "Following" : "Follow"}
          </button>
        </div>
      </Show>
    </aside>
  );
}

function ThreadPreview(props: { row: FeedRow; canPublish: boolean }) {
  const { client } = useNmpClient();
  const [reply, setReply] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);
  const stats = () => [
    { label: "Replies", value: props.row.relationCounts.replies },
    { label: "Likes", value: props.row.relationCounts.reactions },
    { label: "Reposts", value: props.row.relationCounts.reposts },
    { label: "Zaps", value: props.row.relationCounts.zaps },
    { label: "Comments", value: props.row.relationCounts.comments },
  ];
  const canSubmit = () => props.canPublish && reply().trim().length > 0 && !submitting();

  const publishReply = async () => {
    const content = reply().trim();
    if (!content || !props.canPublish || submitting()) return;
    setSubmitting(true);
    try {
      await client.dispatchChirp(publishNoteAction(content, props.row.id));
      setReply("");
    } finally {
      setSubmitting(false);
    }
  };

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
      <form
        class="thread-reply-form"
        onSubmit={(event) => {
          event.preventDefault();
          void publishReply();
        }}
      >
        <textarea
          aria-label="Reply to thread"
          data-testid="thread-reply-input"
          placeholder={props.canPublish ? "Reply to this thread" : "Sign in to reply"}
          value={reply()}
          onInput={(event) => setReply(event.currentTarget.value)}
          disabled={!props.canPublish || submitting()}
          rows={3}
          maxLength={280}
        />
        <button
          class="detail-secondary"
          data-testid="thread-reply-submit"
          type="submit"
          disabled={!canSubmit()}
        >
          {submitting() ? "Replying..." : "Reply"}
        </button>
      </form>
    </div>
  );
}
