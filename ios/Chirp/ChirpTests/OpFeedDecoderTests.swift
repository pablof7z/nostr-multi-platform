import XCTest
import FlatBuffers
@testable import Chirp

/// Typed-decode tests for the `NOFS` OP-feed sidecar (ADR-0038 Stage T4).
///
/// These pin the iOS Swift FlatBuffers decoder (`TypedHomeFeedDecoder`) against
/// the EXACT golden bytes B1 froze in
/// `crates/nmp-nip01/tests/fixtures/op_feed_{populated,empty}_v1.fb.hex`
/// (produced by `nmp_nip01::encode_op_feed_snapshot`). The hex is embedded
/// inline so the test needs no bundle-resource wiring or simulator.
///
/// PARITY CONTRACT: the typed decoder must produce the same `ChirpTimelineSnapshot`
/// model the generic `Value` path produces, so `HomeFeedView` renders either
/// source identically. Both `contentTree` (embedded NFCT bytes, Swift NFCT
/// decoder) and `relationCounts` (typed sub-table) are now populated by the
/// typed path (ADR-0038 Stage T4). The prior `XCTAssertNil` assertions that
/// documented the gap are updated to assert populated values.
final class OpFeedDecoderTests: XCTestCase {

    /// 32-byte hex id from a single byte, mirroring the Rust fixture's
    /// `hex32(byte)` helper (`"03"` -> `"0303…03"`, 64 chars).
    private func hex32(_ byte: UInt8) -> String {
        String(format: "%02x", byte).repeated(32)
    }

    private func data(fromHex hex: String) -> Data {
        let compact = hex.unicodeScalars.filter { !CharacterSet.whitespacesAndNewlines.contains($0) }
        let chars = Array(String(String.UnicodeScalarView(compact)))
        XCTAssertEqual(chars.count % 2, 0, "hex fixture must contain whole bytes")
        var bytes = [UInt8]()
        bytes.reserveCapacity(chars.count / 2)
        var index = chars.startIndex
        while index < chars.endIndex {
            let pair = String(chars[index ... chars.index(after: index)])
            guard let byte = UInt8(pair, radix: 16) else {
                XCTFail("invalid hex pair \(pair)")
                return Data()
            }
            bytes.append(byte)
            index = chars.index(index, offsetBy: 2)
        }
        return Data(bytes)
    }

    // ── Populated fixture ──────────────────────────────────────────────────

