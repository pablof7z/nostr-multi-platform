import { Show } from "solid-js";
import type { ProfileSummary } from "../chirp/model";

export function Avatar(props: { profile: ProfileSummary; className?: string }) {
  const label = () => props.profile.name.slice(0, 1).toUpperCase();
  return (
    <span class={props.className ?? "avatar"}>
      <Show when={props.profile.avatarUrl} fallback={label()}>
        {(url) => <img src={url()} alt="" loading="lazy" />}
      </Show>
    </span>
  );
}
