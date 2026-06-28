package org.nmp.android

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test
import org.nmp.android.components.NostrIdenticon

/**
 * Cross-platform golden tests for [NostrIdenticon].
 *
 * These tests encode hardcoded expected values that are byte-identical to the
 * Swift golden test in
 * `apps/chirp/ios/ChirpTests/IdenticonGoldenTests.swift`. Both test suites
 * verify the same pubkeys → same 5×5 cell patterns, proving the djb2-based
 * algorithm is byte-identical across iOS and Android (#2224).
 *
 * Reference algorithm (djb2 + symmetric 5×5 grid):
 *   hash = 5381u
 *   for each UTF-8 byte: hash = (hash * 33u + byte.toUByte()) mod 2^32
 *   color hue = (hash % 360) / 360.0  (HSV S=0.55, V=0.75)
 *   cells: bits 0–14 of hash → 5 rows × 3 left cols; cols 3–4 mirror cols 1–0
 */
class IdenticonGoldenTest {

    // ── pubkey "a" ────────────────────────────────────────────────────────────
    //
    // hash("a") = 5381 * 33 + 97 = 177670
    // lower 15 bits of 177670: 177670 & 32767 = 13830
    // 13830 = 0b11_0110_0000_0110
    //   bit 0=0, bit 1=1, bit 2=1, bit 3=0, bit 4=0, bit 5=0,
    //   bit 6=0, bit 7=0, bit 8=0, bit 9=1, bit 10=1, bit 11=0,
    //   bit 12=1, bit 13=1, bit 14=0
    //
    // Row 0: [F,T,T,T,F]   Row 1: [F,F,F,F,F]   Row 2: [F,F,F,F,F]
    // Row 3: [T,T,F,T,T]   Row 4: [T,T,F,T,T]

    @Test
    fun cellsForSingleCharPubkey_matchesSwiftGolden() {
        val cells = NostrIdenticon.cellsForPubkey("a")
        assertEquals("must have exactly 5 rows", 5, cells.size)
        assertArrayEquals("row 0", booleanArrayOf(false, true,  true,  true,  false), cells[0])
        assertArrayEquals("row 1", booleanArrayOf(false, false, false, false, false), cells[1])
        assertArrayEquals("row 2", booleanArrayOf(false, false, false, false, false), cells[2])
        assertArrayEquals("row 3", booleanArrayOf(true,  true,  false, true,  true),  cells[3])
        assertArrayEquals("row 4", booleanArrayOf(true,  true,  false, true,  true),  cells[4])
    }

    // ── empty pubkey "" ───────────────────────────────────────────────────────
    //
    // hash("") = 5381 (seed, no bytes processed)
    // Row 0: [T,F,T,F,T]   Row 1: [F,F,F,F,F]   Row 2: [F,F,T,F,F]
    // Row 3: [F,T,F,T,F]   Row 4: [T,F,F,F,T]

    @Test
    fun cellsForEmptyPubkey_matchesSwiftGolden() {
        val cells = NostrIdenticon.cellsForPubkey("")
        assertArrayEquals("row 0", booleanArrayOf(true,  false, true,  false, true),  cells[0])
        assertArrayEquals("row 1", booleanArrayOf(false, false, false, false, false), cells[1])
        assertArrayEquals("row 2", booleanArrayOf(false, false, true,  false, false), cells[2])
        assertArrayEquals("row 3", booleanArrayOf(false, true,  false, true,  false), cells[3])
        assertArrayEquals("row 4", booleanArrayOf(true,  false, false, false, true),  cells[4])
    }

    // ── Real 64-hex pubkey full-grid golden ───────────────────────────────────
    //
    // pubkey = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
    // djb2(pubkey) = 1119655360
    // lower 15 bits = 1119655360 & 0x7FFF = 5568 = 0b001_0101_1100_0000
    //   set bits: {6, 7, 8, 10, 12}; all others 0
    //
    // Row 0: [F,F,F,F,F]   Row 1: [F,F,F,F,F]   Row 2: [T,T,T,T,T]
    // Row 3: [F,T,F,T,F]   Row 4: [T,F,F,F,T]
    //
    // Hardcoded identically in the Swift golden IdenticonGoldenTests.swift.