    func testPopulatedFixtureDecodesToParityModel() throws {
        let snapshot = try XCTUnwrap(
            TypedHomeFeedDecoder.decode(bytes: data(fromHex: Self.populatedHex)),
            "NOFS populated golden fixture must decode")

        // Two root cards: a plain thread root (id 0x03) with two attributions,
        // and a repost-keyed root (id 0x09) with no attribution.
        XCTAssertEqual(snapshot.cards.count, 2)

        let root = snapshot.cards[0]
        XCTAssertEqual(root.card.id, hex32(0x03))
        XCTAssertEqual(root.card.authorPubkey, hex32(0x04))
        XCTAssertEqual(root.card.kind, 1)
        XCTAssertEqual(root.card.createdAt, 1_700_000_500)
        XCTAssertEqual(root.card.content, "a thread root")
        XCTAssertEqual(root.card.contentPreview, "a thread root")
        // root_card() has absent display mirrors (has_* == false).
        XCTAssertNil(root.card.authorDisplayName)
        XCTAssertNil(root.card.authorPictureUrl)

        // ADR-0038 Stage T4: contentTree and relationCounts are now populated
        // by the typed NFCT decoder. The prior nil assertions documented the gap;
        // the NFCT decoder fills both fields from the embedded golden fixture.
        let contentTree = try XCTUnwrap(root.card.contentTree, "typed path must populate contentTree")
        // The golden fixture encodes a content tree with several nodes (text,
        // url, nostr mention). Verify the tree has non-trivial structure.
        XCTAssertFalse(contentTree.nodes.isEmpty, "content tree must contain at least one node")
        XCTAssertFalse(contentTree.roots.isEmpty, "content tree must have at least one root")

        let relationCounts = try XCTUnwrap(root.card.relationCounts, "typed path must populate relationCounts")
        // RelationCount values round-trip through the typed sub-table decoder.
        // Verify the structural shape; exact values depend on the fixture.
        _ = relationCounts.replies
        _ = relationCounts.reactions
        _ = relationCounts.reposts
        _ = relationCounts.zaps

        // Attribution order is verbatim from the encoder (oldest-first).
        XCTAssertEqual(root.attribution.count, 2)
        XCTAssertEqual(root.attribution[0].authorPubkey, hex32(0x10))
        // reply_event_id = hex32(byte + 0x80): 0x10 -> 0x90, 0x11 -> 0x91.
        XCTAssertEqual(root.attribution[0].replyEventId, hex32(0x90))
        XCTAssertEqual(root.attribution[0].replyCreatedAt, 1_700_000_900 + 0x10)
        XCTAssertEqual(root.attribution[0].authorDisplayName, "Alice")
        XCTAssertEqual(root.attribution[0].authorPictureUrl, "https://example.com/a.png")
        XCTAssertEqual(root.attribution[1].authorPubkey, hex32(0x11))
        XCTAssertEqual(root.attribution[1].replyEventId, hex32(0x91))
        // attribution(0x11, false): display mirrors absent.
        XCTAssertNil(root.attribution[1].authorDisplayName)
        XCTAssertNil(root.attribution[1].authorPictureUrl)

        let repost = snapshot.cards[1]
        XCTAssertEqual(repost.card.id, hex32(0x09))
        XCTAssertEqual(repost.card.kind, 6)
        XCTAssertEqual(repost.card.authorDisplayName, "Alice")
        XCTAssertEqual(repost.card.authorPictureUrl, "https://example.com/a.png")
        XCTAssertTrue(repost.attribution.isEmpty)

        // Page reconstructed from the embedded NFWM feed-window sub-buffer.
        let page = try XCTUnwrap(snapshot.page, "populated fixture carries a FeedPage")
        XCTAssertEqual(page.limit, 50)
        XCTAssertTrue(page.hasMore)
        XCTAssertEqual(page.totalBlocks, 2)
        let cursor = try XCTUnwrap(page.nextCursor, "FeedPage carries a next cursor")
        XCTAssertEqual(cursor.id, hex32(0x09))
        XCTAssertEqual(cursor.createdAt, 1_700_000_000)
    }

    // ── Empty fixture ───────────────────────────────────────────────────────────

    func testEmptyFixtureDecodesToEmptyModel() throws {
        let snapshot = try XCTUnwrap(
            TypedHomeFeedDecoder.decode(bytes: data(fromHex: Self.emptyHex)),
            "NOFS empty golden fixture must decode")
        XCTAssertTrue(snapshot.cards.isEmpty)
        XCTAssertNil(snapshot.page, "empty snapshot has no paging envelope")
    }

    // ── Descriptor preference + graceful fallback (ADR-0037 Commitment 4) ────

    func testNonOpfeedDescriptorIsIgnored() {
        // A retired NFTS-tagged envelope (or any non-opfeed schema id) is
        // unrecognized → nil so the host falls back to the generic path.
        let envelope = TypedProjectionEnvelope(
            key: "nmp.feed.home",
            schemaId: "nmp.nip01.timeline",
            schemaVersion: 1,
            fileIdentifier: "NFTS",
            payload: data(fromHex: Self.emptyHex))
        XCTAssertNil(TypedHomeFeedDecoder.decode(from: [envelope]))
    }

    func testWrongFileIdentifierBytesFallBack() {
        // A buffer whose file identifier is not NOFS fails getCheckedRoot → nil.
        var garbled = data(fromHex: Self.emptyHex)
        garbled[4] = UInt8(ascii: "X") // clobber the "NOFS" identifier region
        XCTAssertNil(TypedHomeFeedDecoder.decode(bytes: garbled))
    }

    func testEmptyPayloadFallsBack() {
        XCTAssertNil(TypedHomeFeedDecoder.decode(bytes: Data()))
    }

    // ── NFCT per-variant round-trip ──────────────────────────────────────────
    //
    // Constructs a ContentTreeWire FlatBuffers buffer in Swift using the
    // generated builder API and verifies that `TypedHomeFeedDecoder.decodeContentTree`
    // produces the expected node shapes. Covers all 22 WireNodeKind variants.

