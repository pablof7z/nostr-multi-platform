import type { PlatformImpl } from "./types";

import nostrAvatarWeb from "@nmp/components-web/src/user-avatar/NostrAvatar.tsx?raw";
import nostrProfileHostWeb from "@nmp/components-web/src/user-avatar/NostrProfileHost.tsx?raw";
import profileWireWeb from "@nmp/components-web/src/user-avatar/ProfileWire.ts?raw";
import nostrUserCardWeb from "@nmp/components-web/src/user-card/NostrUserCard.tsx?raw";
import nostrProfileNameWeb from "@nmp/components-web/src/user-name/NostrProfileName.tsx?raw";
import nostrNip05BadgeWeb from "@nmp/components-web/src/user-nip05/NostrNip05Badge.tsx?raw";
import nostrNpubChipWeb from "@nmp/components-web/src/user-npub/NostrNpubChip.tsx?raw";

export const webUserCore: PlatformImpl = {
  status: "stable",
  installId: "web/user-core",
  version: "0.1.0",
  dependencies: [],
  longDescription:
    "`ProfileWire` is the web-side mirror of the kernel's `resolved_profiles` (KRPR) projection — the shared wire type every web user-* component renders. It carries display-ready fields (display name, picture URL, nip05, lnurl, optional Rust-formatted `npubShort`) plus pure helpers (`avatarUrl`, `displayLabel`, `shortHex`). The hex fallback is honest raw protocol data — npubs are never bech32-encoded in the browser (aim.md §6.9). On web this type co-locates with `user-avatar` (web doesn't split the renderer from the wire type the way the Rust platforms do), so installing any user-* component brings it in.",
  files: [
    { source: "web/user-avatar/ProfileWire.ts", target: "src/components/nostr-user/ProfileWire.ts", role: "source", content: profileWireWeb },
  ],
  screenshots: ["user-core-web-preview.png"],
  customization: [
    "Keep this interface aligned with the kernel projection; the user-* components are pure renderers over it.",
    "Populate `npubShort` from the kernel's Rust NIP-19 encoder (see user-npub) rather than deriving an npub in the browser.",
  ],
};

export const webUserAvatar: PlatformImpl = {
  status: "stable",
  installId: "web/user-avatar",
  version: "0.1.0",
  dependencies: [],
  longDescription:
    "`<NostrAvatar pubkey={...} />` is a SolidJS component that claims/releases its own profile interest through the `NostrProfileHost` context, reads the resolved `ProfileWire`, and shows the real kind:0 picture — falling back to a deterministic pubkey-derived identicon (same palette + initials algorithm as SwiftUI/Compose) until the picture loads or when the profile has none. The host wires `NostrProfileHost` to the real WASM kernel (claim → kind:0 fetch). Verified live in the NMP web gallery against real relays.",
  files: [
    { source: "web/user-avatar/ProfileWire.ts", target: "src/components/nostr-user/ProfileWire.ts", role: "source", content: profileWireWeb },
    { source: "web/user-avatar/NostrProfileHost.tsx", target: "src/components/nostr-user/NostrProfileHost.tsx", role: "source", content: nostrProfileHostWeb },
    { source: "web/user-avatar/NostrAvatar.tsx", target: "src/components/nostr-user/NostrAvatar.tsx", role: "source", content: nostrAvatarWeb },
  ],
  screenshots: ["user-avatar-web-preview.png"],
  customization: [
    "Edit the `PALETTE` array in `NostrAvatar.tsx` to match your brand; the color is deterministic from the pubkey so a user always gets the same one.",
    "Swap the `<img>` for your own image cache component — the identicon fallback (`<span>` with initials) is self-contained.",
    "Implement `NostrProfileHost` over your kernel host: `profile(pubkey)` reads the resolved projection reactively, `claimProfile`/`releaseProfile` register interest on mount/cleanup.",
  ],
};

