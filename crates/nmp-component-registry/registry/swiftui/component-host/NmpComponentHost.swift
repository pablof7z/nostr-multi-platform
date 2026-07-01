import SwiftUI

/// App-root binding for NMP registry components.
///
/// The host installs the existing profile and embed environment values in one
/// place. Apps still own the concrete bridge objects: profile data comes from
/// `refs.profile`, event envelopes come from the Rust-derived
/// `refs.event.envelopes` sidecar, and components only render plus manage
/// visible resolve/release lifecycle.
public struct NmpComponentHost<Content: View>: View {
    private let profileHost: NostrProfileHost?
    private let embedSource: EmbedEnvelopeSource?
    private let eventRefResolver: EventRefResolverProtocol?
    private let kindRegistry: NostrKindRegistry?
    private let content: Content

    public init(
        profileHost: NostrProfileHost?,
        embedSource: EmbedEnvelopeSource?,
        eventRefResolver: EventRefResolverProtocol? = nil,
        kindRegistry: NostrKindRegistry? = nil,
        @ViewBuilder content: () -> Content
    ) {
        self.profileHost = profileHost
        self.embedSource = embedSource
        self.eventRefResolver = eventRefResolver
        self.kindRegistry = kindRegistry
        self.content = content()
    }

    public var body: some View {
        content.nmpComponentHost(
            profileHost: profileHost,
            embedSource: embedSource,
            eventRefResolver: eventRefResolver,
            kindRegistry: kindRegistry
        )
    }
}

public extension View {
    /// Install the standard NMP registry component host at an app or screen root.
    ///
    /// This is DX glue over the lower-level environment keys. It does not own a
    /// kernel handle, parse Nostr events, or maintain profile/embed caches.
    func nmpComponentHost(
        profileHost: NostrProfileHost?,
        embedSource: EmbedEnvelopeSource?,
        eventRefResolver: EventRefResolverProtocol? = nil,
        kindRegistry: NostrKindRegistry? = nil
    ) -> some View {
        self
            .environment(\.nostrProfileHost, profileHost)
            .embedEnvelopeSource(
                embedSource,
                eventRefResolver: eventRefResolver,
                registry: kindRegistry
            )
    }
}
