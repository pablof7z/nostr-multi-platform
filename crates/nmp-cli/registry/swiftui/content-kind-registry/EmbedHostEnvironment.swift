import SwiftUI

/// Read-only source of resolved embed envelopes the app-level host/provider
/// receives from Rust on every snapshot frame, keyed by `primaryId`
/// (event-id hex or `kind:pubkey:d` coordinate). The map is the derived
/// `refs.event.envelopes` sidecar produced from authoritative `refs.event`
/// rows by `nmp_content::derive_ref_event_envelopes`; this conformer only
/// mirrors that sidecar into SwiftUI. `NostrContentView`'s event-ref renderer
/// reads it to feed `EmbeddedEvent`.
///
/// THIN-SHELL: the conformer only stores and looks up the already-resolved
/// envelopes — no kind dispatch, no protocol parsing (the Rust resolver did
/// that before the envelope crossed the wire).
@MainActor
public protocol EmbedEnvelopeSource {
    func envelopeForPrimaryID(_ id: String) -> EmbeddedEventEnvelope?
    func envelopeForURI(_ uri: String) -> EmbeddedEventEnvelope?
}

// MARK: - Environment keys

private struct EmbedEnvelopeSourceKey: EnvironmentKey {
    nonisolated(unsafe)
    static let defaultValue: EmbedEnvelopeSource? = nil
}

private struct EmbedEventRefResolverKey: EnvironmentKey {
    static let defaultValue: EventRefResolverProtocol? = nil
}

private struct NostrKindRegistryKey: EnvironmentKey {
    @MainActor static let defaultValue: NostrKindRegistry? = nil
}

public extension EnvironmentValues {
    /// The host that resolves embed envelopes for `nostr:` event refs.
    var embedEnvelopeSource: EmbedEnvelopeSource? {
        get { self[EmbedEnvelopeSourceKey.self] }
        set { self[EmbedEnvelopeSourceKey.self] = newValue }
    }

    /// The resolve/release adapter `EmbeddedEvent` fires on enter/exit so the kernel
    /// reference-counts the embed URI and triggers upstream fetch.
    var embedEventRefResolver: EventRefResolverProtocol? {
        get { self[EmbedEventRefResolverKey.self] }
        set { self[EmbedEventRefResolverKey.self] = newValue }
    }

    /// The kind → renderer dispatch table consulted for each resolved embed.
    var nostrKindRegistry: NostrKindRegistry? {
        get { self[NostrKindRegistryKey.self] }
        set { self[NostrKindRegistryKey.self] = newValue }
    }
}

public extension View {
    /// Bind the embed host, event-ref resolver, and kind registry so any nested
    /// `NostrContentView` renders `nostr:` event refs through the kind-dispatch
    /// registry (ADR-0034).
    func embedEnvelopeSource(
        _ source: EmbedEnvelopeSource?,
        eventRefResolver: EventRefResolverProtocol? = nil,
        registry: NostrKindRegistry? = nil
    ) -> some View {
        self
            .environment(\.embedEnvelopeSource, source)
            .environment(\.embedEventRefResolver, eventRefResolver)
            .environment(\.nostrKindRegistry, registry)
    }
}