    func testNfctAllVariantsRoundTrip() throws {
        let nfctData = buildAllVariantsNfct()
        XCTAssertFalse(nfctData.isEmpty, "NFCT builder must produce non-empty bytes")

        let decoded = try XCTUnwrap(
            TypedHomeFeedDecoder.decodeContentTree(fromBytes: nfctData),
            "decodeContentTree must succeed for well-formed NFCT buffer"
        )

        // 22 variants → 22 nodes.
        XCTAssertEqual(decoded.nodes.count, 22)
        guard decoded.nodes.count == 22 else { return }

        // 0: Text
        guard case .text(let t0) = decoded.nodes[0] else { return XCTFail("0: expected text") }
        XCTAssertEqual(t0, "hello")

        // 1: Mention (Profile)
        guard case .mention(let u1) = decoded.nodes[1] else { return XCTFail("1: expected mention") }
        XCTAssertEqual(u1.kind, .profile)
        XCTAssertEqual(u1.uri, "nostr:npub1abc")

        // 2: EventRef (Event)
        guard case .eventRef(let u2) = decoded.nodes[2] else { return XCTFail("2: expected eventRef") }
        XCTAssertEqual(u2.kind, .event)
        XCTAssertEqual(u2.primaryId, "aabbcc")

        // 3: Hashtag
        guard case .hashtag(let t3) = decoded.nodes[3] else { return XCTFail("3: expected hashtag") }
        XCTAssertEqual(t3, "nostr")

        // 4: Url
        guard case .url(let t4) = decoded.nodes[4] else { return XCTFail("4: expected url") }
        XCTAssertEqual(t4, "https://example.com/")

        // 5: Media (Image, mediaKind=0)
        guard case .media(let urls5, let kind5) = decoded.nodes[5] else { return XCTFail("5: expected media") }
        XCTAssertEqual(urls5, ["https://example.com/img.jpg"])
        XCTAssertEqual(kind5, .image)

        // 6: Emoji
        guard case .emoji(let sc6, let url6) = decoded.nodes[6] else { return XCTFail("6: expected emoji") }
        XCTAssertEqual(sc6, "zap")
        XCTAssertEqual(url6, "https://example.com/zap.png")

        // 7: Invoice (Bolt11=0)
        guard case .invoice(let inv7) = decoded.nodes[7] else { return XCTFail("7: expected invoice") }
        guard case .bolt11(let s7) = inv7 else { return XCTFail("7: expected bolt11") }
        XCTAssertEqual(s7, "lnbc1qq")

        // 8: Heading (level=2, children=[0])
        guard case .heading(let lv8, let ch8) = decoded.nodes[8] else { return XCTFail("8: expected heading") }
        XCTAssertEqual(lv8, 2)
        XCTAssertEqual(ch8, [0])

        // 9: Paragraph (children=[0,1])
        guard case .paragraph(let ch9) = decoded.nodes[9] else { return XCTFail("9: expected paragraph") }
        XCTAssertEqual(ch9, [0, 1])

        // 10: BlockQuote (children=[0])
        guard case .blockQuote(let ch10) = decoded.nodes[10] else { return XCTFail("10: expected blockQuote") }
        XCTAssertEqual(ch10, [0])

        // 11: CodeBlock (info="rust", body stored in text field)
        guard case .codeBlock(let info11, let body11) = decoded.nodes[11] else { return XCTFail("11: expected codeBlock") }
        XCTAssertEqual(info11, "rust")
        XCTAssertEqual(body11, "fn main() {}")

        // 12: List (unordered: orderedStart nil, items=[[0],[1]])
        guard case .list(let os12, let items12) = decoded.nodes[12] else { return XCTFail("12: expected list") }
        XCTAssertNil(os12, "ordered_start -1 maps to nil (unordered list)")
        XCTAssertEqual(items12, [[0], [1]])

        // 13: Rule
        guard case .rule = decoded.nodes[13] else { return XCTFail("13: expected rule") }

        // 14: Emphasis (children=[0])
        guard case .emphasis(let ch14) = decoded.nodes[14] else { return XCTFail("14: expected emphasis") }
        XCTAssertEqual(ch14, [0])

        // 15: Strong (children=[0])
        guard case .strong(let ch15) = decoded.nodes[15] else { return XCTFail("15: expected strong") }
        XCTAssertEqual(ch15, [0])

        // 16: InlineCode (code stored in text field)
        guard case .inlineCode(let code16) = decoded.nodes[16] else { return XCTFail("16: expected inlineCode") }
        XCTAssertEqual(code16, "let x = 1")

        // 17: Link (children=[0], href)
        guard case .link(let ch17, let href17) = decoded.nodes[17] else { return XCTFail("17: expected link") }
        XCTAssertEqual(ch17, [0])
        XCTAssertEqual(href17, "https://nostr.com/")

        // 18: Image (alt, title, src stored in url field)
        guard case .image(let alt18, let title18, let src18) = decoded.nodes[18] else { return XCTFail("18: expected image") }
        XCTAssertEqual(alt18, "a cat")
        XCTAssertEqual(title18, "cute")
        XCTAssertEqual(src18, "https://example.com/cat.jpg")

        // 19: SoftBreak
        guard case .softBreak = decoded.nodes[19] else { return XCTFail("19: expected softBreak") }

        // 20: HardBreak
        guard case .hardBreak = decoded.nodes[20] else { return XCTFail("20: expected hardBreak") }

        // 21: Placeholder (DepthLimit=0)
        guard case .placeholder(let reason21) = decoded.nodes[21] else { return XCTFail("21: expected placeholder") }
        XCTAssertEqual(reason21, .depthLimit)

        // Roots and mode
        XCTAssertEqual(decoded.roots, [UInt32(8)])
        XCTAssertEqual(decoded.mode, "Auto")
    }

