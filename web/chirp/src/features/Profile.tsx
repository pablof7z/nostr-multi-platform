import { For, Show } from "solid-js";
import { AtSign, BellPlus, UserMinus, UserPlus } from "lucide-solid";
import { Avatar } from "../components/Avatar";
import type { ProfileSummary, TimelineItem } from "../chirp/model";
import { EntityContent } from "../components/EntityContent";
import { Topbar } from "./Topbar";

export function ProfileView(props: {
  profile: ProfileSummary;
  notes: TimelineItem[];
  onFollow: (pubkey: string, follow: boolean) => void;
  onEntity: (entity: string) => void;
}) {
  return (
    <>
      <Topbar eyebrow="Profile" title={props.profile.name} />
      <section class="profile-hero">
        <div class="profile-banner" style={{ "background-image": `url(${props.profile.bannerUrl ?? ""})` }} />
        <div class="profile-main">
          <Avatar profile={props.profile} className="profile-avatar" />
          <button
            type="button"
            onClick={() => props.onFollow(props.profile.pubkey, !props.profile.followedByViewer)}
          >
            {props.profile.followedByViewer ? <UserMinus size={17} /> : <UserPlus size={17} />}
            {props.profile.followedByViewer ? "Unfollow" : "Follow"}
          </button>
        </div>
        <h2>{props.profile.name}</h2>
        <p class="profile-handle">
          <AtSign size={15} />
          {props.profile.handle}
        </p>
        <p>{props.profile.about}</p>
        <div class="stats-row">
          <strong>{props.profile.stats.following}</strong>
          <span>Following</span>
          <strong>{props.profile.stats.followers}</strong>
          <span>Followers</span>
          <strong>{props.profile.stats.notes}</strong>
          <span>Notes</span>
        </div>
        <Show when={props.profile.followsYou}>
          <p class="follows-you">
            <BellPlus size={15} />
            Follows you
          </p>
        </Show>
      </section>
      <section class="stack-list">
        <For each={props.notes}>
          {(note) => (
            <article class="compact-note">
              <span>{note.createdAt}</span>
              <EntityContent content={note.content} onOpen={props.onEntity} />
            </article>
          )}
        </For>
      </section>
    </>
  );
}
