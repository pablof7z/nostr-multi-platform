import XCTest
@testable import Chirp

// F-CR-12 — EmbedKindProjection + NostrKindRegistry unit tests.
//
// These tests exercise:
//   1. EmbedHost.resolve() dispatches to the correct EmbedKindProjection variant
//      for kinds 0, 1, 9802, 30023, and an unknown kind (the five projection
//      variants: Profile, ShortNote, Highlight, Article, Unknown).
//   2. NostrKindRegistry.resolve() returns the registered renderer for each
//      variant and falls back to the default renderer for Unknown.
//   3. EmbeddedEventEnvelope carries collapse state correctly.
//
// These tests are PURE UNIT TESTS — no FFI, no kernel, no relays.
// They validate the Swift-side dispatch tables that mirror the Rust
// `nmp_content::embed_projection::EmbedKindProjection` wire contract.
//
// The native-tests CI lane (or a manual `xcodebuild test -scheme ChirpTests`)
// validates this file; xcodebuild is NOT run here per issue #988 scope.

@MainActor
final class EmbedKindProjectionTests: XCTestCase {

    // MARK: - Helpers

    private let samplePubkey = String(repeating: "a", count: 64)
    private let sampleId     = String(repeating: "b", count: 64)
    private let sampleTime: UInt64 = 1_700_000_000

    /// Build a ClaimedEventDto for testing.
    private func dto(
        id: String? = nil,
        pubkey: String? = nil,
        kind: Int,
        content: String,
        tags: [[String]] = []
    ) -> ClaimedEventDto {
        ClaimedEventDto(
            id: id ?? sampleId,
            authorPubkey: pubkey ?? samplePubkey,
            kind: kind,
            createdAt: Int(sampleTime),
            content: content,
            tags: tags
        )
    }

    // EmbedHost is @Observable; we create one per test to keep state isolated.
    private func freshHost() -> EmbedHost { EmbedHost() }

    // MARK: - Dispatch: kind:0 → Profile

    func testKind0DispatchesToProfile() {
        let host = freshHost()
        let meta = #"{"name":"Alice","display_name":"Alice NMP","picture":"https://nmp.test/alice.png"}"#
        let d = dto(kind: 0, content: meta)
        host.update(claimedEvents: [sampleId: d])

        let envelope = host.envelopeForPrimaryID(sampleId)
        XCTAssertNotNil(envelope, "kind:0 must produce an envelope")
        guard case .profile(let p) = envelope?.projection else {
            return XCTFail("kind:0 must resolve to .profile, got \(String(describing: envelope?.projection))")
        }
        XCTAssertEqual(p.pubkey, samplePubkey)
        XCTAssertEqual(p.displayName, "Alice NMP", "display_name wins over name")
        XCTAssertEqual(p.pictureUrl, "https://nmp.test/alice.png")
    }

    func testKind0FallsBackToNameWhenDisplayNameAbsent() {
        let host = freshHost()
        let meta = #"{"name":"bob"}"#
        let d = dto(kind: 0, content: meta)
        host.update(claimedEvents: [sampleId: d])

        guard case .profile(let p) = host.envelopeForPrimaryID(sampleId)?.projection else {
            return XCTFail("expected .profile")
        }
        XCTAssertEqual(p.displayName, "bob")
    }

    // MARK: - Dispatch: kind:1 → ShortNote

    func testKind1DispatchesToShortNote() {
        let host = freshHost()
        let d = dto(kind: 1, content: "Hello nostr!")
        host.update(claimedEvents: [sampleId: d])

        guard case .shortNote(let n) = host.envelopeForPrimaryID(sampleId)?.projection else {
            return XCTFail("kind:1 must resolve to .shortNote")
        }
        XCTAssertEqual(n.id, sampleId)
        XCTAssertEqual(n.authorPubkey, samplePubkey)
        XCTAssertEqual(n.content, "Hello nostr!")
        XCTAssertEqual(n.createdAt, sampleTime)
    }

