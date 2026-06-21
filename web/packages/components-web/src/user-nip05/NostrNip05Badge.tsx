import { Show } from "solid-js";
import type { ProfileWire } from "../user-avatar/ProfileWire";

// NIP-05 verified-identity badge — a seal/check glyph + the identifier string.
// Renders nothing when the profile has no NIP-05 identifier. `_@domain` is the
// NIP-05 shorthand meaning the domain itself is the identity, so the `_@`
// prefix is stripped for display. Mirrors the SwiftUI / Compose badge.
//
// Depends on `web/user-avatar` for `ProfileWire`.
export function NostrNip05Badge(props: {
  profile: ProfileWire;
  class?: string;
}) {
  const nip05 = () => props.profile.nip05 ?? "";
  const displayText = () => {
    const v = nip05();
    return v.startsWith("_@") ? v.slice(2) : v;
  };
  return (
    <Show when={nip05().length > 0}>
      <span
        class={props.class ?? "nostr-nip05-badge"}
        aria-label={`Verified: ${displayText()}`}
        style={{ display: "inline-flex", "align-items": "center", gap: "4px" }}
      >
        <svg
          width="13"
          height="13"
          viewBox="0 0 24 24"
          fill="currentColor"
          aria-hidden="true"
          style={{ "flex-shrink": "0" }}
        >
          <path d="M12 1.5l2.6 1.9 3.2-.3 1 3.1 2.7 1.8-1 3.1 1 3.1-2.7 1.8-1 3.1-3.2-.3L12 22.5l-2.6-1.9-3.2.3-1-3.1-2.7-1.8 1-3.1-1-3.1 2.7-1.8 1-3.1 3.2.3L12 1.5z" />
          <path d="M10.6 14.6l-2.2-2.2-1.4 1.4 3.6 3.6 6.4-6.4-1.4-1.4z" fill="white" />
        </svg>
        <span class="nostr-nip05-text">{displayText()}</span>
      </span>
    </Show>
  );
}
