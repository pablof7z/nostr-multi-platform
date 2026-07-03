import { nativeSource } from "./vendorSource";
import type { Component } from "./types";

const chatCoreSwift = nativeSource("registry/swiftui/chat-core/NostrGroupChatWire.swift");
const chatMessageRowSwift = nativeSource("registry/swiftui/chat-message-row/NostrGroupMessageRow.swift");
const chatComposerSwift = nativeSource("registry/swiftui/chat-composer/NostrGroupComposer.swift");
const chatRosterListSwift = nativeSource("registry/swiftui/chat-roster-list/NostrGroupRosterList.swift");

const chatCoreKotlin = nativeSource("registry/compose/chat-core/NostrGroupChatWire.kt");
const chatMessageRowKotlin = nativeSource("registry/compose/chat-message-row/NostrGroupMessageRow.kt");
const chatComposerKotlin = nativeSource("registry/compose/chat-composer/NostrGroupComposer.kt");
const chatRosterListKotlin = nativeSource("registry/compose/chat-roster-list/NostrGroupRosterList.kt");

const chatCoreRust = nativeSource("registry/tui/chat-core/nostr_group_chat_wire.rs");
const chatMessageRowRust = nativeSource("registry/tui/chat-message-row/nostr_group_message_row.rs");
const chatComposerRust = nativeSource("registry/tui/chat-composer/nostr_group_composer.rs");
const chatRosterListRust = nativeSource("registry/tui/chat-roster-list/nostr_group_roster_list.rs");

