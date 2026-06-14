import type { Component } from "./types";

// User profile — SwiftUI
import profileWireSwift from "../vendor/swiftui/user-avatar/ProfileWire.swift?raw";
import nostrProfileHostSwift from "../vendor/swiftui/user-avatar/NostrProfileHost.swift?raw";
import nostrAvatarSwift from "../vendor/swiftui/user-avatar/NostrAvatar.swift?raw";
import nostrProfileNameSwift from "../vendor/swiftui/user-name/NostrProfileName.swift?raw";
import nostrNip05BadgeSwift from "../vendor/swiftui/user-nip05/NostrNip05Badge.swift?raw";
import nostrNpubChipSwift from "../vendor/swiftui/user-npub/NostrNpubChip.swift?raw";
import nostrUserCardSwift from "../vendor/swiftui/user-card/NostrUserCard.swift?raw";

// User profile — Compose
import profileWireKotlin from "../vendor/compose/user-avatar/ProfileWire.kt?raw";
import nostrProfileHostKotlin from "../vendor/compose/user-avatar/NostrProfileHost.kt?raw";
import nostrAvatarKotlin from "../vendor/compose/user-avatar/NostrAvatar.kt?raw";
import nostrProfileNameKotlin from "../vendor/compose/user-name/NostrProfileName.kt?raw";
import nostrNip05BadgeKotlin from "../vendor/compose/user-nip05/NostrNip05Badge.kt?raw";
import nostrNpubChipKotlin from "../vendor/compose/user-npub/NostrNpubChip.kt?raw";
import nostrUserCardKotlin from "../vendor/compose/user-card/NostrUserCard.kt?raw";

// User profile — Ratatui
import profileWireRust from "../vendor/tui/user-core/profile_wire.rs?raw";
import nostrAvatarRust from "../vendor/tui/user-avatar/nostr_avatar.rs?raw";
import nostrProfileNameRust from "../vendor/tui/user-name/nostr_profile_name.rs?raw";
import nostrNip05BadgeRust from "../vendor/tui/user-nip05/nostr_nip05_badge.rs?raw";
import nostrNpubChipRust from "../vendor/tui/user-npub/nostr_npub_chip.rs?raw";
import nostrUserCardRust from "../vendor/tui/user-card/nostr_user_card.rs?raw";

// User profile — Desktop (iced)
import profileWireDesktopRust from "../vendor/desktop/user-core/profile_wire.rs?raw";
import userAvatarDesktopRust from "../vendor/desktop/user-avatar/user_avatar.rs?raw";
import userNameDesktopRust from "../vendor/desktop/user-name/user_name.rs?raw";
import userNip05DesktopRust from "../vendor/desktop/user-nip05/user_nip05.rs?raw";
import userNpubDesktopRust from "../vendor/desktop/user-npub/user_npub.rs?raw";
import userCardDesktopRust from "../vendor/desktop/user-card/user_card.rs?raw";

// User profile — Web (SolidJS)
import profileWireWeb from "../vendor/web/user-avatar/ProfileWire.ts?raw";
import nostrProfileHostWeb from "../vendor/web/user-avatar/NostrProfileHost.tsx?raw";
import nostrAvatarWeb from "../vendor/web/user-avatar/NostrAvatar.tsx?raw";
import nostrProfileNameWeb from "../vendor/web/user-name/NostrProfileName.tsx?raw";
import nostrNip05BadgeWeb from "../vendor/web/user-nip05/NostrNip05Badge.tsx?raw";
import nostrUserCardWeb from "../vendor/web/user-card/NostrUserCard.tsx?raw";
import nostrNpubChipWeb from "../vendor/web/user-npub/NostrNpubChip.tsx?raw";