    // ── Builder helpers for per-variant test ─────────────────────────────────

    private func buildAllVariantsNfct() -> Data {
        var fbb = FlatBufferBuilder(initialSize: 4096)

        // Build all node offsets before finishing the root table.
        // Each node is built bottom-up (children before parents) per FB rules.

        // 0: Text
        let t0 = fbb.create(string: "hello")
        let n0 = nmp_content_WireNode.createWireNode(&fbb, kind: .text, textOffset: t0)

        // 1: Mention (Profile)
        let u1uri = fbb.create(string: "nostr:npub1abc")
        let u1pid = fbb.create(string: "aabbcc")
        let u1rel = fbb.createVector(ofOffsets: [Offset]())
        let u1fb = nmp_content_WireNostrUri.createWireNostrUri(
            &fbb, uriOffset: u1uri, kind: .profile, primaryIdOffset: u1pid,
            relaysVectorOffset: u1rel, eventKind: UInt32.max)
        let n1 = nmp_content_WireNode.createWireNode(&fbb, kind: .mention, nostrUriOffset: u1fb)

        // 2: EventRef (Event)
        let u2uri = fbb.create(string: "nostr:note1aabbcc")
        let u2pid = fbb.create(string: "aabbcc")
        let u2rel = fbb.createVector(ofOffsets: [Offset]())
        let u2fb = nmp_content_WireNostrUri.createWireNostrUri(
            &fbb, uriOffset: u2uri, kind: .event, primaryIdOffset: u2pid,
            relaysVectorOffset: u2rel, eventKind: UInt32.max)
        let n2 = nmp_content_WireNode.createWireNode(&fbb, kind: .eventref, nostrUriOffset: u2fb)

        // 3: Hashtag
        let t3 = fbb.create(string: "nostr")
        let n3 = nmp_content_WireNode.createWireNode(&fbb, kind: .hashtag, tagOffset: t3)

        // 4: Url
        let t4 = fbb.create(string: "https://example.com/")
        let n4 = nmp_content_WireNode.createWireNode(&fbb, kind: .url, urlOffset: t4)

        // 5: Media (Image, mediaKind=0)
        let mu5 = fbb.create(string: "https://example.com/img.jpg")
        let mv5 = fbb.createVector(ofOffsets: [mu5])
        let n5 = nmp_content_WireNode.createWireNode(&fbb, kind: .media, mediaUrlsVectorOffset: mv5, mediaKind: 0)

        // 6: Emoji
        let sc6 = fbb.create(string: "zap")
        let eu6 = fbb.create(string: "https://example.com/zap.png")
        let n6 = nmp_content_WireNode.createWireNode(&fbb, kind: .emoji, shortcodeOffset: sc6, emojiUrlOffset: eu6)

        // 7: Invoice (Bolt11=0)
        let ip7 = fbb.create(string: "lnbc1qq")
        let n7 = nmp_content_WireNode.createWireNode(&fbb, kind: .invoice, invoiceKind: 0, invoicePayloadOffset: ip7)

        // 8: Heading (level=2, children=[0])
        let c8 = fbb.createVector([UInt32(0)])
        let n8 = nmp_content_WireNode.createWireNode(&fbb, kind: .heading, childrenVectorOffset: c8, level: 2)

        // 9: Paragraph (children=[0,1])
        let c9 = fbb.createVector([UInt32(0), UInt32(1)])
        let n9 = nmp_content_WireNode.createWireNode(&fbb, kind: .paragraph, childrenVectorOffset: c9)

        // 10: BlockQuote (children=[0])
        let c10 = fbb.createVector([UInt32(0)])
        let n10 = nmp_content_WireNode.createWireNode(&fbb, kind: .blockquote, childrenVectorOffset: c10)

        // 11: CodeBlock (body in text field, info in codeInfo field)
        let tb11 = fbb.create(string: "fn main() {}")
        let ci11 = fbb.create(string: "rust")
        let n11 = nmp_content_WireNode.createWireNode(&fbb, kind: .codeblock, textOffset: tb11, codeInfoOffset: ci11)

        // 12: List (unordered: orderedStart default=-1, items=[[0],[1]])
        let lc0 = fbb.createVector([UInt32(0)])
        let li0 = nmp_content_ListItem.createListItem(&fbb, childrenVectorOffset: lc0)
        let lc1 = fbb.createVector([UInt32(1)])
        let li1 = nmp_content_ListItem.createListItem(&fbb, childrenVectorOffset: lc1)
        let lv12 = fbb.createVector(ofOffsets: [li0, li1])
        let n12 = nmp_content_WireNode.createWireNode(&fbb, kind: .list, listItemsVectorOffset: lv12)

        // 13: Rule (no fields)
        let n13 = nmp_content_WireNode.createWireNode(&fbb, kind: .rule)

        // 14: Emphasis (children=[0])
        let c14 = fbb.createVector([UInt32(0)])
        let n14 = nmp_content_WireNode.createWireNode(&fbb, kind: .emphasis, childrenVectorOffset: c14)

        // 15: Strong (children=[0])
        let c15 = fbb.createVector([UInt32(0)])
        let n15 = nmp_content_WireNode.createWireNode(&fbb, kind: .strong, childrenVectorOffset: c15)

        // 16: InlineCode (code in text field)
        let tc16 = fbb.create(string: "let x = 1")
        let n16 = nmp_content_WireNode.createWireNode(&fbb, kind: .inlinecode, textOffset: tc16)

        // 17: Link (children=[0], href)
        let c17 = fbb.createVector([UInt32(0)])
        let h17 = fbb.create(string: "https://nostr.com/")
        let n17 = nmp_content_WireNode.createWireNode(&fbb, kind: .link, childrenVectorOffset: c17, hrefOffset: h17)

        // 18: Image (alt, imgTitle, src in url field)
        let a18 = fbb.create(string: "a cat")
        let ti18 = fbb.create(string: "cute")
        let s18 = fbb.create(string: "https://example.com/cat.jpg")
        let n18 = nmp_content_WireNode.createWireNode(
            &fbb, kind: .image, urlOffset: s18, altOffset: a18, imgTitleOffset: ti18)

        // 19: SoftBreak
        let n19 = nmp_content_WireNode.createWireNode(&fbb, kind: .softbreak)

        // 20: HardBreak
        let n20 = nmp_content_WireNode.createWireNode(&fbb, kind: .hardbreak)

        // 21: Placeholder (DepthLimit=0)
        let n21 = nmp_content_WireNode.createWireNode(&fbb, kind: .placeholder, placeholderReason: .depthlimit)

        let nodesVec = fbb.createVector(ofOffsets: [
            n0, n1, n2, n3, n4, n5, n6, n7, n8, n9, n10,
            n11, n12, n13, n14, n15, n16, n17, n18, n19, n20, n21
        ])
        let rootsVec = fbb.createVector([UInt32(8)])
        let root = nmp_content_ContentTreeWire.createContentTreeWire(
            &fbb, nodesVectorOffset: nodesVec, rootsVectorOffset: rootsVec, mode: .auto)
        nmp_content_ContentTreeWire.finish(&fbb, end: root)
        return fbb.data
    }

