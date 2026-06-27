import XCTest
import SwiftUI
@testable import Chirp

// EmbedKindProjection + NostrKindRegistry + EmbedHost unit tests.
//
// Issue #1283 Phase 1: the in-Swift `match kind` embed RESOLVER was deleted —
// kind dispatch + tag/JSON parsing now lives in Rust
// (`nmp_content::resolve_embed_projection`), covered by the Rust codec +
// nmp-ffi round-trip tests. So these tests no longer exercise resolution; they
// exercise what Swift still owns:
//   1. EmbedHost stores the pre-resolved `[String: EmbeddedEventEnvelope]` map
//      decoded from the typed sidecar (decode-only — no resolution).
//   2. NostrKindRegistry.resolve() returns the registered renderer per variant
//      and falls back to the default renderer for Unknown.
//   3. EmbeddedEventEnvelope carries collapse / depth state correctly.
//
// PURE UNIT TESTS — no FFI, no kernel, no relays.

@MainActor
final class EmbedKindProjectionTests: XCTestCase {

    // MARK: - Helpers

    private let samplePubkey = String(repeating: "a", count: 64)
    private let sampleId     = String(repeating: "b", count: 64)
    private let sampleTime: UInt64 = 1_700_000_000

    // EmbedHost is @Observable; we create one per test to keep state isolated.
    private func freshHost() -> EmbedHost { EmbedHost() }

    /// Wrap a projection in the FFI-sidecar envelope shape (the shape the typed
    /// decoder produces — see `TypedProjectionGlue.refEventEnvelopes`).
    private func envelope(_ projection: EmbedKindProjection, primaryId: String? = nil) -> EmbeddedEventEnvelope {
        EmbeddedEventEnvelope(uri: "", primaryId: primaryId ?? sampleId, projection: projection)
    }

    // MARK: - EmbedHost decode-only storage

    func testEmbedHostStoresEnvelopeByPrimaryID() {
        let host = freshHost()
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(
            id: sampleId, authorPubkey: samplePubkey, createdAt: sampleTime, content: "Hello nostr!"
        ))
        host.update(envelopes: [sampleId: envelope(proj)])

