import { For } from "solid-js";
import { MessageCircle, Repeat2, Send, Star } from "lucide-solid";
import { Avatar } from "../components/Avatar";
import { EntityContent } from "../components/EntityContent";
import type { TimelineItem } from "../chirp/model";
import { Topbar } from "./Topbar";

export function TimelineView(props: {
  title: string;
  eyebrow: string;
  items: TimelineItem[];
  draft: string;
  onDraft: (value: string) => void;
  onPublish: () => void;
  onRefresh: () => void;
  onProfile: (pubkey: string) => void;
  onEntity: (entity: string) => void;
}) {
  return (
    <>
      <Topbar eyebrow={props.eyebrow} title={props.title} actionLabel="Refresh timeline" onAction={props.onRefresh} />
      <div class="composer">
        <textarea
          value={props.draft}
          aria-label="Compose chirp"
          placeholder="What is happening on Nostr?"
          onInput={(event) => props.onDraft(event.currentTarget.value)}
        />
        <div class="composer-actions">
          <span>{props.draft.trim().length}/280</span>
          <button type="button" onClick={props.onPublish} disabled={props.draft.trim().length === 0}>
            <Send size={17} />
            Publish
          </button>
        </div>
      </div>
      <For each={props.items}>{(item) => <NoteRow item={item} onProfile={props.onProfile} onEntity={props.onEntity} />}</For>
    </>
  );
}

function NoteRow(props: {
  item: TimelineItem;
  onProfile: (pubkey: string) => void;
  onEntity: (entity: string) => void;
}) {
  return (
    <article class="post">
      <button
        class="avatar-button"
        type="button"
        aria-label={`Open ${props.item.author.name}`}
        onClick={() => props.onProfile(props.item.author.pubkey)}
      >
        <Avatar profile={props.item.author} />
      </button>
      <div class="post-body">
        <div class="post-meta">
          <button type="button" onClick={() => props.onProfile(props.item.author.pubkey)}>
            {props.item.author.name}
          </button>
          <span>@{props.item.author.handle}</span>
          <span>{props.item.createdAt}</span>
        </div>
        <EntityContent content={props.item.content} onOpen={props.onEntity} />
        <div class="post-actions">
          <span>
            <MessageCircle size={15} />
            {props.item.replyCount}
          </span>
          <span>
            <Repeat2 size={15} />
            {props.item.repostCount}
          </span>
          <span>
            <Star size={15} />
            {props.item.reactionCount}
          </span>
        </div>
      </div>
    </article>
  );
}
