/**
 * NostrMentionChip — inline avatar + display-name chip for the web. Used anywhere
 * a Nostr profile is referenced inline (the `embed-profile` body, `nostr:npub…`
 * mentions in content).
 *
 * Pure renderer (D7): takes a resolved `ProfileWire` (the host owns claim/resolve
 * via the same profile projection the user-* components read). Renders a small
 * round avatar (picture or deterministic identicon fallback) + the display name,
 * inline. No claim is required for `npub:` URIs — the profile resolves through
 * the kernel projection exactly like the user-* components. Reuses the
 * `user-avatar` identicon palette + `displayLabel` so the look matches the
 * avatar/name components exactly. Mirrors the SwiftUI/Compose `NostrMentionChip`.
 */
import type { JSX } from "solid-js";
import { Show } from "solid-js";
import { type ProfileWire, displayLabel } from "../user-avatar/ProfileWire";
import { identiconColor } from "../user-avatar/NostrAvatar";

export function NostrMentionChip(props: { profile: ProfileWire }): JSX.Element {
  return (
    <span class="nostr-mention-chip">
      <Show
        when={props.profile.pictureUrl}
        fallback={
          <span
            class="nostr-mention-chip__avatar nostr-mention-chip__avatar--identicon"
            style={{ "background-color": identiconColor(props.profile.pubkey) }}
          />
        }
      >
        {(pic) => <img class="nostr-mention-chip__avatar" src={pic()} alt="" loading="lazy" />}
      </Show>
      <span class="nostr-mention-chip__name">
        {displayLabel(props.profile, props.profile.pubkey)}
      </span>
    </span>
  );
}
