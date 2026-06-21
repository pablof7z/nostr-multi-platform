import type { ProfileWire } from "../user-avatar/ProfileWire";
import { NostrAvatar } from "../user-avatar/NostrAvatar";
import { NostrProfileName } from "../user-name/NostrProfileName";
import { NostrNip05Badge } from "../user-nip05/NostrNip05Badge";

// Compact author header: avatar + display name + optional NIP-05 badge — the
// most common pattern in note feeds and thread views. The avatar claims its
// own profile via the host (reference-first); name and badge render the given
// `ProfileWire`. Mirrors the SwiftUI / Compose `NostrUserCard`.
//
// Depends on `web/user-avatar`, `web/user-name`, `web/user-nip05`.
export function NostrUserCard(props: {
  profile: ProfileWire;
  avatarSize?: number;
  onTap?: (pubkey: string) => void;
}) {
  return (
    <button
      type="button"
      class="nostr-user-card"
      onClick={() => props.onTap?.(props.profile.pubkey)}
      aria-label={`${props.profile.displayName ?? props.profile.pubkey}, profile`}
      style={{
        display: "flex",
        "align-items": "center",
        gap: "10px",
        background: "none",
        border: "none",
        padding: "0",
        cursor: props.onTap ? "pointer" : "default",
        "text-align": "left",
        width: "100%",
        color: "inherit",
        font: "inherit",
      }}
    >
      <NostrAvatar pubkey={props.profile.pubkey} size={props.avatarSize ?? 40} />
      <span style={{ display: "flex", "flex-direction": "column", gap: "2px", "min-width": "0" }}>
        <NostrProfileName profile={props.profile} />
        <NostrNip05Badge profile={props.profile} />
      </span>
    </button>
  );
}
