import { Show, createSignal, createMemo, createUniqueId, onCleanup, onMount } from "solid-js";
import { avatarUrl, type ProfileWire } from "./ProfileWire";
import { useNostrProfileHost } from "./NostrProfileHost";

// Circular avatar for a Nostr pubkey. Shows the profile picture when the host
// projection has it; falls back to a deterministic identicon derived from the
// raw pubkey (color + first two hex chars), mirroring the SwiftUI / Compose
// `NostrAvatar`. Claims the profile on mount so the kernel fetches kind:0, and
// releases on cleanup.
//
// Depends on `ProfileWire` + `NostrProfileHost`. Replace the <img> with your
// own image cache if you have one — the identicon fallback is self-contained.
export function NostrAvatar(props: {
  pubkey: string;
  size?: number;
  /** Stable refcount key. Defaults to a per-instance unique id. */
  consumerId?: string;
}) {
  const host = useNostrProfileHost();
  const size = () => props.size ?? 40;
  const consumerId = props.consumerId ?? `nostr-avatar.${createUniqueId()}`;

  // The kernel resolves kind:0 after the claim; <img> errors fall back to the
  // identicon. `imgFailed` resets whenever the resolved URL changes.
  const [imgFailed, setImgFailed] = createSignal(false);

  const url = createMemo(() => {
    const u = avatarUrl(host.profile(props.pubkey));
    return u;
  });
  // Reset the failure flag when the URL changes (new pubkey / late resolution).
  createMemo<string | undefined>((prev) => {
    const u = url();
    if (u !== prev) setImgFailed(false);
    return u;
  });

  onMount(() => host.resolveProfileRef(props.pubkey, consumerId));
  onCleanup(() => host.releaseProfileRef(props.pubkey, consumerId));

  return (
    <span
      class="nostr-avatar"
      style={{
        width: `${size()}px`,
        height: `${size()}px`,
        "border-radius": "50%",
        display: "inline-flex",
        "align-items": "center",
        "justify-content": "center",
        overflow: "hidden",
        "flex-shrink": "0",
        "background-color": identiconColor(props.pubkey),
      }}
      aria-hidden="true"
    >
      <Show
        when={url() && !imgFailed()}
        fallback={
          <span
            style={{
              "font-size": `${size() * 0.35}px`,
              "font-weight": "600",
              color: "white",
              "line-height": "1",
            }}
          >
            {identiconInitials(props.pubkey)}
          </span>
        }
      >
        <img
          src={url()!}
          alt=""
          width={size()}
          height={size()}
          referrerPolicy="no-referrer"
          style={{ width: "100%", height: "100%", "object-fit": "cover", display: "block" }}
          onError={() => setImgFailed(true)}
        />
      </Show>
    </span>
  );
}

// Deterministic identicon from a raw pubkey hex string. Edit `PALETTE` to match
// your brand. Identical algorithm to the SwiftUI / Compose components so the
// fallback color/initials match across platforms.
const PALETTE = [
  "rgb(92, 51, 207)",
  "rgb(26, 135, 209)",
  "rgb(33, 140, 107)",
  "rgb(209, 84, 46)",
  "rgb(194, 38, 115)",
  "rgb(51, 51, 51)",
];

export function identiconColor(pubkey: string): string {
  let sum = 0;
  for (let i = 0; i < Math.min(4, pubkey.length); i += 1) {
    sum += pubkey.charCodeAt(i);
  }
  return PALETTE[sum % PALETTE.length]!;
}

export function identiconInitials(pubkey: string): string {
  if (pubkey.length < 2) return "?";
  return pubkey.slice(0, 2).toUpperCase();
}
