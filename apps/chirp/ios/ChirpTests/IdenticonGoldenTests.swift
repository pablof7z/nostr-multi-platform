import XCTest
@testable import Chirp

/// Cross-platform golden tests for `NostrIdenticon`.
///
/// These tests encode hardcoded expected values that are byte-identical to the
/// Kotlin golden test in
/// `apps/chirp/android/app/src/test/java/org/nmp/android/IdenticonGoldenTest.kt`.
/// Both test suites verify the same pubkeys → same 5×5 cell patterns and same
/// hue values, proving the djb2-based algorithm is byte-identical across
/// iOS and Android (#2224).
///
/// Reference algorithm (djb2 + symmetric 5×5 grid):
///   hash = 5381
///   for each UTF-8 byte: hash = (hash * 33 + byte) mod 2^32
///   color hue = hash % 360 / 360.0  (HSB S=0.55, B=0.75)
///   cells: bits 0–14 of hash → 5 rows × 3 left cols; cols 3–4 mirror cols 1–0
final class IdenticonGoldenTests: XCTestCase {

    // MARK: - djb2 cell pattern for pubkey "a" (ASCII 97)
    //
    // hash("a") = 5381 * 33 + 97 = 177670
    // hash("a") & 0x7FFF (lower 15 bits) = 177670 & 32767 = 13830
    // 13830 in binary: 11 0110 0000 0110
    //   bit 0=0, bit 1=1, bit 2=1, bit 3=0, bit 4=0, bit 5=0,
    //   bit 6=0, bit 7=0, bit 8=0, bit 9=1, bit 10=1, bit 11=0,
    //   bit 12=1, bit 13=1, bit 14=0
    //
    // Row 0: col0=bit0=0 → F, col1=bit1=1 → T, col2=bit2=1 → T  →  [F,T,T,T,F]
    // Row 1: col0=bit3=0 → F, col1=bit4=0 → F, col2=bit5=0 → F  →  [F,F,F,F,F]
    // Row 2: col0=bit6=0 → F, col1=bit7=0 → F, col2=bit8=0 → F  →  [F,F,F,F,F]
    // Row 3: col0=bit9=1 → T, col1=bit10=1 → T, col2=bit11=0 → F →  [T,T,F,T,T]
    // Row 4: col0=bit12=1 → T, col1=bit13=1 → T, col2=bit14=0 → F → [T,T,F,T,T]

    func testCellsForSingleCharPubkey() {
        let cells = NostrIdenticon.cells(forPubkey: "a")
        XCTAssertEqual(cells.count, 5, "must have exactly 5 rows")
        XCTAssertEqual(cells[0], [false, true,  true,  true,  false], "row 0")
        XCTAssertEqual(cells[1], [false, false, false, false, false], "row 1")
        XCTAssertEqual(cells[2], [false, false, false, false, false], "row 2")
        XCTAssertEqual(cells[3], [true,  true,  false, true,  true],  "row 3")
        XCTAssertEqual(cells[4], [true,  true,  false, true,  true],  "row 4")
    }

    // MARK: - djb2 cell pattern for empty pubkey ""
    //
    // hash("") = 5381 (seed, no bytes processed)
    // 5381 in binary: 1 0101 0000 0101
    //   bit 0=1, bit 1=0, bit 2=1, bit 3=0, bit 4=0, bit 5=0,
    //   bit 6=0, bit 7=0, bit 8=1, bit 9=0, bit 10=1, bit 11=0,
    //   bit 12=1, bit 13=0, bit 14=0
    //
    // Row 0: col0=bit0=1 → T, col1=bit1=0 → F, col2=bit2=1 → T  →  [T,F,T,F,T]
    // Row 1: col0=bit3=0 → F, col1=bit4=0 → F, col2=bit5=0 → F  →  [F,F,F,F,F]
    // Row 2: col0=bit6=0 → F, col1=bit7=0 → F, col2=bit8=1 → T  →  [F,F,T,F,F]
    // Row 3: col0=bit9=0 → F, col1=bit10=1 → T, col2=bit11=0 → F →  [F,T,F,T,F]
    // Row 4: col0=bit12=1 → T, col1=bit13=0 → F, col2=bit14=0 → F → [T,F,F,F,T]