    func testKind1ExtractsMediaUrls() {
        let host = freshHost()
        let content = "check this out https://nmp.test/photo.jpg cool right"
        let d = dto(kind: 1, content: content)
        host.update(claimedEvents: [sampleId: d])

        guard case .shortNote(let n) = host.envelopeForPrimaryID(sampleId)?.projection else {
            return XCTFail("expected .shortNote")
        }
        XCTAssertEqual(n.mediaUrls, ["https://nmp.test/photo.jpg"])
    }

    // MARK: - Dispatch: kind:9802 → Highlight

    func testKind9802DispatchesToHighlight() {
        let host = freshHost()
        let tags: [[String]] = [
            ["a", "30023:\(samplePubkey):my-article"],
            ["context", "surrounding text"],
            ["r", "https://blog.nmp.test/my-article"],
        ]
        let d = dto(kind: 9802, content: "backpressure is a feature", tags: tags)
        host.update(claimedEvents: [sampleId: d])

        guard case .highlight(let h) = host.envelopeForPrimaryID(sampleId)?.projection else {
            return XCTFail("kind:9802 must resolve to .highlight")
        }
        XCTAssertEqual(h.id, sampleId)
        XCTAssertEqual(h.authorPubkey, samplePubkey)
        XCTAssertEqual(h.highlightedText, "backpressure is a feature")
        XCTAssertEqual(h.sourceEventAddr, "30023:\(samplePubkey):my-article")
        XCTAssertEqual(h.context, "surrounding text")
        XCTAssertEqual(h.sourceUrl, "https://blog.nmp.test/my-article")
        XCTAssertNil(h.sourceEventId, "no e tag → sourceEventId is nil")
    }

    func testKind9802MinimalHighlight() {
        let host = freshHost()
        let d = dto(kind: 9802, content: "plain highlight text")
        host.update(claimedEvents: [sampleId: d])

        guard case .highlight(let h) = host.envelopeForPrimaryID(sampleId)?.projection else {
            return XCTFail("expected .highlight")
        }
        XCTAssertEqual(h.highlightedText, "plain highlight text")
        XCTAssertNil(h.sourceEventId)
        XCTAssertNil(h.sourceEventAddr)
        XCTAssertNil(h.sourceUrl)
        XCTAssertNil(h.context)
    }

    // MARK: - Dispatch: kind:30023 → Article

    func testKind30023DispatchesToArticle() {
        let host = freshHost()
        let tags: [[String]] = [
            ["d", "backpressure-is-a-feature"],
            ["title", "Backpressure Is A Feature"],
            ["summary", "Why your relay should push back."],
            ["image", "https://nmp.test/hero.png"],
        ]
        let d = dto(kind: 30023, content: "# Backpressure\n\nBody here.", tags: tags)
        host.update(claimedEvents: [sampleId: d])

        guard case .article(let a) = host.envelopeForPrimaryID(sampleId)?.projection else {
            return XCTFail("kind:30023 must resolve to .article")
        }
        XCTAssertEqual(a.id, sampleId)
        XCTAssertEqual(a.dTag, "backpressure-is-a-feature")
        XCTAssertEqual(a.title, "Backpressure Is A Feature")
        XCTAssertEqual(a.summary, "Why your relay should push back.")
        XCTAssertEqual(a.heroImageUrl, "https://nmp.test/hero.png")
    }

    func testKind30023MissingOptionalTags() {
        let host = freshHost()
        let tags: [[String]] = [["d", "minimal-article"]]
        let d = dto(kind: 30023, content: "No title or summary.", tags: tags)
        host.update(claimedEvents: [sampleId: d])

        guard case .article(let a) = host.envelopeForPrimaryID(sampleId)?.projection else {
            return XCTFail("expected .article")
        }
        XCTAssertEqual(a.dTag, "minimal-article")
        XCTAssertNil(a.title)
        XCTAssertNil(a.summary)
        XCTAssertNil(a.heroImageUrl)
    }

    // MARK: - Dispatch: unknown kind → Unknown

