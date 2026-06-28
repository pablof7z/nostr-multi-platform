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
struct EmbeddedEvent: View {
    var uri: String
    /// Optional resolved envelope. `nil` while the kernel fetches; the view
    /// shows a loading placeholder until the snapshot arrives.
    var envelope: EmbeddedEventEnvelope?
    var registry: NostrKindRegistry
    var claimSink: EventClaimSinkProtocol?
    var consumerId: String

    init(
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

    var body: some View {
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
            // (D8); a subdued skeleton matching the resolved quote-card
            // geometry. Never surface the raw bech32 URI to the user.
            EmbeddedEventSkeleton()
        }
    }
}

/// Redacted placeholder shown while an embedded event is being fetched.
/// Mirrors the avatar + name + two-line body shape of a resolved quote card
/// so the row doesn't reflow when the real content arrives.
private struct EmbeddedEventSkeleton: View {
    private var bar: Color { ChirpColor.secondaryFill }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Circle()
                    .fill(bar)
                    .frame(width: 22, height: 22)
                RoundedRectangle(cornerRadius: 3, style: .continuous)
                    .fill(bar)
                    .frame(width: 110, height: 10)
                Spacer(minLength: 0)
            }
            RoundedRectangle(cornerRadius: 3, style: .continuous)
                .fill(bar)
                .frame(maxWidth: .infinity)
                .frame(height: 10)
            RoundedRectangle(cornerRadius: 3, style: .continuous)
                .fill(bar)
                .frame(width: 180, height: 10)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(ChirpColor.surface.opacity(0.6))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(ChirpColor.hairline.opacity(0.5), lineWidth: 0.5)
        )
        .accessibilityLabel("Loading embedded note")
    }
}
