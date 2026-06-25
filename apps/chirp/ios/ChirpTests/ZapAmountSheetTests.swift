import XCTest
@testable import Chirp

// V-106 — proves the zap amount picker's pure presentation logic: sats→msats
// conversion, custom-field parsing (the guard that prevents a zero-amount
// zap), and the static preset ladder. The sheet replaces the old hardcoded
// 21,000-msat default; these tests pin the conversion the kernel's
// `nmp.nip57.zap` action body depends on (`amount_msats`).
final class ZapAmountSheetTests: XCTestCase {
    func testSatsToMsatsConversion() {
        XCTAssertEqual(zapMsats(fromSats: 21), 21_000)
        XCTAssertEqual(zapMsats(fromSats: 1_000), 1_000_000)
        XCTAssertEqual(zapMsats(fromSats: 21_000), 21_000_000)
        XCTAssertEqual(zapMsats(fromSats: 1), 1_000)
    }

    func testPresetLadderIsTheExpectedSatsValues() {
        // Static UI constants — 21k stays available, but as a *choice*.
        XCTAssertEqual(zapPresetSats, [21, 100, 500, 1_000, 5_000, 21_000])
    }

    func testEveryPresetResolvesToANonZeroMsatsAmount() {
        for sats in zapPresetSats {
            XCTAssertGreaterThan(zapMsats(fromSats: sats), 0)
        }
    }

    func testParseCustomAcceptsPlainDigits() {
        XCTAssertEqual(parseCustomZapMsats("42"), 42_000)
        XCTAssertEqual(parseCustomZapMsats("1"), 1_000)
    }

    func testParseCustomStripsGroupingAndWhitespace() {
        XCTAssertEqual(parseCustomZapMsats("1,234"), 1_234_000)
        XCTAssertEqual(parseCustomZapMsats(" 500 "), 500_000)
    }

    func testParseCustomRejectsEmptyZeroAndNonNumeric() {
        // The confirm button keys on these returning nil, so the host can
        // never dispatch a zero-amount or garbage zap.
        XCTAssertNil(parseCustomZapMsats(""))
        XCTAssertNil(parseCustomZapMsats("0"))
        XCTAssertNil(parseCustomZapMsats("abc"))
        XCTAssertNil(parseCustomZapMsats("   "))
    }
}
