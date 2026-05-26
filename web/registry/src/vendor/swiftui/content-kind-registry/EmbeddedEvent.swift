import SwiftUI

/// SwiftUI view that renders one embedded Nostr event by dispatching through
/// `NostrKindRegistry`. The view itself is purely declarative — it owns the
/// claim/release lifecycle of the embed URI (via `task(id:)` / `onDisappear`),
/// reads the resolved `EmbeddedEventEnvelope` from the host bound by the
/// caller, and lets the registry pick the right renderer.
///
/// Mirrors the TUI's `EmbeddedEvent` widget (`crates/nmp-cli/registry/tui/
/// content-kind-registry/embedded_event.rs`).
///
/// Lifecycle (D8 — no polling; aligned with task instructions to never claim
/// inside `var body`):
///   • `.task(id: uri)` calls the sink's `claim` exactly once per URI.
///   • `.onDisappear` releases. SwiftUI's identity-stable `id:` parameter
///     guarantees one matched claim/release pair per embedded slot.
public struct EmbeddedEvent: View {
    public var uri: String
    /// Optional resolved envelope. `nil` while the kernel fetches; the view
    /// shows a loading placeholder until the snapshot arrives.
    public var envelope: EmbeddedEventEnvelope?
    public var registry: NostrKindRegistry
    public var claimSink: EventClaimSinkProtocol?
    public var consumerId: String

    public init(
        uri: String,
        envelope: EmbeddedEventEnvelope?,
        registry: NostrKindRegistry,
        claimSink: EventClaimSinkProtocol? = nil,
        consumerId: String = "nmp-gallery-ios.embed"
    ) {
        self.uri = uri
        self.envelope = envelope
        self.registry = registry
        self.claimSink = claimSink
        self.consumerId = consumerId
    }

    public var body: some View {
        EmbedChromeContainer(
            depth: envelope?.depth ?? 0,
            collapsed: envelope?.collapsed ?? false
        ) {
            content
        }
        .task(id: uri) {
            claimSink?.claim(uri: uri, consumerId: consumerId)
        }
        .onDisappear {
            claimSink?.release(uri: uri, consumerId: consumerId)
        }
    }

    @ViewBuilder
    private var content: some View {
        if let envelope {
            if envelope.collapsed {
                let reason = envelope.collapseReason ?? "collapsed"
                Text("embedded event \(reason)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                registry.resolve(envelope.projection)
                    .body(projection: envelope.projection, registry: registry)
            }
        } else {
            // Loading state — the kernel is fetching the event. No spinner
            // (D8); show a subdued placeholder identical in shape to the
            // resolved render.
            VStack(alignment: .leading, spacing: 4) {
                Text("loading embedded event…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(shortUri(uri))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
    }

    private func shortUri(_ value: String) -> String {
        guard value.count > 24 else { return value }
        return "\(value.prefix(14))…\(value.suffix(8))"
    }
}

/// Protocol mirror of `nmp_content::EventClaimSink`. The renderer fires
/// `claim(uri:consumerId:)` when an embed enters the view tree and the
/// matching `release(...)` when it leaves. Implementations forward to the
/// kernel's `nmp_app_claim_event` / `nmp_app_release_event` FFI symbols.
///
/// `Sendable` because the gallery's concrete impl is the kernel actor's
/// command channel — every method is fire-and-forget across threads.
public protocol EventClaimSinkProtocol: Sendable {
    func claim(uri: String, consumerId: String)
    func release(uri: String, consumerId: String)
}