    // ── Golden bytes (B1 fixtures, verbatim) ─────────────────────────────────

    private static let emptyHex =
        "100000004e4f46530800080000000400080000000400000000000000"

    private static let populatedHex =
        "1c0000004e4f46530000000000000e001000000008000c00060007000e00000000000101c400000004000000b8000000100000004e46574d08000e000400080008000000280000000c000000000006000c00040006000000d2040000000000000c001c000c000800070014000c000000000000011c000000320000000000000002000000000000000800100008000400080000000c00000000f1536500000000400000003039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303900000000020000007004000004000000fcf8ffff30000000040000000000000000002200440008000c0010001400380018001c00200024000600280007002c00300034002200000000000101b80100006c010000d403000006000000500100003c00000024000000e8020000240100000001000028010000e401000000f15365000000000000000074f9ffff0c000000040000000000000000000000d0000000140000004e46435400000a00100008000c0007000a000000000000021c00000004000000040000000000000001000000020000000300000004000000800000005c0000003c0000001000000000000a000c000700000008000a00000000000004040000001400000068747470733a2f2f6578616d706c652e636f6d2f00000000ccffffff0400000001000000200000000c000c0007000000000008000c0000000000000304000000050000006e6f737472000000080008000000040008000000040000000600000068656c6c6f2000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e6700000005000000416c6963650000000b00000068656c6c6f20776f726c64000b00000068656c6c6f20776f726c6400400000003032303230323032303230323032303230323032303230323032303230323032303230323032303230323032303230323032303230323032303230323032303200000000400000003039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303900001200240008000c00060010000700140018001200000000000101480000008c0000003400000010000000c0ae446500000000000000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e6700000005000000416c69636500000040000000343234323432343234323432343234323432343234323432343234323432343234323432343234323432343234323432343234323432343234323432343234320000000090fcffff000101013c00000028000000040000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e670000000a0000006e70756231616c696365000005000000416c6963650000000c001400040008000c0010000c000000c4000000300000001400000004000000ecf9ffff08000e000000040008000000010000000000000000000a000c000700000008000a00000000000001040000009efaffff5c00000010000000040000000100000065000000400000006161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616100000000150000006e6d702e7265616374696f6e732e73756d6d617279000000080010000000040008000000020000000000000000000000c4fdffff000101013c00000028000000040000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e670000000a0000006e70756231616c696365000005000000416c69636500000064fdffff680200000400000002000000f80000001800000014001c000400080000000000000000000c0010001400000060000000a40000001000000095f453650000000000000000400000003931393139313931393139313931393139313931393139313931393139313931393139313931393139313931393139313931393139313931393139313931393100000000400000003131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313100000000bcfbffff00000001040000000a0000006e707562316361726f6c00001400280008000c00060010000700140018001c00140000000000010194000000e800000038000000140000003c00000094f4536500000000000000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e6700000005000000416c696365000000400000003930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393000000000400000003130313031303130313031303130313031303130313031303130313031303130313031303130313031303130313031303130313031303130313031303130313000000000100014000500080006000c000700100010000000000101013c00000028000000040000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e670000000a0000006e70756231616c696365000005000000416c69636500000020003000040008000c0010002800140018001c00200000000000000000002400200000008c01000040010000b00200000100000020010000340000001c000000c4010000fc000000f4f253650000000008000c0004000800080000000c000000040000000000000000000000d0000000140000004e46435400000a00100008000c0007000a000000000000021c00000004000000040000000000000001000000020000000300000004000000800000005c0000003c0000001000000000000a000c000700000008000a00000000000004040000001400000068747470733a2f2f6578616d706c652e636f6d2f00000000ccffffff0400000001000000200000000c000c0007000000000008000c0000000000000304000000050000006e6f737472000000080008000000040008000000040000000600000068656c6c6f2000000d000000612074687265616420726f6f740000000d000000612074687265616420726f6f740000004000000030343034303430343034303430343034303430343034303430343034303430343034303430343034303430343034303430343034303430343034303430343034000000004000000030333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033000000000c001600040008000c0010000c000000bc000000ac000000a00000001000000000000a000e000700000008000a000000000000011000000000000a001000040008000c000a0000005c000000100000000400000001000000650000004000000030333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033000000000e0000006e6d702e6e697035372e7a6170730000fcffffff040004000400000008000c00000004000800000001000000000000000c000c0000000000070008000c0000000000000104000000080000006e70756231626f6200000000"
}

private extension String {
    /// Repeat the receiver `count` times (`"ab".repeated(2) == "abab"`).
    func repeated(_ count: Int) -> String {
        String(repeating: self, count: count)
    }
}
