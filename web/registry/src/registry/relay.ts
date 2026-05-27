import type { Component } from "./types";

// Relay — SwiftUI
import nostrRelayListSwift from "../vendor/swiftui/relay-list/NostrRelayList.swift?raw";
import nostrRelayListPreviewSwift from "../vendor/swiftui/relay-list/Examples/NostrRelayListPreview.swift?raw";

export const relayComponents: Component[] = [
  {
    slug: "relay-list",
    routeId: "relay-list",
    version: "0.1.0",
    description: "Relay list showing relay URLs with role badges and live connection status dots.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/relay-list",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`NostrRelayList` renders the `projections.relay_edit_rows` array as a list of relay URLs with semantic role badges and animated connection status dots. Connection dots pulse on `.connecting` state. Pass `relayStatuses` to fold live connection state from the top-level `relay_statuses` snapshot field.",
        files: [
          { source: "swiftui/relay-list/NostrRelayList.swift", target: "Components/NostrRelays/NostrRelayList.swift", role: "source", content: nostrRelayListSwift },
          { source: "swiftui/relay-list/Examples/NostrRelayListPreview.swift", target: "Components/NostrRelays/Examples/NostrRelayListPreview.swift", role: "example", content: nostrRelayListPreviewSwift },
        ],
        screenshots: ["relay-list-ios-gallery-preview.png"],
        customization: [
          "Pass a `relayStatuses: [String: String]` dictionary keyed by relay URL to animate connection dots. Build it with `Dictionary(uniqueKeysWithValues: snapshot.relayStatuses.map { ($0.relayUrl, $0.connection) })`.",
          "Role badge colors map semantic tokens (`accent`, `info`, `success`, `neutral`) to SwiftUI system colors — override `tintColor(for:)` to match your brand.",
          "Edit `displayUrl` in `NostrRelayEditRow` to strip or preserve the `wss://` scheme prefix.",
        ],
      },
    },
  },
];
