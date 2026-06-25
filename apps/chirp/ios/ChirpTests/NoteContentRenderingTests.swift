import SwiftUI
import XCTest
@testable import Chirp

@MainActor
final class NoteContentRenderingTests: XCTestCase {
    func testNostrMentionAndInlineEventReferenceBecomeRichEntities() throws {
        let pubkey = String(repeating: "a", count: 64)
        let eventID = String(repeating: "b", count: 64)
        let tree = ContentTreeWire(
            nodes: [
                .paragraph(children: [1, 2, 3, 4]),
                .text("hey "),
                .mention(WireNostrUri(
                    uri: "nostr:npub1example",
                    kind: .profile,
                    primaryId: pubkey,
                    relays: [],
                    author: nil,
                    eventKind: nil
                )),
                .text(" here is "),
                .eventRef(WireNostrUri(
                    uri: "nostr:nevent1example",
                    kind: .event,
                    primaryId: eventID,
                    relays: [],
                    author: pubkey,
                    eventKind: 1
                )),
            ],
            roots: [0],
            mode: nil
        )

        // nostrContentGroups promotes inline eventRef children out of the
        // paragraph into a top-level .eventRef group. The paragraph becomes
        // .inline(level: .paragraph, children: [1, 2, 3, sentinel]).
        let groups = nostrContentGroups(tree)
        XCTAssertEqual(groups.count, 2)
        if case .inline(let level, let children) = groups.first {
            XCTAssertEqual(level, .paragraph)
            // children: text(1), mention(2), text(3), plus trailing sentinel
            XCTAssertEqual(children.prefix(3), [1, 2, 3])
        } else {
            XCTFail("expected .inline as first group, got \(String(describing: groups.first))")
        }
        if case .eventRef(let uri) = groups.last {
            XCTAssertEqual(uri.primaryId, eventID)
        } else {
            XCTFail("inline nevent reference was not promoted to an embedded event group")
        }

        // ADR-0063 Lane E (#1671): mention labels are no longer carried in the
        // render context (the whole-map `mentionProfiles` dictionary is gone).
        // `NoteContentView` reads the mention display name from the per-key
        // `keyedRefCache` at render time, falling back to the shortened pubkey
        // when unresolved. With no `nostrProfileHost` bound in this render host,
        // the mention renders its short-pubkey fallback — the tree still
        // produces a non-trivial image, which is what this test guards.
        let context = NoteRenderContext(
            eventCards: [
                eventID: ChirpEventCard(
                    id: eventID,
                    authorPubkey: pubkey,
                    kind: 1,
                    createdAt: 1_762_000_000,
                    content: "embedded note body",
                    contentTree: ContentTreeWire(
                        nodes: [.paragraph(children: [1]), .text("embedded note body")],
                        roots: [0],
                        mode: nil
                    ),
                    relationCounts: nil,
                    authorDisplayName: "pablof7z",
                    authorPictureUrl: "identicon:\(pubkey.prefix(16))",
                    contentPreview: "embedded note body",
                    relayProvenance: [],
                    isRepost: false
                ),
            ]
        )

        let image = try renderImage(
            NoteContentView(content: "", contentTree: tree, renderContext: context)
                .environmentObject(ChirpRouter())
                .frame(width: 320, alignment: .leading)
        )
        XCTAssertGreaterThan(image.size.width, 0)
        XCTAssertGreaterThan(image.size.height, 0)
        XCTAssertGreaterThan(image.pngData()?.count ?? 0, 2_000)
    }

    private func renderImage<V: View>(_ view: V) throws -> UIImage {
        let renderer = ImageRenderer(content: view)
        renderer.scale = 2
        renderer.proposedSize = ProposedViewSize(width: 320, height: nil)
        guard let image = renderer.uiImage else {
            throw XCTSkip("SwiftUI ImageRenderer did not produce an image in this test host")
        }
        return image
    }
}
