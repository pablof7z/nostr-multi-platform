package org.nmp.gallery.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

/**
 * Deterministic identicon (D1 placeholder) — a stable 5×5 symmetric blocky
 * avatar seeded by the pubkey hex string. Byte-for-byte port of the Swift
 * `Identicon` so output matches between platforms.
 *
 * Important: the seed bytes are the **UTF-8 bytes of the hex string**, not
 * the hex-decoded bytes. The hue is computed by summing those bytes mod 360
 * (Swift's `Int($0) &+ Int($1)` wraps; for our hex inputs the sum stays
 * well below Int max, so the wrap is moot).
 */
@Composable
fun Identicon(seed: String, modifier: Modifier = Modifier) {
    val bytes = remember(seed) { seed.toByteArray(Charsets.UTF_8) }
    val hueDeg = remember(bytes) {
        if (bytes.isEmpty()) 0f
        else (bytes.fold(0) { acc, b -> acc + (b.toInt() and 0xFF) } % 360).toFloat()
    }
    val color = Color.hsv(hueDeg, 0.55f, 0.75f)

    Canvas(
        modifier
            .clip(RoundedCornerShape(4.dp))
            .background(Color.Gray.copy(alpha = 0.15f)),
    ) {
        val cell = size.width / 5f
        val cellSize = Size(cell, cell)
        for (row in 0 until 5) {
            for (col in 0 until 3) {
                val on = bit(bytes, row * 3 + col)
                if (on) {
                    drawRect(
                        color = color,
                        topLeft = Offset(col * cell, row * cell),
                        size = cellSize,
                    )
                    drawRect(
                        color = color,
                        topLeft = Offset((4 - col) * cell, row * cell),
                        size = cellSize,
                    )
                }
            }
        }
    }
}

private fun bit(bytes: ByteArray, i: Int): Boolean {
    if (bytes.isEmpty()) return false
    return (bytes[i % bytes.size].toInt() and 1) == 1
}
