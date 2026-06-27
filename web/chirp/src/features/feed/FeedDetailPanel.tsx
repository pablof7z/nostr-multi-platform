import { For, Show, createEffect, createMemo, createSignal, createUniqueId } from "solid-js";
import type { FeedReplyAttribution, FeedRow } from "../../nmp/feedDecoder";
import { useNmpClient } from "../../nmp/context";
import { followCommand, publishNoteAction } from "../../nmp/actions";
import { displayLabel, shortHex as shortPubkey } from "@nmp/components-web/src/user-avatar/ProfileWire";
import { useNostrProfileHost } from "@nmp/components-web/src/user-avatar/NostrProfileHost";
import { NostrAvatar } from "@nmp/components-web/src/user-avatar/NostrAvatar";
import { ProfileDetail } from "./ProfileDetail";
import "./feed-detail.css";

export type FeedDetailSelection = {
  kind: "profile" | "thread";
  row: FeedRow;
};

export function FeedDetailPanel(props: {
  selection: FeedDetailSelection;
  rows: FeedRow[];
  canPublish: boolean;
  followPubkeys: string[];
  onClose: () => void;
}) {
  const { client } = useNmpClient();
  const [followBusy, setFollowBusy] = createSignal(false);
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
        <ProfileDetail
          row={props.selection.row}
          rows={props.rows}
          consumerId={consumerId()}
          following={following()}
          canPublish={props.canPublish}
          followBusy={followBusy()}
          onFollowToggle={(next) => void publishFollow(next)}
        />
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
      <section class="thread-replies" aria-label="Replies">
        <div class="thread-section-heading">
          <strong>Replies</strong>
          <span>{props.row.replyAttributions.length}</span>
        </div>
        <Show
          when={props.row.replyAttributions.length > 0}
          fallback={<p class="thread-empty">No replies in view yet.</p>}
        >
          <For each={props.row.replyAttributions}>
            {(reply) => <ThreadReplyAttribution reply={reply} />}
          </For>
        </Show>
      </section>
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

function ThreadReplyAttribution(props: { reply: FeedReplyAttribution }) {
  const host = useNostrProfileHost();
  const consumerId = `thread-reply.${createUniqueId()}`;
  const author = () => {
    const profile = host.profile(props.reply.authorPubkey);
    if (profile) return displayLabel(profile, props.reply.authorPubkey);
    return props.reply.authorDisplayName || shortPubkey(props.reply.authorPubkey);
  };
  const timestamp = () =>
    new Date(props.reply.replyCreatedAt * 1000).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  return (
    <article class="thread-reply" data-testid="thread-reply-attribution">
      <NostrAvatar pubkey={props.reply.authorPubkey} size={28} consumerId={consumerId} />
      <div class="thread-reply-body">
        <div>
          <strong title={props.reply.authorPubkey}>{author()}</strong>
          <span>{timestamp()}</span>
        </div>
        <code title={props.reply.replyEventId}>{shortHex(props.reply.replyEventId)}</code>
      </div>
    </article>
  );
}

function shortHex(value: string): string {
  return value.length > 10 ? `${value.slice(0, 6)}...${value.slice(-4)}` : value;
}