export const userComponents: Component[] = [
  {
    slug: "user-core",
    routeId: "user-core",
    version: "0.1.0",
    description: "Shared Ratatui ProfileWire mirror for Rust-owned user profile projections.",
    platforms: {
      tui: {
        status: "stable",
        installId: "tui/user-core",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`ProfileWire` is the Rust-side projection mirror used by the TUI user widgets. It carries display-ready profile fields from the kernel; host apps should not derive profile names or npub truncation in terminal UI code.",
        files: [
          { source: "tui/user-core/profile_wire.rs", target: "src/components/nostr_user/profile_wire.rs", role: "source", content: profileWireRust },
        ],
        screenshots: [],
        customization: [
          "Keep this type aligned with the kernel projection and use it as the input to the display widgets.",
        ],
      },
      desktop: {
        status: "stable",
        installId: "desktop/user-core",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`ProfileWire` is the Rust-side projection mirror used by the iced desktop user widgets. It carries display-ready profile fields (display name, nip05, npub_short) from the kernel; host apps build their iced views from this type rather than reformatting keys in widget code.",
        files: [
          { source: "desktop/user-core/profile_wire.rs", target: "src/components/nostr_user/profile_wire.rs", role: "source", content: profileWireDesktopRust },
        ],
        screenshots: [],
        customization: [
          "Keep this type aligned with the kernel projection and use it as the input to the iced display widgets.",
        ],
      },
      web: {
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
      },
    },
  },
  {
    slug: "user-avatar",
    routeId: "user-avatar",
    version: "0.1.0",
    description: "Reference-first avatar that claims and observes its profile projection.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-avatar",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`NostrAvatar(pubkey:)` claims/releases its own profile interest through `NostrProfileHost`, reads the current Rust-owned profile projection, and falls back to a deterministic identicon until the picture URL arrives. Install once; every other user component depends on the shared `ProfileWire`.",
        files: [
          { source: "swiftui/user-avatar/ProfileWire.swift", target: "Components/NostrUser/ProfileWire.swift", role: "source", content: profileWireSwift },
          { source: "swiftui/user-avatar/NostrProfileHost.swift", target: "Components/NostrUser/NostrProfileHost.swift", role: "source", content: nostrProfileHostSwift },
          { source: "swiftui/user-avatar/NostrAvatar.swift", target: "Components/NostrUser/NostrAvatar.swift", role: "source", content: nostrAvatarSwift },
        ],
        screenshots: ["user-avatar-ios-gallery-preview.png"],
        customization: [
          "Edit the `palette` array in `NostrIdenticon` to match your app's brand colors. The color is deterministic from the pubkey so the same user always gets the same color.",
          "Replace `AsyncImage` with your own image cache (Kingfisher, Nuke) by swapping the URL loading block — the identicon fallback is self-contained.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-avatar",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`NostrAvatar(pubkey = ...)` claims/releases its own profile interest through `LocalNostrProfileHost`, reads the current Rust-owned profile projection, and falls back to a deterministic identicon until the picture URL arrives. Install once; every other Compose user component depends on the shared `ProfileWire`.",
        files: [
          { source: "compose/user-avatar/ProfileWire.kt", target: "Components/NostrUser/ProfileWire.kt", role: "source", content: profileWireKotlin },
          { source: "compose/user-avatar/NostrProfileHost.kt", target: "Components/NostrUser/NostrProfileHost.kt", role: "source", content: nostrProfileHostKotlin },
          { source: "compose/user-avatar/NostrAvatar.kt", target: "Components/NostrUser/NostrAvatar.kt", role: "source", content: nostrAvatarKotlin },
        ],
        screenshots: ["user-avatar-kotlin-preview.png"],
        customization: [
          "Edit `IDENTICON_PALETTE` in `NostrAvatar.kt` to match your brand colors.",
          "Replace `SubcomposeAsyncImage` with Glide or a custom Painter — the identicon fallback composables don't depend on Coil.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-avatar",
        version: "0.1.1",
        dependencies: ["user-core"],
        longDescription:
          "`NostrAvatar::for_pubkey(pubkey, host)` claims its own profile interest through `NostrProfileHost`, reads the current Rust-owned profile projection each frame, accepts an optional `ratatui-image` protocol supplied by the host app, and falls back to deterministic initials until the image is available.",
        files: [
          { source: "tui/user-core/profile_wire.rs", target: "src/components/nostr_user/profile_wire.rs", role: "source", content: profileWireRust },
          { source: "tui/user-avatar/nostr_avatar.rs", target: "src/components/nostr_user/nostr_avatar.rs", role: "source", content: nostrAvatarRust },
        ],
        screenshots: ["tui-user-avatar-preview.png"],
        customization: [
          "Edit `PALETTE` in `nostr_avatar.rs` to match your terminal theme.",
          "The widget is render-only; host apps own image fetching, terminal protocol selection, and navigation.",
        ],
      },
      desktop: {
        status: "stable",
        installId: "desktop/user-avatar",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`UserAvatar::new(pubkey_hex)` is a self-contained iced widget that renders a circular avatar. With a pre-built `iced::widget::image::Handle` it clips the real profile picture to a circle; without one it falls back to a deterministic pubkey-derived tint plus initials, using `nmp_core::display` for the color and initials so the fallback matches every other surface. The host builds the `Handle` once in `update()` (never in `view()`) to avoid per-frame GPU re-uploads — so this widget takes no `ProfileWire` and has no dependencies.",
        files: [
          { source: "desktop/user-avatar/user_avatar.rs", target: "src/components/nostr_user/user_avatar.rs", role: "source", content: userAvatarDesktopRust },
        ],
        screenshots: ["user-avatar-desktop-preview.png"],
        customization: [
          "Tune the default `size` (36px) per call site via `.size(48.0)`; the circle radius tracks it automatically.",
          "The deterministic tint and initials come from `nmp_core::display` — host apps own image decoding and supply the `Handle`, keeping the widget render-only.",
        ],
      },
      web: {
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
      },
    },
  },
  {
    slug: "user-name",
    routeId: "user-name",
    version: "0.1.0",
    description: "Inline display-name text with fallback to Rust-truncated npub.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-name",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "swiftui/user-name/NostrProfileName.swift", target: "Components/NostrUser/NostrProfileName.swift", role: "source", content: nostrProfileNameSwift },
        ],
        screenshots: ["user-name-ios-gallery-preview.png"],
        customization: [
          "Pass any `Font` and `Color` — the component has no hardcoded styling. Use `.headline` for headers and `.subheadline` with a muted color for secondary rows.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-name",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "compose/user-name/NostrProfileName.kt", target: "Components/NostrUser/NostrProfileName.kt", role: "source", content: nostrProfileNameKotlin },
        ],
        screenshots: ["user-name-kotlin-preview.png"],
        customization: [
          "Pass any `TextStyle` and `Color` — no hardcoded styling. Use `MaterialTheme.typography.titleMedium` for headers and `bodySmall` for secondary rows.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-name",
        version: "0.1.0",
        dependencies: ["user-core"],
        files: [
          { source: "tui/user-name/nostr_profile_name.rs", target: "src/components/nostr_user/nostr_profile_name.rs", role: "source", content: nostrProfileNameRust },
        ],
        screenshots: ["tui-user-name-preview.png"],
        customization: [
          "Pass a Ratatui `Style` with `.style(...)`; the fallback label still comes from `ProfileWire::display()`.",
        ],
      },
      desktop: {
        status: "stable",
        installId: "desktop/user-name",
        version: "0.1.0",
        dependencies: ["user-core"],
        longDescription:
          "`UserName::from_profile(&ProfileWire)` renders the display name as bold iced `text`, falling back to the muted Rust-truncated `npub_short` when no name is present. It clones the display fields at construction so the returned `Element` is `'static` and can be returned from `view()` without lifetime juggling.",
        files: [
          { source: "desktop/user-name/user_name.rs", target: "src/components/nostr_user/user_name.rs", role: "source", content: userNameDesktopRust },
        ],
        screenshots: ["user-name-desktop-preview.png"],
        customization: [
          "Adjust the `.size(16)` and bold `Weight` in `user_name.rs` to match your typographic scale.",
          "The npub fallback uses the kernel-formatted `ProfileWire::npub_short` — never reformat keys in iced code.",
        ],
      },
      web: {
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
      },
    },
  },
  {
    slug: "user-nip05",
    routeId: "user-nip05",
    version: "0.1.0",
    description: "NIP-05 verified identity badge — checkmark icon and identifier string.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-nip05",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "swiftui/user-nip05/NostrNip05Badge.swift", target: "Components/NostrUser/NostrNip05Badge.swift", role: "source", content: nostrNip05BadgeSwift },
        ],
        screenshots: ["user-nip05-ios-gallery-preview.png"],
        customization: [
          "The failable `init?(profile:)` lets you gate the badge in one line: `if let badge = NostrNip05Badge(profile: profile) { badge }`.",
          "`_@domain` identifiers (root-domain NIP-05) automatically render as just `domain` — no extra handling needed.",
          "Swap `Color.accentColor` for your brand verification color on the checkmark icon.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-nip05",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "compose/user-nip05/NostrNip05Badge.kt", target: "Components/NostrUser/NostrNip05Badge.kt", role: "source", content: nostrNip05BadgeKotlin },
        ],
        screenshots: ["user-nip05-kotlin-preview.png"],
        customization: [
          "`NostrNip05Badge(profile)` returns early when nip05 is absent; `NostrNip05Badge(nip05)` renders unconditionally when you've already validated the value.",
          "`_@domain` identifiers (root-domain NIP-05) automatically render as just `domain` — no extra handling needed.",
          "Swap `MaterialTheme.colorScheme.primary` for your brand verification color on the icon tint.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-nip05",
        version: "0.1.0",
        dependencies: ["user-core"],
        files: [
          { source: "tui/user-nip05/nostr_nip05_badge.rs", target: "src/components/nostr_user/nostr_nip05_badge.rs", role: "source", content: nostrNip05BadgeRust },
        ],
        screenshots: ["tui-user-nip05-preview.png"],
        customization: [
          "`NostrNip05Badge::from_profile` returns `None` when the projection has no identifier, so callers can skip the row cleanly.",
          "`_@domain` identifiers (root-domain NIP-05) automatically render as just `domain` — no extra handling needed.",
        ],
      },
      desktop: {
        status: "stable",
        installId: "desktop/user-nip05",
        version: "0.1.0",
        dependencies: ["user-core"],
        longDescription:
          "`Nip05Badge::from_profile(&ProfileWire)` is a failable constructor returning `None` when the projection carries no NIP-05 identifier, so callers gate the row in one line. When present it renders a green checkmark plus the identifier as an iced `row`. A leading `_@` (the NIP-05 root-domain convention) is elided, so `_@f7z.io` shows as the bare domain `f7z.io`.",
        files: [
          { source: "desktop/user-nip05/user_nip05.rs", target: "src/components/nostr_user/user_nip05.rs", role: "source", content: userNip05DesktopRust },
        ],
        screenshots: ["user-nip05-desktop-preview.png"],
        customization: [
          "`Nip05Badge::from_profile` returns `None` for missing identifiers — match on it to skip the row cleanly.",
          "`_@domain` root-domain identifiers automatically render as just `domain`; swap the `GREEN` constant for your brand verification color.",
        ],
      },
      web: {
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
      },
    },
  },
  {
    slug: "user-npub",
    routeId: "user-npub",
    version: "0.1.0",
    description: "Tappable npub chip — shows Rust-truncated npub and copies full bech32 on tap.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-npub",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "swiftui/user-npub/NostrNpubChip.swift", target: "Components/NostrUser/NostrNpubChip.swift", role: "source", content: nostrNpubChipSwift },
        ],
        screenshots: ["user-npub-ios-gallery-preview.png"],
        customization: [
          "`npub` and `npubShort` must come from the kernel projection — never format them in Swift (aim.md §6.9).",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-npub",
        version: "0.1.0",
        dependencies: ["user-avatar"],
        files: [
          { source: "compose/user-npub/NostrNpubChip.kt", target: "Components/NostrUser/NostrNpubChip.kt", role: "source", content: nostrNpubChipKotlin },
        ],
        screenshots: ["user-npub-kotlin-preview.png"],
        customization: [
          "`npub` and `npubShort` must come from the kernel projection — never format them in Kotlin.",
          "Uses `ClipboardManager` directly; no permission required on API 32 and below.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-npub",
        version: "0.1.0",
        dependencies: ["user-core"],
        files: [
          { source: "tui/user-npub/nostr_npub_chip.rs", target: "src/components/nostr_user/nostr_npub_chip.rs", role: "source", content: nostrNpubChipRust },
        ],
        screenshots: ["tui-user-npub-preview.png"],
        customization: [
          "Clipboard writes are host capabilities; bind your copy key to `profile.npub` outside the widget.",
        ],
      },
      desktop: {
        status: "stable",
        installId: "desktop/user-npub",
        version: "0.1.0",
        dependencies: ["user-core"],
        longDescription:
          "`NpubChip::from_profile(&ProfileWire)` renders the Rust-truncated `npub_short` in a monospace iced chip — a rounded container with a slate background and muted foreground. Display-only: clipboard writes are a host capability, bound to the full `ProfileWire::npub` outside the widget.",
        files: [
          { source: "desktop/user-npub/user_npub.rs", target: "src/components/nostr_user/user_npub.rs", role: "source", content: userNpubDesktopRust },
        ],
        screenshots: ["user-npub-desktop-preview.png"],
        customization: [
          "Tune the `BG`/`FG` constants and chip padding/radius in `user_npub.rs` to match your theme.",
          "`npub_short` comes from the kernel projection — never format keys in iced; wire copy-to-clipboard to `ProfileWire::npub` at the call site.",
        ],
      },
      web: {
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
      },
    },
  },
  {
    slug: "user-card",
    routeId: "user-card",
    version: "0.1.0",
    description: "Compact author header: avatar, display name, and optional NIP-05 badge.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/user-card",
        version: "0.1.0",
        dependencies: ["user-avatar", "user-name", "user-nip05"],
        longDescription:
          "The most common pattern in note feeds and thread views. Composes `NostrAvatar`, `NostrProfileName`, and `NostrNip05Badge` into a single tappable row. Tap routes through an `onTap` callback so it works in any navigation stack.",
        files: [
          { source: "swiftui/user-card/NostrUserCard.swift", target: "Components/NostrUser/NostrUserCard.swift", role: "source", content: nostrUserCardSwift },
        ],
        screenshots: ["user-card-ios-gallery-preview.png"],
        customization: [
          "Set `avatarSize` to `32` for dense list rows and `64` for profile headers.",
          "The `onTap` callback receives the raw pubkey — push your own profile route from there rather than hardcoding any navigation dependency inside this component.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/user-card",
        version: "0.1.0",
        dependencies: ["user-avatar", "user-name", "user-nip05"],
        longDescription:
          "The most common pattern in note feeds and thread views. Composes `NostrAvatar`, `NostrProfileName`, and `NostrNip05Badge` into a single tappable row. Tap routes through an `onTap` callback so it works with any Compose navigation setup.",
        files: [
          { source: "compose/user-card/NostrUserCard.kt", target: "Components/NostrUser/NostrUserCard.kt", role: "source", content: nostrUserCardKotlin },
        ],
        screenshots: ["user-card-kotlin-preview.png"],
        customization: [
          "Set `avatarSize` to `32.dp` for dense list rows and `64.dp` for profile headers.",
          "The `onTap` callback receives the raw pubkey — push your own NavController route from there.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/user-card",
        version: "0.1.1",
        dependencies: ["user-core", "user-avatar", "user-name", "user-nip05"],
        longDescription:
          "A compact terminal author header composed from `NostrAvatar`, `NostrProfileName`, and `NostrNip05Badge`. Host apps pass an optional avatar image protocol and handle selection/profile navigation in their input loop.",
        files: [
          { source: "tui/user-card/nostr_user_card.rs", target: "src/components/nostr_user/nostr_user_card.rs", role: "source", content: nostrUserCardRust },
        ],
        screenshots: ["tui-user-card-preview.png"],
        customization: [
          "Adjust the outer `Block` style in `nostr_user_card.rs` for dense feeds, modal headers, or focused rows.",
        ],
      },
      desktop: {
        status: "stable",
        installId: "desktop/user-card",
        version: "0.1.0",
        dependencies: ["user-core", "user-avatar"],
        longDescription:
          "`UserCard::from_profile(&ProfileWire)` composes the iced `UserAvatar` widget with a bold display name (or muted npub fallback) and an optional NIP-05 row into a single avatar + label `row`. It clones the display fields for a `'static` element and accepts an optional avatar image `Handle` forwarded to the embedded avatar. The NIP-05 `_@` root prefix is elided to match the standalone badge.",
        files: [
          { source: "desktop/user-card/user_card.rs", target: "src/components/nostr_user/user_card.rs", role: "source", content: userCardDesktopRust },
        ],
        screenshots: ["user-card-desktop-preview.png"],
        customization: [
          "Pass a pre-built image `Handle` via `.avatar_handle(handle)` so the embedded avatar shows the real picture instead of initials.",
          "Adjust the avatar `.size(40.0)` and row spacing in `user_card.rs` for dense list rows versus profile headers.",
        ],
      },
      web: {
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
      },
    },
  },
];
