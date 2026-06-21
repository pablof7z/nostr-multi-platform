import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { MessageSquare, Reply, Send, Star, UserRound, Repeat2, CornerDownRight } from "lucide-solid";
import {
  resolveProfileCommand,
  followCommand,
  openProfileCommand,
  openThreadCommand,
  reactCommand,
  releaseProfileCommand,
  type RuntimeCommand,
} from "../nmp/actions";
import { shortKey, type TimelineItem } from "../nmp/snapshot";
import {
  NostrProfileHostProvider,
  useNostrProfileHost,
  type NostrProfileHost,
} from "../components/user-avatar/NostrProfileHost";
import { NostrAvatar } from "../components/user-avatar/NostrAvatar";
import { NostrProfileName } from "../components/user-name/NostrProfileName";
import { NostrContentView } from "../nmp/content/NostrContentView";

export function HomePanel(props: {
  rows: TimelineItem[];
  profileHost: NostrProfileHost;
  revision?: number;
  onPublish: (content: string, replyToId: string | null) => Promise<void>;
  onCommand: (command: RuntimeCommand) => Promise<void>;
  /** Fire-and-forget dispatch for profile claim/release — does not update the
   *  snapshot signal so Post remount churn does not prevent display-name
   *  resolution. Falls back to onCommand when absent. */
  onClaimCommand?: (command: RuntimeCommand) => void;
  onConnect?: () => void;
  signerConnected?: boolean;
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
    <NostrProfileHostProvider host={props.profileHost}>
    <section class="feed-panel" id="feed">
      <header class="topbar">
        <div>
          <p class="eyebrow">NMP snapshot {props.revision === undefined ? "pending" : `rev ${props.revision}`}</p>
          <h1>Home</h1>
        </div>
        <Show when={!props.signerConnected && props.onConnect}>
          <ConnectPrompt onConnect={props.onConnect!} />
        </Show>
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
              onClaimCommand={props.onClaimCommand}
            />
          )}
        </For>
      </Show>
    </section>
    </NostrProfileHostProvider>
  );
}

function EmptyTimeline() {
  return (
    <div class="empty-state">
      <MessageSquare size={22} />
      <p>No feed items yet — connect a signer to load your active-follows feed.</p>
    </div>
  );
}

function ConnectPrompt(props: { onConnect: () => void }) {
  const hasExtension = typeof window !== "undefined" && "nostr" in window;
  return (
    <div class="connect-prompt" data-testid="connect-prompt">
      {hasExtension
        ? <button type="button" class="connect-btn" data-testid="connect-btn" onClick={props.onConnect}>Connect</button>
        : <span class="connect-hint">Install a NIP-07 signer to load your feed.</span>
      }
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
  onClaimCommand?: (command: RuntimeCommand) => void;
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
  // Prefer the quiet claim/release dispatcher (no setSnapshot, no re-render churn).
  const claim = (cmd: RuntimeCommand) => {
    if (props.onClaimCommand) {
      props.onClaimCommand(cmd);
    } else {
      void props.onCommand(cmd);
    }
  };

  // The author profile is claimed by <NostrAvatar> below (it owns its own
  // claim/release lifecycle through the NostrProfileHost, using the same stable
  // `consumerId`), so no manual author claim is needed here.

  // F-CR-00 extension — claim attribution badge author profiles.
  //
  // Each attribution badge carries an author pubkey; claim it so the kernel
  // fetches kind:0 and the feed engine refreshes the attribution display name
  // via apply_profile. Consumer id is stable per (item, badge author) pair.
  // Use the quiet dispatcher so claim/release bookkeeping does not trigger
  // snapshot-signal updates that would remount Post components in a loop.
  for (const badge of props.item.attribution ?? []) {
    if (badge.authorPubkey) {
      const badgeConsumerId = `chirp-web-attr-${props.item.id}-${badge.authorPubkey}`;
      const badgePubkey = badge.authorPubkey;
      onMount(() => claim(resolveProfileCommand(badgePubkey, badgeConsumerId)));
      onCleanup(() => claim(releaseProfileCommand(badgePubkey, badgeConsumerId)));
    }
  }

  const host = useNostrProfileHost();
  const authorProfile = () => host.profile(authorPubkey) ?? { pubkey: authorPubkey };
  return (
    <article class="post">
      <button type="button" class="avatar avatar--component" title="Open profile" onClick={props.onProfile}>
        <NostrAvatar pubkey={authorPubkey} consumerId={consumerId} size={40} />
      </button>
      <div class="post-body">
        <button type="button" class="post-meta" onClick={props.onProfile}>
          <strong data-testid="post-author"><NostrProfileName profile={authorProfile()} /></strong>
          <span>{props.item.relativeTime ?? labelTime(props.item.createdAt)}</span>
        </button>
        <div data-testid="post-content">
          <NostrContentView tree={props.item.contentTree} fallback={props.item.content ?? ""} />
        </div>
        <Show when={props.item.attribution && props.item.attribution.length > 0}>
          <div class="attribution-list" data-testid="attribution-list">
            <For each={props.item.attribution}>
              {(badge) => (
                <span class="attribution-badge" data-testid="attribution-badge">
                  <CornerDownRight size={12} />
                  <span class="attribution-name">
                    {badge.authorDisplayName ?? shortKey(badge.authorPubkey)}
                  </span>
                  {" replied"}
                </span>
              )}
            </For>
          </div>
        </Show>
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
