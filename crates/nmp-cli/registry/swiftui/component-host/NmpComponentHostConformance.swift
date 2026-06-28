import SwiftUI

/// In-memory rows for component-host conformance tests and previews.
///
/// These values model the host contract without a live kernel:
/// - `refs.profile` is the profile row source.
/// - `refs.event` is the authoritative event-ref row source.
/// - `refs.event.envelopes` is derived render data for embeds.
///
/// App tests can mount `NmpComponentHostConformanceHarness` around registry
/// components and assert that the component reads these host values instead of
/// reaching for a kernel handle or ABI/runtime object.
public enum NmpComponentHostConformanceFixture {
    public static let refsProfileKey = "refs.profile"
    public static let refsEventKey = "refs.event"
    public static let refsEventEnvelopesKey = "refs.event.envelopes"

    public static let pubkey =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    public static let primaryEventId =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    public static let eventUri = "nostr:nevent1componenthost"

    public static let profile = ProfileWire(
        pubkey: pubkey,
        displayName: "Conformance Alice",
        about: "Profile row supplied by refs.profile.",
        pictureUrl: "https://example.invalid/alice.png",
        nip05: "alice@example.invalid",
        npub: "npub1componenthostfixture",
        npubShort: "npub1component...fixture"
    )

    public static let eventEnvelope = EmbeddedEventEnvelope(
        uri: eventUri,
        primaryId: primaryEventId,
        projection: .shortNote(ShortNoteProjection(
            id: primaryEventId,
            authorPubkey: pubkey,
            authorDisplayName: nil,
            authorPictureUrl: nil,
            createdAt: 1_700_000_000,
            content: "Event render data supplied by refs.event.envelopes.",
            mediaUrls: []
        ))
    )

    public static let envelopesByPrimaryID: [String: EmbeddedEventEnvelope] = [
        primaryEventId: eventEnvelope,
        eventUri: eventEnvelope,
    ]

    public static let expectedKeys = [
        refsProfileKey,
        refsEventKey,
        refsEventEnvelopesKey,
    ]
}

@MainActor
public final class FixtureNostrProfileHost: NostrProfileHost {
    public private(set) var resolved: [(pubkey: String, consumerID: String)] = []
    public private(set) var released: [(pubkey: String, consumerID: String)] = []

    public init() {}

    public func profile(forPubkey pubkey: String) -> ProfileWire? {
        pubkey == NmpComponentHostConformanceFixture.pubkey
            ? NmpComponentHostConformanceFixture.profile
            : nil
    }

    public func resolveProfileRef(pubkey: String, consumerID: String) {
        resolved.append((pubkey, consumerID))
    }

    public func releaseProfileRef(pubkey: String, consumerID: String) {
        released.append((pubkey, consumerID))
    }
}

@MainActor
public final class FixtureEmbedEnvelopeSource: EmbedEnvelopeSource {
    public init() {}

    public func envelopeForPrimaryID(_ id: String) -> EmbeddedEventEnvelope? {
        NmpComponentHostConformanceFixture.envelopesByPrimaryID[id]
    }

    public func envelopeForURI(_ uri: String) -> EmbeddedEventEnvelope? {
        NmpComponentHostConformanceFixture.envelopesByPrimaryID[uri]
    }
}

public struct FixtureEventRefResolver: EventRefResolverProtocol {
    public init() {}

    public func resolveEventRef(uri: String, consumerId: String) {}
    public func releaseEventRef(uri: String, consumerId: String) {}
}

@MainActor
public struct NmpComponentHostConformanceHarness<Content: View>: View {
    private let profileHost: FixtureNostrProfileHost
    private let embedSource: FixtureEmbedEnvelopeSource
    private let resolver: FixtureEventRefResolver
    private let content: Content

    public init(
        profileHost: FixtureNostrProfileHost = FixtureNostrProfileHost(),
        embedSource: FixtureEmbedEnvelopeSource = FixtureEmbedEnvelopeSource(),
        resolver: FixtureEventRefResolver = FixtureEventRefResolver(),
        @ViewBuilder content: () -> Content
    ) {
        self.profileHost = profileHost
        self.embedSource = embedSource
        self.resolver = resolver
        self.content = content()
    }

    public var body: some View {
        NmpComponentHost(
            profileHost: profileHost,
            embedSource: embedSource,
            eventRefResolver: resolver,
            kindRegistry: NostrKindRegistry.makeDefault()
        ) {
            content
        }
    }
}