    @Test
    fun cellsForRealHexPubkey_matchesSwiftGolden() {
        val pubkey = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
        val cells = NostrIdenticon.cellsForPubkey(pubkey)
        assertArrayEquals("row 0", booleanArrayOf(false, false, false, false, false), cells[0])
        assertArrayEquals("row 1", booleanArrayOf(false, false, false, false, false), cells[1])
        assertArrayEquals("row 2", booleanArrayOf(true,  true,  true,  true,  true),  cells[2])
        assertArrayEquals("row 3", booleanArrayOf(false, true,  false, true,  false), cells[3])
        assertArrayEquals("row 4", booleanArrayOf(true,  false, false, false, true),  cells[4])
    }

    @Test
    fun colorForRealHexPubkey_rgbMatchesHsvFormula() {
        // 1119655360 % 360 = 280; Color.hsv(280f, 0.55f, 0.75f)
        // HSV→RGB: (R, G, B) = (0.6125, 0.3375, 0.75)
        val color = NostrIdenticon.colorForPubkey(
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
        )
        assertEquals("red ≈ 0.6125",  0.6125f, color.red,   0.01f)
        assertEquals("green ≈ 0.3375", 0.3375f, color.green, 0.01f)
        assertEquals("blue = 0.75",   0.75f,   color.blue,  0.01f)
    }

    // ── Symmetry invariant ────────────────────────────────────────────────────

    @Test
    fun cells_areAlwaysHorizontallySymmetric() {
        val pubkeys = listOf(
            "a",
            "",
            "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        for (pk in pubkeys) {
            val cells = NostrIdenticon.cellsForPubkey(pk)
            for (row in 0..4) {
                assertEquals("$pk row $row: col0 must mirror col4", cells[row][0], cells[row][4])
                assertEquals("$pk row $row: col1 must mirror col3", cells[row][1], cells[row][3])
            }
        }
    }

    // ── Color determinism ─────────────────────────────────────────────────────
    //
    // hash("a") = 177670; 177670 % 360 = 190; hue = 190f (HSV, S=0.55, V=0.75)
    // Compose Color.hsv(190f, 0.55f, 0.75f) → R≈0.3375, G≈0.6812, B=0.75, A=1.0
    // These RGB values are stable pure-Kotlin computations (no Android runtime).

    @Test
    fun colorForSingleCharPubkey_isOpaqueAndDeterministic() {
        val color = NostrIdenticon.colorForPubkey("a")
        // Compose Color fields are pure float — no Android runtime needed.
        assertEquals("alpha must be fully opaque", 1.0f, color.alpha, 0.001f)
        // Verify reproducibility
        val color2 = NostrIdenticon.colorForPubkey("a")
        assertEquals("color must be deterministic (red)",   color.red,   color2.red,   0.0f)
        assertEquals("color must be deterministic (green)", color.green, color2.green, 0.0f)
        assertEquals("color must be deterministic (blue)",  color.blue,  color2.blue,  0.0f)
    }

    @Test
    fun colorForPubkey_differsAcrossDifferentPubkeys() {
        val colorA = NostrIdenticon.colorForPubkey("a")
        val colorB = NostrIdenticon.colorForPubkey("b")
        // Different pubkeys must produce different hues (hash differs).
        val sameRed   = Math.abs(colorA.red   - colorB.red)   < 0.001f
        val sameGreen = Math.abs(colorA.green - colorB.green) < 0.001f
        val sameBlue  = Math.abs(colorA.blue  - colorB.blue)  < 0.001f
        assert(!(sameRed && sameGreen && sameBlue)) {
            "pubkey 'a' and 'b' must render different colors"
        }
    }

    // ── RGB golden for pubkey "a" (cross-platform spec anchor) ───────────────
    //
    // Color.hsv(190f, 0.55f, 0.75f) in Compose is a pure-Kotlin computation.
    // Computed via HSV→RGB: h_i=3, f=0.1667, p=0.3375, q=0.6812, v=0.75
    // → (R, G, B) = (p, q, v) = (0.3375, 0.6812, 0.75)

    @Test
    fun colorForSingleCharPubkey_rgbMatchesHsvFormula() {
        val color = NostrIdenticon.colorForPubkey("a")
        assertEquals("red ≈ 0.3375",  0.3375f, color.red,   0.01f)
        assertEquals("green ≈ 0.6812", 0.6812f, color.green, 0.01f)
        assertEquals("blue = 0.75",   0.75f,   color.blue,  0.01f)
    }
}