export const chatComponents: Component[] = [
  {
    slug: "chat-core",
    routeId: "chat-core",
    version: "0.1.0",
    description: "Shared group-chat wire structs for Rust-owned message, reaction, and roster projections.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/chat-core",
        version: "0.1.0",
        dependencies: [],
        files: [{ source: "swiftui/chat-core/NostrGroupChatWire.swift", target: "Components/NostrChat/NostrGroupChatWire.swift", role: "source", content: chatCoreSwift }],
        screenshots: [],
        customization: [
          "Keep these wire structs aligned with the Rust-owned group-chat projection; do not parse Nostr tags in SwiftUI.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/chat-core",
        version: "0.1.0",
        dependencies: [],
        files: [{ source: "compose/chat-core/NostrGroupChatWire.kt", target: "Components/NostrChat/NostrGroupChatWire.kt", role: "source", content: chatCoreKotlin }],
        screenshots: [],
        customization: [
          "Keep these wire structs aligned with the Rust-owned group-chat projection; do not parse Nostr tags in Compose.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/chat-core",
        version: "0.1.0",
        dependencies: [],
        files: [{ source: "tui/chat-core/nostr_group_chat_wire.rs", target: "src/components/nostr_chat/nostr_group_chat_wire.rs", role: "source", content: chatCoreRust }],
        screenshots: [],
        customization: [
          "Keep these wire structs aligned with the Rust-owned group-chat projection; do not parse Nostr tags in the TUI.",
        ],
      },
    },
  },
  {
    slug: "chat-message-row",
    routeId: "chat-message-row",
    version: "0.1.0",
    description: "Group-chat message row with profile self-claiming, reply preview, outgoing bubble styling, and reaction badges.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/chat-message-row",
        version: "0.1.0",
        dependencies: ["chat-core", "user-avatar", "user-name"],
        files: [{ source: "swiftui/chat-message-row/NostrGroupMessageRow.swift", target: "Components/NostrChat/NostrGroupMessageRow.swift", role: "source", content: chatMessageRowSwift }],
        screenshots: [],
        customization: [
          "Feed it `NostrGroupChatMessageWire` rows from Rust projections. Profile name/avatar resolution stays component-owned through the user components.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/chat-message-row",
        version: "0.1.0",
        dependencies: ["chat-core", "user-avatar", "user-name"],
        files: [{ source: "compose/chat-message-row/NostrGroupMessageRow.kt", target: "Components/NostrChat/NostrGroupMessageRow.kt", role: "source", content: chatMessageRowKotlin }],
        screenshots: [],
        customization: [
          "Feed it `NostrGroupChatMessageWire` rows from Rust projections. Profile name/avatar resolution stays component-owned through the user components.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/chat-message-row",
        version: "0.1.1",
        dependencies: ["chat-core", "user-avatar", "user-name"],
        files: [{ source: "tui/chat-message-row/nostr_group_message_row.rs", target: "src/components/nostr_chat/nostr_group_message_row.rs", role: "source", content: chatMessageRowRust }],
        screenshots: [],
        customization: [
          "Feed it `NostrGroupChatMessageWire` rows from Rust projections. Profile name/avatar resolution stays component-owned through the TUI user components.",
        ],
      },
    },
  },
  {
    slug: "chat-composer",
    routeId: "chat-composer",
    version: "0.1.0",
    description: "Group-chat composer that owns draft UI only and emits send callbacks without publishing or parsing protocol data.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/chat-composer",
        version: "0.1.0",
        dependencies: ["chat-core"],
        files: [{ source: "swiftui/chat-composer/NostrGroupComposer.swift", target: "Components/NostrChat/NostrGroupComposer.swift", role: "source", content: chatComposerSwift }],
        screenshots: [],
        customization: [
          "Route `onSend` into your Rust-owned group action. The component owns only draft text and disabled state.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/chat-composer",
        version: "0.1.0",
        dependencies: ["chat-core"],
        files: [{ source: "compose/chat-composer/NostrGroupComposer.kt", target: "Components/NostrChat/NostrGroupComposer.kt", role: "source", content: chatComposerKotlin }],
        screenshots: [],
        customization: [
          "Route `onSend` into your Rust-owned group action. The component owns only draft text and disabled state.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/chat-composer",
        version: "0.1.0",
        dependencies: ["chat-core"],
        files: [{ source: "tui/chat-composer/nostr_group_composer.rs", target: "src/components/nostr_chat/nostr_group_composer.rs", role: "source", content: chatComposerRust }],
        screenshots: [],
        customization: [
          "Route trimmed drafts into your Rust-owned group action. The component owns only display state.",
        ],
      },
    },
  },
  {
    slug: "chat-roster-list",
    routeId: "chat-roster-list",
    version: "0.1.0",
    description: "Group roster list that renders Rust-owned participant rows through self-claiming profile components.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/chat-roster-list",
        version: "0.1.0",
        dependencies: ["chat-core", "user-avatar", "user-name"],
        files: [{ source: "swiftui/chat-roster-list/NostrGroupRosterList.swift", target: "Components/NostrChat/NostrGroupRosterList.swift", role: "source", content: chatRosterListSwift }],
        screenshots: [],
        customization: [
          "Pass participant rows from the NIP-29 roster projection. The row components own profile self-claims.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/chat-roster-list",
        version: "0.1.0",
        dependencies: ["chat-core", "user-avatar", "user-name"],
        files: [{ source: "compose/chat-roster-list/NostrGroupRosterList.kt", target: "Components/NostrChat/NostrGroupRosterList.kt", role: "source", content: chatRosterListKotlin }],
        screenshots: [],
        customization: [
          "Pass participant rows from the NIP-29 roster projection. The row components own profile self-claims.",
        ],
      },
      tui: {
        status: "stable",
        installId: "tui/chat-roster-list",
        version: "0.1.0",
        dependencies: ["chat-core", "user-avatar", "user-name"],
        files: [{ source: "tui/chat-roster-list/nostr_group_roster_list.rs", target: "src/components/nostr_chat/nostr_group_roster_list.rs", role: "source", content: chatRosterListRust }],
        screenshots: [],
        customization: [
          "Pass participant rows from the NIP-29 roster projection. The rows own visible profile self-claims through the TUI user components.",
        ],
      },
    },
  },
];