export const webUserName: PlatformImpl = {
  status: "stable",
  installId: "web/user-name",
  version: "0.1.0",
  dependencies: ["user-avatar"],
  longDescription:
    "`<NostrProfileName profile={...} />` renders the resolved display name, falling back to `npubShort` (Rust-formatted, when the kernel emits it) and finally to a short raw-hex form of the pubkey. Pure render of the `ProfileWire` it is given — the host owns resolution. Verified live in the NMP web gallery (real kind:0 display name).",
  files: [
    { source: "web/user-name/NostrProfileName.tsx", target: "src/components/nostr-user/NostrProfileName.tsx", role: "source", content: nostrProfileNameWeb },
  ],
  screenshots: ["user-name-web-preview.png"],
  customization: [
    "Style via the `nostr-profile-name` class (or pass `class`); the component emits a single `<span>`.",
    "The hex fallback is honest raw protocol data — do not bech32-encode an npub in the browser; surface `npubShort` from the kernel projection instead.",
  ],
};

export const webUserNip05: PlatformImpl = {
  status: "stable",
  installId: "web/user-nip05",
  version: "0.1.0",
  dependencies: ["user-avatar"],
  longDescription:
    "`<NostrNip05Badge profile={...} />` renders a verification seal glyph plus the NIP-05 identifier, and renders nothing when the profile carries no nip05. A leading `_@` (the NIP-05 root-domain convention) is elided, so `_@f7z.io` shows as the bare domain `f7z.io`. Verified live in the NMP web gallery (real kind:0 nip05).",
  files: [
    { source: "web/user-nip05/NostrNip05Badge.tsx", target: "src/components/nostr-user/NostrNip05Badge.tsx", role: "source", content: nostrNip05BadgeWeb },
  ],
  screenshots: ["user-nip05-web-preview.png"],
  customization: [
    "Style via the `nostr-nip05-badge` / `nostr-nip05-text` classes; swap the inline seal `<svg>` for your brand verification mark.",
    "`_@domain` root-domain identifiers automatically render as just `domain`.",
  ],
};

export const webUserNpub: PlatformImpl = {
  status: "stable",
  installId: "web/user-npub",
  version: "0.1.0",
  dependencies: ["user-core"],
  longDescription:
    "`<NostrNpubChip npub={...} npubShort={...} />` is the copyable short-npub chip. Both forms come from the canonical Rust NIP-19 encoder exposed through the WASM module (`nmp_encode_npub`) — never bech32-encoded or truncated in the browser (aim.md §6.9). Renders the short form as a monospace chip; clicking copies the full npub to the clipboard. Verified live in the NMP web gallery: the showcase identity's npub is encoded by the real kernel and matches the curated reference exactly.",
  files: [
    { source: "web/user-npub/NostrNpubChip.tsx", target: "src/components/nostr-user/NostrNpubChip.tsx", role: "source", content: nostrNpubChipWeb },
  ],
  screenshots: ["user-npub-web-preview.png"],
  customization: [
    "Obtain `npub`/`npubShort` from your kernel boundary (the gallery adds a worker `encode_npub` round-trip to the Rust encoder) — do not bech32-encode in JS.",
    "Clipboard writes use `navigator.clipboard`; swap in your own copy affordance if you need a fallback for non-secure contexts.",
  ],
};

export const webUserCard: PlatformImpl = {
  status: "stable",
  installId: "web/user-card",
  version: "0.1.0",
  dependencies: ["user-avatar", "user-name", "user-nip05"],
  longDescription:
    "`<NostrUserCard profile={...} onTap={...} />` composes the avatar (reference-first, claims its own profile), the display name, and the NIP-05 badge into the compact author header used across feeds and thread views. Renders as a tappable `<button>` routing through `onTap`. Verified live in the NMP web gallery against real relays.",
  files: [
    { source: "web/user-card/NostrUserCard.tsx", target: "src/components/nostr-user/NostrUserCard.tsx", role: "source", content: nostrUserCardWeb },
  ],
  screenshots: ["user-card-web-preview.png"],
  customization: [
    "Style via the `nostr-user-card` class; the component composes `NostrAvatar` + `NostrProfileName` + `NostrNip05Badge`.",
    "Wire `onTap` to your router to open the author's profile; omit it for a non-interactive header.",
  ],
};