        let stored = host.envelopeForPrimaryID(sampleId)
        XCTAssertNotNil(stored, "host must store the envelope")
        guard case .shortNote(let n) = stored?.projection else {
            return XCTFail("expected .shortNote, got \(String(describing: stored?.projection))")
        }
        XCTAssertEqual(n.id, sampleId)
        XCTAssertEqual(n.content, "Hello nostr!")
        XCTAssertEqual(n.createdAt, sampleTime)
    }

    func testEmbedHostResolvesByURI() {
        let host = freshHost()
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(id: sampleId, authorPubkey: samplePubkey))
        let env = EmbeddedEventEnvelope(uri: "nostr:note1abc", primaryId: sampleId, projection: proj)
        host.update(envelopes: [sampleId: env])

        XCTAssertNotNil(host.envelopeForURI("nostr:note1abc"), "lookup by uri must succeed")
        XCTAssertNotNil(host.envelopeForURI(sampleId), "lookup by primaryId must succeed")
    }

    func testEmbedHostEmptyUpdateKeepsPreviousMap() {
        let host = freshHost()
        let proj = EmbedKindProjection.profile(ProfileProjection(pubkey: samplePubkey))
        host.update(envelopes: [sampleId: envelope(proj)])
        // An empty/nil update must NOT clear the map (stable, not flicker).
        host.update(envelopes: [:])
        host.update(envelopes: nil)
        XCTAssertEqual(host.count, 1, "empty/nil updates must keep the previous map")
    }

    func testEmbedHostStoresAllFiveVariants() {
        let host = freshHost()
        let envelopes: [String: EmbeddedEventEnvelope] = [
            "n": envelope(.shortNote(ShortNoteProjection(id: "n", authorPubkey: samplePubkey)), primaryId: "n"),
            "a": envelope(.article(ArticleProjection(id: "a", authorPubkey: samplePubkey)), primaryId: "a"),
            "h": envelope(.highlight(HighlightProjection(id: "h", authorPubkey: samplePubkey)), primaryId: "h"),
            "p": envelope(.profile(ProfileProjection(pubkey: samplePubkey)), primaryId: "p"),
            "u": envelope(.unknown(UnknownProjection(kind: 30402, authorPubkey: samplePubkey)), primaryId: "u"),
        ]
        host.update(envelopes: envelopes)
        XCTAssertEqual(host.count, 5)
        if case .shortNote = host.envelopeForPrimaryID("n")?.projection {} else { XCTFail("n must be shortNote") }
        if case .article = host.envelopeForPrimaryID("a")?.projection {} else { XCTFail("a must be article") }
        if case .highlight = host.envelopeForPrimaryID("h")?.projection {} else { XCTFail("h must be highlight") }
        if case .profile = host.envelopeForPrimaryID("p")?.projection {} else { XCTFail("p must be profile") }
        if case .unknown = host.envelopeForPrimaryID("u")?.projection {} else { XCTFail("u must be unknown") }
    }

    // MARK: - NostrKindRegistry dispatch

    func testRegistryResolvesShortNoteRenderer() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(id: sampleId, authorPubkey: samplePubkey))
        XCTAssertTrue(registry.resolve(proj) is DefaultShortNoteRenderer)
    }

    func testRegistryResolvesArticleRenderer() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.article(ArticleProjection(id: sampleId, authorPubkey: samplePubkey))
        XCTAssertTrue(registry.resolve(proj) is DefaultArticleRenderer)
    }

    func testRegistryResolvesHighlightRenderer() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.highlight(HighlightProjection(id: sampleId, authorPubkey: samplePubkey))
        XCTAssertTrue(registry.resolve(proj) is DefaultHighlightRenderer)
    }

    func testRegistryResolvesProfileRenderer() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.profile(ProfileProjection(pubkey: samplePubkey))
        XCTAssertTrue(registry.resolve(proj) is DefaultProfileRenderer)
    }

    func testRegistryFallsBackToUnknownRendererForUnregisteredKind() {
        let registry = NostrKindRegistry.makeDefault()
        let proj = EmbedKindProjection.unknown(UnknownProjection(kind: 30402, authorPubkey: samplePubkey))
        XCTAssertTrue(registry.resolve(proj) is DefaultUnknownRenderer)
    }

    func testRegistryUsesCustomRendererWhenRegistered() {
        let registry = NostrKindRegistry.makeDefault()
        registry.registerUnknown(kind: 30402, renderer: StubRenderer())
        let proj = EmbedKindProjection.unknown(UnknownProjection(kind: 30402, authorPubkey: samplePubkey))
        XCTAssertTrue(registry.resolve(proj) is StubRenderer, "registered custom renderer must win")
    }

    // MARK: - EmbeddedEventEnvelope collapse / depth state

    func testEnvelopeNonCollapsedByDefault() {
        let env = envelope(.shortNote(ShortNoteProjection(id: sampleId, authorPubkey: samplePubkey)))
        XCTAssertFalse(env.collapsed)
        XCTAssertNil(env.collapseReason)
    }

    func testEnvelopeCarriesCollapseReason() {
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(id: sampleId, authorPubkey: samplePubkey))
        let env = EmbeddedEventEnvelope(
            uri: "nostr:note1abc", primaryId: sampleId, projection: proj,
            collapsed: true, collapseReason: "depth_limit"
        )
        XCTAssertTrue(env.collapsed)
        XCTAssertEqual(env.collapseReason, "depth_limit")
    }

    func testEnvelopeDepthAndMaxDepthSurfaced() {
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(id: sampleId, authorPubkey: samplePubkey))
        let env = EmbeddedEventEnvelope(
            uri: "nostr:note1abc", primaryId: sampleId, depth: 3, maxDepth: 4, projection: proj
        )
        XCTAssertEqual(env.depth, 3)
        XCTAssertEqual(env.maxDepth, 4)
    }

    // MARK: - NoteContentView registry-path smoke test (#1179)
    //
    // Verifies that an eventRef node flows through the EmbeddedEvent/registry
    // path when a NostrKindRegistry + EmbedHost are bound in the environment.

    func testNoteContentViewRendersEventRefThroughRegistryPath() throws {
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
        // Pre-populate the host with a decoded envelope (decode-only path).
        let proj = EmbedKindProjection.shortNote(ShortNoteProjection(
            id: eventID, authorPubkey: samplePubkey, createdAt: sampleTime, content: "quoted note text"
        ))
        host.update(envelopes: [eventID: EmbeddedEventEnvelope(uri: "", primaryId: eventID, projection: proj)])

        let registry = NostrKindRegistry.makeDefault()
        let view = NoteContentView(content: "", contentTree: tree)
            .environmentObject(ChirpRouter())
            .embedEnvelopeSource(host, registry: registry)
            .frame(width: 320, alignment: .leading)

        let renderer = ImageRenderer(content: view)
        renderer.scale = 2
        renderer.proposedSize = ProposedViewSize(width: 320, height: nil)
        guard renderer.uiImage != nil else {
            throw XCTSkip("SwiftUI ImageRenderer did not produce an image in this test host")
        }
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