    func testCellsForEmptyPubkey() {
        let cells = NostrIdenticon.cells(forPubkey: "")
        XCTAssertEqual(cells[0], [true,  false, true,  false, true],  "row 0")
        XCTAssertEqual(cells[1], [false, false, false, false, false], "row 1")
        XCTAssertEqual(cells[2], [false, false, true,  false, false], "row 2")
        XCTAssertEqual(cells[3], [false, true,  false, true,  false], "row 3")
        XCTAssertEqual(cells[4], [true,  false, false, false, true],  "row 4")
    }

    // MARK: - Real 64-hex pubkey full-grid golden
    //
    // pubkey = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
    // djb2(pubkey) = 1119655360
    // lower 15 bits = 1119655360 & 0x7FFF = 5568 = 0b001_0101_1100_0000
    //   set bits: {6, 7, 8, 10, 12}; all others 0
    //
    // Row 0: bits 0,1,2  = 0,0,0  → cols [F,F,F] → mirror → [F,F,F,F,F]
    // Row 1: bits 3,4,5  = 0,0,0  → cols [F,F,F] → mirror → [F,F,F,F,F]
    // Row 2: bits 6,7,8  = 1,1,1  → cols [T,T,T] → mirror → [T,T,T,T,T]
    // Row 3: bits 9,10,11= 0,1,0  → cols [F,T,F] → mirror → [F,T,F,T,F]
    // Row 4: bits 12,13,14=1,0,0  → cols [T,F,F] → mirror → [T,F,F,F,T]
    //
    // These exact values are hardcoded identically in the Kotlin golden
    // `IdenticonGoldenTest.kt` (cross-platform parity anchor).

    func testCellsForRealHexPubkey() {
        let pubkey = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
        let cells = NostrIdenticon.cells(forPubkey: pubkey)
        XCTAssertEqual(cells[0], [false, false, false, false, false], "row 0")
        XCTAssertEqual(cells[1], [false, false, false, false, false], "row 1")
        XCTAssertEqual(cells[2], [true,  true,  true,  true,  true],  "row 2")
        XCTAssertEqual(cells[3], [false, true,  false, true,  false], "row 3")
        XCTAssertEqual(cells[4], [true,  false, false, false, true],  "row 4")
    }

    func testColorHueForRealHexPubkey() {
        // 1119655360 % 360 = 280; hue = 280/360 ≈ 0.7778
        let color = NostrIdenticon.color(forPubkey:
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d")
        var h: CGFloat = 0, s: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        UIColor(color).getHue(&h, saturation: &s, brightness: &b, alpha: &a)
        XCTAssertEqual(Double(h), 280.0 / 360.0, accuracy: 0.001, "hue for fiatjaf pubkey")
        XCTAssertEqual(Double(s), 0.55, accuracy: 0.001, "saturation fixed at 0.55")
        XCTAssertEqual(Double(b), 0.75, accuracy: 0.001, "brightness fixed at 0.75")
    }

    // MARK: - Symmetry invariant

    func testCellsAreAlwaysHorizontallySymmetric() {
        let pubkeys = [
            "a",
            "",
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
            "0000000000000000000000000000000000000000000000000000000000000001",
        ]
        for pk in pubkeys {
            let cells = NostrIdenticon.cells(forPubkey: pk)
            for row in 0..<5 {
                XCTAssertEqual(cells[row][0], cells[row][4], "\(pk) row \(row): col0 must mirror col4")
                XCTAssertEqual(cells[row][1], cells[row][3], "\(pk) row \(row): col1 must mirror col3")
            }
        }
    }

    // MARK: - Color hue for pubkey "a"
    //
    // hash("a") = 177670; 177670 % 360 = 190; hue = 190/360 ≈ 0.5278

    func testColorHueForSingleCharPubkey() {
        let color = NostrIdenticon.color(forPubkey: "a")
        // Extract hue component — approximate comparison (floating-point)
        var h: CGFloat = 0, s: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        UIColor(color).getHue(&h, saturation: &s, brightness: &b, alpha: &a)
        XCTAssertEqual(Double(h), 190.0 / 360.0, accuracy: 0.001, "hue for 'a'")
        XCTAssertEqual(Double(s), 0.55, accuracy: 0.001, "saturation fixed at 0.55")
        XCTAssertEqual(Double(b), 0.75, accuracy: 0.001, "brightness fixed at 0.75")
    }
}
