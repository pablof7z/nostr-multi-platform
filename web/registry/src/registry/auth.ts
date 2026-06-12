import type { Component } from "./types";

// Auth - SwiftUI
import loginBlockSwift from "../vendor/swiftui/login-block/NostrLoginBlock.swift?raw";

// Auth - Compose (ADR-0048 Stage 2: NIP-55 login-block)
import composeLoginBlockKotlin from "../vendor/compose/login-block/NostrLoginBlock.kt?raw";
import composeExternalSignerBridgeKotlin from "../vendor/compose/login-block/ExternalSignerCapabilityBridge.kt?raw";
import composeExternalSignerWireKotlin from "../vendor/compose/login-block/ExternalSignerWire.kt?raw";
import composeAmberIntentCodecKotlin from "../vendor/compose/login-block/AmberIntentCodec.kt?raw";

export const authComponents: Component[] = [
  {
    slug: "login-block",
    routeId: "login-block",
    version: "0.1.0",
    description:
      "Login UI with Amber, Primal, and other local Nostr signer detection, plus a manual key entry fallback.",
    platforms: {
      swiftui: {
        status: "stable",
        installId: "swiftui/login-block",
        version: "0.1.0",
        dependencies: ["content-core"],
        longDescription:
          "`NostrLoginBlock` probes the device for installed Nostr signer apps (Amber, Primal, nostrconnect-compatible) via `UIApplication.canOpenURL` and surfaces each one as a tappable card. If no signers are found it shows only the manual key entry option with an install hint. Detection happens lazily in `.task {}` — never at module load — so `UIApplication.shared` is always fully active when the probe runs.",
        files: [
          {
            source: "swiftui/login-block/NostrLoginBlock.swift",
            target: "Components/Auth/NostrLoginBlock.swift",
            role: "source",
            content: loginBlockSwift,
          },
        ],
        screenshots: ["login-block-ios-gallery-preview.png"],
        customization: [
          "Add `LSApplicationQueriesSchemes` to your app's Info.plist listing `nostrsigner`, `primal`, and `nostrconnect`. Without this entry `canOpenURL` always returns `false`, even when the signer is installed.",
          "Extend `NostrSignerDetector.knownSigners` to add future signer apps. Each entry needs its URL scheme listed in Info.plist too.",
          "Theming is driven by `NostrContentRenderer` from the `swiftui/content-core` dependency. Override colors with `.nostrContentRenderer(...)` on a parent view.",
          "Wire `onSignerSelected` to your NIP-46 Nostr Connect or NIP-55 deep-link flow. The `NostrSignerInfo.urlScheme` value is the scheme to use when constructing the handshake URL.",
        ],
      },
      compose: {
        status: "stable",
        installId: "compose/login-block",
        version: "0.1.0",
        dependencies: [],
        longDescription:
          "`NostrLoginBlock` detects installed Nostr signer apps (currently Amber / `nostrsigner:` via Android `PackageManager.queryIntentActivities`) and surfaces each as a one-tap sign-in card. Falls back to a manual key-entry row when no signers are found. The `ExternalSignerCapabilityBridge` handles the D7 host contract: fires the Rust-built `ExternalSignerRequest` as either an Android Intent round-trip or a `ContentResolver` fast-path (post-permission-grant), and reports raw results back unchanged. Status flipped to `stable` when the Stage-4 emulator E2E (Amber APK installed, sign-in → publish kind:1 → event signed by Amber key) passed (ADR-0048 D7; 2026-06-12, event `11652d49…`).",
        files: [
          {
            source: "compose/login-block/NostrLoginBlock.kt",
            target: "Components/Auth/NostrLoginBlock.kt",
            role: "source",
            content: composeLoginBlockKotlin,
          },
          {
            source: "compose/login-block/ExternalSignerCapabilityBridge.kt",
            target: "Components/Auth/ExternalSignerCapabilityBridge.kt",
            role: "source",
            content: composeExternalSignerBridgeKotlin,
          },
          {
            source: "compose/login-block/ExternalSignerWire.kt",
            target: "Components/Auth/ExternalSignerWire.kt",
            role: "source",
            content: composeExternalSignerWireKotlin,
          },
          {
            source: "compose/login-block/AmberIntentCodec.kt",
            target: "Components/Auth/AmberIntentCodec.kt",
            role: "source",
            content: composeAmberIntentCodecKotlin,
          },
        ],
        screenshots: [],
        customization: [
          "Add a `<queries>` block to your `AndroidManifest.xml` listing `nostrsigner` (and any future signer schemes). Without this Android 11+ (API 30+) returns an empty list from `PackageManager.queryIntentActivities` even when Amber is installed.",
          "Extend `KNOWN_NOSTR_SIGNERS` in `ExternalSignerWire.kt` to add future signer apps. Each scheme here must also appear in `<queries>`.",
          "Register the bridge in `Activity.onCreate` (before first `onStart`) via `bridge.register()`, and call `bridge.unregister()` in `onDestroy`. The bridge wraps `registerForActivityResult` and must be registered before the activity starts.",
          "Wire `onSignerSelected` to report user intent to Rust (`nativeSignInNip55(signerPackage)` on your kernel bridge). Rust builds the `get_public_key` + permission-batch request and dispatches it back through the kernel's signer-request channel; drain that channel (`nativeNextSignerRequest`) and hand each request JSON to `bridge.handleJson`. Route the `onResult` callback back to Rust via `nativeDeliverSignerResponse` (D7 — Kotlin decides nothing).",
          "Pass the kernel's `signer_state` projection into `NostrLoginBlock(signerState = …)` — the `is*` flags drive the inline waiting / ready / failed indicators without string-matching `state`.",
          "Testable without a real Activity: `shouldUseContentResolver` (the exact predicate `handle()` branches on) and `signerCardUi` (the exact presentation rule `SignerCard` renders from) are pure internal functions exercised directly by the JVM unit suite.",
        ],
      },
    },
  },
];
