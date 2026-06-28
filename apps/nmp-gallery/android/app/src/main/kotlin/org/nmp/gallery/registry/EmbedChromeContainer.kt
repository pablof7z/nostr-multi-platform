// Requires: compose-ui, compose-foundation, compose-material3. Kotlin 1.9+.
//
// Visual chrome shared by every kind-dispatched embed renderer. Compose mirror
// of the SwiftUI `EmbedChromeContainer.swift` and the TUI
// `EmbedChromeContainer`: a left accent stripe whose colour deepens with
// nesting depth, plus a small indent so embedded content reads as a child of
// the surrounding paragraph. The renderer itself draws inside `content` —
// chrome knows nothing about the embedded kind.

package org.nmp.gallery.registry

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp

@Composable
public fun EmbedChromeContainer(
    depth: Int,
    collapsed: Boolean,
    content: @Composable () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .padding(start = (depth * 8).dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.Top,
    ) {
        Box(
            modifier = Modifier
                .width(3.dp)
                .fillMaxHeight()
                .clip(RoundedCornerShape(1.5.dp))
                .background(accentColor(depth, collapsed)),
        )
        Spacer(Modifier.width(10.dp))
        Box(modifier = Modifier.fillMaxWidth()) {
            content()
        }
    }
}

/**
 * Depth-graded accent. Matches the TUI's `Rgb(140, 160 + 8·depth, 220)` blueish
 * ramp; collapsed embeds dim out. Mirrors the SwiftUI `accentColor`.
 */
private fun accentColor(depth: Int, collapsed: Boolean): Color {
    if (collapsed) {
        return Color(red = 100, green = 100, blue = 110)
    }
    val green = (160 + depth * 8).coerceAtMost(200)
    return Color(red = 140, green = green, blue = 220)
}
