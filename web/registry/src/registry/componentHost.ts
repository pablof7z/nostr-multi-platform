import { nativeSource } from "./vendorSource";
import type { Component } from "./types";

const swiftuiComponentHostSwift = nativeSource("registry/swiftui/component-host/NmpComponentHost.swift");
const swiftuiComponentHostFixtureSwift = nativeSource("registry/swiftui/component-host/NmpComponentHostConformance.swift");
const composeComponentHostKotlin = nativeSource("registry/compose/component-host/NmpComponentHostProvider.kt");
const composeComponentHostFixtureKotlin = nativeSource("registry/compose/component-host/NmpComponentHostConformance.kt");

export const componentHostComponents: Component[] = [
  {
    slug: "component-host",
    routeId: "component-host",
    version: "0.1.0",
    description: "App-root host/provider for NMP registry component profile and embed context.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/component-host",
        version: "0.1.0",
        dependencies: ["user-avatar", "content-kind-registry"],
        longDescription:
          "`NmpComponentHost` and `.nmpComponentHost(...)` bind `NostrProfileHost`, `EmbedEnvelopeSource`, `EventRefResolverProtocol`, and `NostrKindRegistry` once at the SwiftUI app root. Components below still read the existing lower-level environment values and own only visible resolve/release lifecycle.",
        files: [
          { source: "swiftui/component-host/NmpComponentHost.swift", target: "Components/NmpComponentHost/NmpComponentHost.swift", role: "source", content: swiftuiComponentHostSwift },
          { source: "swiftui/component-host/NmpComponentHostConformance.swift", target: "Components/NmpComponentHost/Fixtures/NmpComponentHostConformance.swift", role: "fixture", content: swiftuiComponentHostFixtureSwift },
        ],
        screenshots: [],
        customization: [
          "Create the concrete profile host, embed source, event-ref resolver, and kind registry in your app shell, then pass those app-owned bridge objects to `.nmpComponentHost(...)`.",
          "`refs.profile` remains the profile source and `refs.event.envelopes` remains a derived render sidecar; the host wrapper does not own kernel handles or event parsing.",
          "Install with `--with fixture` to copy the in-memory conformance harness for app tests that must run without a live kernel.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/component-host",
        version: "0.1.0",
        dependencies: ["user-avatar", "content-kind-registry"],
        longDescription:
          "`NmpComponentHostProvider` binds `LocalNostrProfileHost`, `LocalResolvedEventEmbeds`, `LocalEventRefResolver`, and `LocalNostrKindRegistry` once at the Compose app root. Components below still consume the existing locals and own only visible resolve/release lifecycle.",
        files: [
          { source: "compose/component-host/NmpComponentHostProvider.kt", target: "Components/NmpComponentHost/NmpComponentHostProvider.kt", role: "source", content: composeComponentHostKotlin },
          { source: "compose/component-host/NmpComponentHostConformance.kt", target: "Components/NmpComponentHost/Fixtures/NmpComponentHostConformance.kt", role: "fixture", content: composeComponentHostFixtureKotlin },
        ],
        screenshots: [],
        customization: [
          "Create the concrete profile host, event-ref resolver, resolved-envelope map, and kind registry in your app shell, then pass those app-owned bridge objects to `NmpComponentHostProvider(...)`.",
          "`LocalResolvedEventEmbeds` mirrors derived `refs.event.envelopes`, not authoritative event rows; Rust/NMP still owns protocol parsing and projection production.",
          "Install with `--with fixture` to copy the in-memory conformance harness for app tests that must run without a live kernel.",
        ],
      },
    },
  },
];
