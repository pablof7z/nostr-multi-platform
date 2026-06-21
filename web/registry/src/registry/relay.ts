import { nativeSource } from "./vendorSource";
import type { Component } from "./types";

// Relay — SwiftUI
const nostrRelayListSwift = nativeSource("registry/swiftui/relay-list/NostrRelayList.swift");
const nostrRelayListPreviewSwift = nativeSource("registry/swiftui/relay-list/Examples/NostrRelayListPreview.swift");

// Relay — Web (SolidJS)
import nostrRelayListWeb from "@nmp/components/src/relay-list/NostrRelayList.tsx?raw";

// Render Identity — SwiftUI
const renderIdentifiableSwift = nativeSource("registry/swiftui/render-identity/RenderIdentifiable.swift");

export const relayComponents: Component[] = [
  {
    slug: "relay-list",
    routeId: "relay-list",
    version: "0.2.0",
    description: "Relay list showing relay URLs with role badges and live connection status dots.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/relay-list",
        version: "0.2.0",
        dependencies: ["render-identity"],
        longDescription:
          "`NostrRelayList` renders the `projections.configured_relays` array as a list of relay URLs with semantic role badges and animated connection status dots. Connection dots pulse on `.connecting` state. Pass `relayStatuses` to fold live connection state from the top-level `relay_statuses` snapshot field.",
        files: [
          { source: "swiftui/relay-list/NostrRelayList.swift", target: "Components/NostrRelays/NostrRelayList.swift", role: "source", content: nostrRelayListSwift },
          { source: "swiftui/relay-list/Examples/NostrRelayListPreview.swift", target: "Components/NostrRelays/Examples/NostrRelayListPreview.swift", role: "example", content: nostrRelayListPreviewSwift },
        ],
        screenshots: ["relay-list-ios-gallery-preview.png", "tui-relay-list-preview.png"],
        customization: [
          "Pass a `relayStatuses: [String: String]` dictionary keyed by relay URL to animate connection dots. Build it with `Dictionary(uniqueKeysWithValues: snapshot.relayStatuses.map { ($0.relayUrl, $0.connection) })`.",
          "Role badge colors map semantic tokens (`accent`, `info`, `success`, `neutral`) to SwiftUI system colors — override `tintColor(for:)` to match your brand.",
          "Edit `displayUrl` in `NostrRelayEditRow` to strip or preserve the `wss://` scheme prefix.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/relay-list",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`<NostrRelayList relays={...} />` renders relay rows folded from the kernel snapshot — `url` + `role` from `configured_relays`, `connection` from the top-level `relay_statuses` field — with a live connection-status dot (green connected / amber connecting+pulsing / red error / grey disconnected) and per-token role badges. Render-only; the host owns relay config. Verified live in the NMP web gallery against real relays (relay.primal.net + purplepag.es, both connected).",
        files: [
          { source: "web/relay-list/NostrRelayList.tsx", target: "src/components/nostr-relays/NostrRelayList.tsx", role: "source", content: nostrRelayListWeb },
        ],
        screenshots: ["relay-list-web-preview.png"],
        customization: [
          "Fold `relay_statuses` into each row's `connection` before passing `relays` (closed token set: connected | connecting | disconnected | error).",
          "Edit `connectionColor` / `roleTint` / `roleLabel` to match your theme; `displayUrl` strips the `wss://` scheme.",
          "Pass `onRelayTap` to make rows interactive; omit it for a read-only list.",
        ],
      },
    },
  },
  {
    slug: "render-identity",
    routeId: "render-identity",
    version: "0.1.0",
    description: "RenderIdentifiable protocol and EquatableRow helper for SwiftUI row equatability optimization.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/render-identity",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "Provides the `RenderIdentifiable` protocol and `EquatableRow` generic helper struct to optimize SwiftUI ForEach row re-evaluation. Wrap your row content in `EquatableRow(model:) { ... }.equatable()` to short-circuit body rebuilds when `rendersIdentically` returns true.",
        files: [
          { source: "swiftui/render-identity/RenderIdentifiable.swift", target: "Components/SwiftUI/RenderIdentifiable.swift", role: "source", content: renderIdentifiableSwift },
        ],
        screenshots: [],
        customization: [
          "Implement `RenderIdentifiable` on your row model type, comparing only the fields that affect visual rendering.",
          "Avoid comparing closures/callbacks — they're typically not equal even when semantically identical.",
          "Use alongside `@State` and `@Environment` to isolate view state from row data.",
        ],
      },
      web: {
        status: "stable",
        installId: "web/render-identity",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "Not applicable on the web — there is nothing to install. `render-identity` is a SwiftUI-specific optimization (`RenderIdentifiable` + `EquatableRow`) for short-circuiting `ForEach` row re-evaluation via `.equatable()`. SolidJS has no equivalent need: its fine-grained reactivity updates only the exact DOM bindings whose signals changed, so rows never re-evaluate wholesale and there is no row-equatability step to optimize. The web user/content components rely on Solid stores keyed per pubkey/event for the same effect. This entry exists so the component page documents the web stance rather than appearing unsupported.",
        files: [],
        screenshots: [],
        customization: [
          "Key your reactive stores by id (pubkey / event id) so a change to one row's data updates only that row — Solid's structural sharing is the web analogue of the SwiftUI equatable-row optimization.",
        ],
      },
    },
  },
];
