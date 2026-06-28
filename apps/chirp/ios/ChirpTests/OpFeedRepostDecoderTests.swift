import XCTest
import FlatBuffers
@testable import Chirp

/// Focused tests for the D0 typed repost signal (`ChirpEventCard.isRepost`) on
/// the `NOFS` OP-feed sidecar (ADR-0038 Stage T4).
///
/// `isRepost` is the field `ThreadNoteRow` reads instead of `card.kind == 6`.
/// It must be driven purely by the typed `reposted_by` presence the encoder
/// froze into the golden fixture — NOT re-derived from `kind` in the view. That
/// matters for the OP-centric feed, where a repost card can carry the
/// *original* note's kind (so `kind == 6` would be wrong). These assertions are
/// split out of `OpFeedDecoderTests` so the repost contract reads as one
/// cohesive unit and the parity test stays focused on structure.
///
/// Golden bytes + hex helpers come from the shared `OpFeedTestFixtures`.
final class OpFeedRepostDecoderTests: XCTestCase {

    /// The populated golden fixture has two root cards: a plain thread root
    /// (id 0x03, no `reposted_by`) and a repost-keyed root (id 0x09, carrying
    /// `reposted_by`). `isRepost` must be false for the first and true for the
    /// second.
    func testRepostSignalDrivenByRepostedByNotKind() throws {
        let snapshot = try XCTUnwrap(
            TypedHomeFeedDecoder.decode(bytes: OpFeedTestFixtures.data(fromHex: OpFeedTestFixtures.populatedHex)),
            "NOFS populated golden fixture must decode")
        XCTAssertEqual(snapshot.cards.count, 2)

        // Plain root: no `reposted_by` → isRepost is false (NOT re-derived from
        // kind in the view).
        let root = snapshot.cards[0]
        XCTAssertEqual(root.card.id, OpFeedTestFixtures.hex32(0x03))
        XCTAssertEqual(root.card.kind, 1)
        XCTAssertFalse(root.card.isRepost, "plain root must not be flagged repost")

        // Repost-keyed card: carries `reposted_by` → isRepost is true. This is
        // the field ThreadNoteRow reads instead of `card.kind == 6`, and it
        // stays correct for the OP-centric feed where a repost card can carry
        // the *original* note's kind (not 6).
        let repost = snapshot.cards[1]
        XCTAssertEqual(repost.card.id, OpFeedTestFixtures.hex32(0x09))
        XCTAssertEqual(repost.card.kind, 6)
        XCTAssertTrue(repost.card.isRepost, "card with reposted_by must be flagged repost")
        // GH #920: the card-level author display is the absent fallback; the
        // card carries no denormalized name/picture (the attribution table is
        // the populated display surface, and a repost card has none here).
        XCTAssertNil(repost.card.authorDisplayName)
        XCTAssertNil(repost.card.authorPictureUrl)
        XCTAssertTrue(repost.attribution.isEmpty)
    }
}
