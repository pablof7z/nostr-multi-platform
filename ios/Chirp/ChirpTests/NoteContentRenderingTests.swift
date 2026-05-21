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
                    kind: "npub",
                    primaryId: pubkey,
                    relays: [],
                    author: nil,
                    eventKind: nil
                )),
                .text(" here is "),
                .eventRef(WireNostrUri(
                    uri: "nostr:nevent1example",
                    kind: "nevent",
                    primaryId: eventID,
                    relays: [],
                    author: pubkey,
                    eventKind: 1
                )),
            ],
            roots: [0],
            mode: nil
        )

        let groups = noteContentGroups(tree)
        XCTAssertEqual(groups.count, 2)
        XCTAssertEqual(groups.first, .inline([1, 2, 3]))
        if case .eventRef(let uri) = groups.last {
            XCTAssertEqual(uri.primaryId, eventID)
        } else {
            XCTFail("inline nevent reference was not promoted to an embedded event group")
        }

        let context = NoteRenderContext(
            mentionProfiles: [
                pubkey: MentionProfile(
                    display: "pablof7z",
                    pictureUrl: nil,
                    initials: "PF",
                    colorHex: "#4B7BEC"
                ),
            ],
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
                    )
                ),
            ],
            timelineItems: [:],
            embedDepth: 0
        )
        XCTAssertEqual(context.mentionLabel(for: pubkey), "pablof7z")

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
