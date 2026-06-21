import { displayLabel, type ProfileWire } from "../user-avatar/ProfileWire";

// Inline display-name text for a Nostr profile. Shows `displayName` when set;
// falls back to `npubShort` (Rust-formatted) and finally to a short raw-hex
// form of the pubkey. Pure render of the `ProfileWire` it is given — mirrors
// the SwiftUI / Compose `NostrProfileName`.
//
// Depends on `web/user-avatar` for `ProfileWire`.
export function NostrProfileName(props: {
  profile: ProfileWire;
  class?: string;
}) {
  const label = () => displayLabel(props.profile, props.profile.pubkey);
  return (
    <span
      class={props.class ?? "nostr-profile-name"}
      aria-label={`Display name: ${label()}`}
    >
      {label()}
    </span>
  );
}
