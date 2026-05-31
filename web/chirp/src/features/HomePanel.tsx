import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { MessageSquare, Repeat2, Reply, Send, Star, UserRound } from "lucide-solid";
import {
  claimProfileCommand,
  followCommand,
  openProfileCommand,
  openThreadCommand,
  reactCommand,
  releaseProfileCommand,
  type RuntimeCommand,
} from "../nmp/actions";
import { displayAuthor, shortKey, type TimelineItem } from "../nmp/snapshot";

export function HomePanel(props: {
  rows: TimelineItem[];
  revision?: number;
  onPublish: (content: string, replyToId: string | null) => Promise<void>;
  onCommand: (command: RuntimeCommand) => Promise<void>;
}) {
  const [draft, setDraft] = createSignal("");
  const [replyToId, setReplyToId] = createSignal<string | null>(null);
  const publish = async () => {
    const content = draft().trim();
    if (!content) {
      return;
    }
    await props.onPublish(content, replyToId());
    setDraft("");
    setReplyToId(null);
  };
  return (
    <section class="feed-panel" id="feed">
      <header class="topbar">
        <div>
          <p class="eyebrow">NMP snapshot {props.revision === undefined ? "pending" : `rev ${props.revision}`}</p>
          <h1>Home</h1>
        </div>
      </header>
      <div class="composer">
        <Show when={replyToId()}>
          {(id) => (
            <button type="button" class="inline-token" onClick={() => setReplyToId(null)}>
              <Reply size={14} /> Replying to {shortKey(id())}
            </button>
          )}
        </Show>
        <textarea value={draft()} aria-label="Compose chirp" placeholder="What is happening on Nostr?" onInput={(event) => setDraft(event.currentTarget.value)} />
        <div class="composer-actions">
          <span>{draft().trim().length}/280</span>
          <button type="button" onClick={publish} disabled={draft().trim().length === 0}>
            <Send size={17} /> Publish
          </button>
        </div>
      </div>
      <Show when={props.rows.length > 0} fallback={<EmptyTimeline />}>
        <For each={props.rows}>
          {(item) => (
            <Post
              item={item}
              onReply={() => setReplyToId(item.id)}
              onReact={() => props.onCommand(reactCommand(item.id))}
              onFollow={() => props.onCommand(followCommand(item.authorPubkey ?? item.pubkey ?? "", true))}
              onProfile={() => props.onCommand(openProfileCommand(item.authorPubkey ?? item.pubkey ?? ""))}
              onThread={() => props.onCommand(openThreadCommand(item.id))}
              onCommand={props.onCommand}
            />
          )}
        </For>
      </Show>
    </section>
  );
}

function EmptyTimeline() {
  return (
    <div class="empty-state">
      <MessageSquare size={22} />
      <p>No Rust snapshot has produced timeline rows yet.</p>
    </div>
  );
}

function Post(props: {
  item: TimelineItem;
  onReply: () => void;
  onReact: () => void;
  onFollow: () => void;
  onProfile: () => void;
  onThread: () => void;
  onCommand: (command: RuntimeCommand) => Promise<void>;
}) {
  // F-CR-00 — component-owned profile claim.
  //
  // On mount dispatch a claim for the author pubkey so the kernel fetches
  // the kind:0 profile. On unmount release the claim so the kernel stops
  // tracking interest and can garbage-collect the subscription once all
  // consumers release. `consumer_id` is stable per card instance — keyed on
  // the event id so two cards for the same author from different events each
  // carry their own refcount entry (matching iOS `chirp-avatar.<uuid>` /
  // Android `note-author-<eventId>` naming conventions).
  //
  // Guard against empty pubkeys (rare but the kernel rejects them silently;
  // no point dispatching a claim we know will be a no-op).
  const authorPubkey = props.item.authorPubkey ?? props.item.pubkey ?? "";
  const consumerId = `chirp-web-author-${props.item.id}`;

  if (authorPubkey) {
    onMount(() => {
      void props.onCommand(claimProfileCommand(authorPubkey, consumerId));
    });
    onCleanup(() => {
      void props.onCommand(releaseProfileCommand(authorPubkey, consumerId));
    });
  }

  const author = () => displayAuthor(props.item);
  return (
    <article class="post">
      <button type="button" class="avatar" title="Open profile" onClick={props.onProfile}>{author().slice(0, 1).toUpperCase()}</button>
      <div class="post-body">
        <button type="button" class="post-meta" onClick={props.onProfile}>
          <strong>{author()}</strong>
          <span>{props.item.relativeTime ?? labelTime(props.item.createdAt)}</span>
        </button>
        <p>{props.item.content ?? ""}</p>
        <div class="post-stats">
          <span>{countLabel(props.item.relationCounts?.replies)} replies</span>
          <span>{countLabel(props.item.relationCounts?.reactions)} reactions</span>
          <span>{countLabel(props.item.relationCounts?.reposts)} reposts</span>
        </div>
        <div class="row-actions">
          <button type="button" title="Open thread" onClick={props.onThread}><MessageSquare size={16} /> Thread</button>
          <button type="button" title="Reply" onClick={props.onReply}><Reply size={16} /> Reply</button>
          <button type="button" title="React" onClick={props.onReact}><Star size={16} /> React</button>
          <button type="button" title="Follow author" onClick={props.onFollow}><UserRound size={16} /> Follow</button>
          <span class="inline-token"><Repeat2 size={14} /> {shortKey(props.item.id)}</span>
        </div>
      </div>
    </article>
  );
}

function countLabel(value?: { status?: string; count?: number }): string {
  return value?.count === undefined ? (value?.status ?? "loading") : String(value.count);
}

function labelTime(epochSeconds?: number): string {
  return epochSeconds ? new Date(epochSeconds * 1000).toLocaleString() : "";
}