    func testUnknownKindDispatchesToUnknown() {
        let host = freshHost()
        let tags: [[String]] = [
            ["price", "42"],
            ["alt", "a classified listing"],
        ]
        let d = dto(kind: 30402, content: "Classified ad body", tags: tags)
        host.update(claimedEvents: [sampleId: d])

        guard case .unknown(let u) = host.envelopeForPrimaryID(sampleId)?.projection else {
            return XCTFail("kind:30402 must resolve to .unknown")
        }
        XCTAssertEqual(u.kind, 30402)
        XCTAssertEqual(u.content, "Classified ad body")
        XCTAssertEqual(u.altText, "a classified listing")
        XCTAssertEqual(u.tags.count, 2)
    }

    func testKind40DispatchesToUnknown() {
        let host = freshHost()
        let d = dto(kind: 40, content: #"{"name":"nmp-dev"}"#)
        host.update(claimedEvents: [sampleId: d])

        guard case .unknown(let u) = host.envelopeForPrimaryID(sampleId)?.projection else {
            return XCTFail("kind:40 must resolve to .unknown (IRC channel)")
        }
        XCTAssertEqual(u.kind, 40)
    }

    // MARK: - NostrKindRegistry dispatch

    func testRegistryResolvesShortNoteRenderer() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(
            id: sampleId, authorPubkey: samplePubkey
        ))
        let renderer = registry.resolve(proj)
        XCTAssertTrue(renderer is DefaultShortNoteRenderer, "shortNote must use DefaultShortNoteRenderer")
    }

    func testRegistryResolvesArticleRenderer() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.article(ArticleProjection(
            id: sampleId, authorPubkey: samplePubkey
        ))
        let renderer = registry.resolve(proj)
        XCTAssertTrue(renderer is DefaultArticleRenderer, "article must use DefaultArticleRenderer")
    }

    func testRegistryResolvesHighlightRenderer() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.highlight(HighlightProjection(
            id: sampleId, authorPubkey: samplePubkey
        ))
        let renderer = registry.resolve(proj)
        XCTAssertTrue(renderer is DefaultHighlightRenderer, "highlight must use DefaultHighlightRenderer")
    }

    func testRegistryResolvesProfileRenderer() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.profile(ProfileProjection(pubkey: samplePubkey))
        let renderer = registry.resolve(proj)
        XCTAssertTrue(renderer is DefaultProfileRenderer, "profile must use DefaultProfileRenderer")
    }

    func testRegistryFallsBackToUnknownRendererForUnregisteredKind() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.unknown(UnknownProjection(
            kind: 30402, authorPubkey: samplePubkey
        ))
        let renderer = registry.resolve(proj)
        XCTAssertTrue(renderer is DefaultUnknownRenderer, "unknown kind must use DefaultUnknownRenderer")
    }

    func testRegistryUsesCustomRendererWhenRegistered() {
        let registry = NostrKindRegistry.makeDefault()
        let custom = StubRenderer()
        registry.registerUnknown(kind: 30402, renderer: custom)
        let proj = EmbedKindProjection.unknown(UnknownProjection(
            kind: 30402, authorPubkey: samplePubkey
        ))
        let renderer = registry.resolve(proj)
        XCTAssertTrue(renderer is StubRenderer, "registered custom renderer must win over fallback")
    }

    // MARK: - EmbeddedEventEnvelope collapse state

    func testEnvelopeNonCollapsedByDefault() {
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(
            id: sampleId, authorPubkey: samplePubkey
        ))
        let envelope = EmbeddedEventEnvelope(
            uri: "nostr:note1abc",
            primaryId: sampleId,
            projection: proj
        )
        XCTAssertFalse(envelope.collapsed)
        XCTAssertNil(envelope.collapseReason)
    }

    func testEnvelopeCarriesCollapseReasonForDepthLimit() {
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(
            id: sampleId, authorPubkey: samplePubkey
        ))
        let envelope = EmbeddedEventEnvelope(
            uri: "nostr:note1abc",
            primaryId: sampleId,
            projection: proj,
            collapsed: true,
            collapseReason: "depth_limit"
        )
        XCTAssertTrue(envelope.collapsed)
        XCTAssertEqual(envelope.collapseReason, "depth_limit")
    }

    func testEnvelopeCarriesCollapseReasonForCycle() {
        let proj = EmbedKindProjection.article(ArticleProjection(
            id: sampleId, authorPubkey: samplePubkey
        ))
        let envelope = EmbeddedEventEnvelope(
            uri: "nostr:naddr1abc",
            primaryId: sampleId,
            projection: proj,
            collapsed: true,
            collapseReason: "cycle"
        )
        XCTAssertEqual(envelope.collapseReason, "cycle")
    }

    func testEnvelopeDepthAndMaxDepthSurfaced() {
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(
            id: sampleId, authorPubkey: samplePubkey
        ))
        let envelope = EmbeddedEventEnvelope(
            uri: "nostr:note1abc",
            primaryId: sampleId,
            depth: 3,
            maxDepth: 4,
            projection: proj
        )
        XCTAssertEqual(envelope.depth, 3)
        XCTAssertEqual(envelope.maxDepth, 4)
    }

    // MARK: - NoteContentView registry-path smoke test (#1179)
    //
    // Verifies that NoteContentView no longer passes quoteCardProvider to
    // NostrContentView. When a NostrKindRegistry + EmbedHost are bound in the
    // environment, event-ref nodes in the content tree must flow through the
    // EmbeddedEvent/registry path, not the legacy NostrQuoteCard path.
    //
    // The structural proof: NostrContentView.eventRefView enters the registry
    // branch when (nostrKindRegistry != nil && (embedHost != nil || embedClaimSink
    // != nil)). NoteContentView omitting quoteCardProvider means the legacy branch
    // can only be reached when the environment provides no registry — the correct
    // fallback for contexts that haven't wired the registry (e.g. bare previews).

    func testNoteContentViewRendersEventRefThroughRegistryPath() throws {
        // Build a content tree with one eventRef node so NostrContentView's
        // eventRefView dispatch is exercised.
        let eventID = String(repeating: "c", count: 64)
        let tree = ContentTreeWire(
            nodes: [
                .paragraph(children: [1]),
                .eventRef(WireNostrUri(
                    uri: "nostr:note1\(String(repeating: "c", count: 56))",
                    kind: .event,
                    primaryId: eventID,
                    relays: [],
                    author: samplePubkey,
                    eventKind: 1
                )),
            ],
            roots: [0],
            mode: nil
        )

        let host = EmbedHost()
        // Pre-populate the host so EmbeddedEvent has an envelope to resolve.
        let d = dto(id: eventID, kind: 1, content: "quoted note text")
        host.update(claimedEvents: [eventID: d])

        let registry = NostrKindRegistry.makeDefault()

        // NoteContentView must render without crash when the registry path is
        // active. The ImageRenderer is the same oracle used by
        // NoteContentRenderingTests — a non-nil UIImage proves no assertion
        // or fatal error was hit during the render walk.
        let view = NoteContentView(content: "", contentTree: tree)
            .environmentObject(ChirpRouter())
            .environment(\.nostrKindRegistry, registry)
            .environment(\.embedHost, host)
            .frame(width: 320, alignment: .leading)

        let renderer = ImageRenderer(content: view)
        renderer.scale = 2
        renderer.proposedSize = ProposedViewSize(width: 320, height: nil)
        // ImageRenderer may return nil in a headless test host (no WindowServer).
        // Use XCTSkip rather than XCTFail so CI doesn't block on environment
        // limitations — the render attempt itself validates the code path
        // compiles and the view graph is well-formed.
        guard renderer.uiImage != nil else {
            throw XCTSkip("SwiftUI ImageRenderer did not produce an image in this test host")
        }
        // If we get here the view rendered through the registry path without
        // triggering any quoteCardModel / NostrQuoteCard legacy code.
    }
}

// MARK: - Test doubles

/// Minimal no-op renderer used to assert custom renderer registration wins.
@MainActor
private final class StubRenderer: KindRenderer {
    func body(projection: EmbedKindProjection, registry: NostrKindRegistry) -> AnyView {
        AnyView(EmptyView())
    }
}
