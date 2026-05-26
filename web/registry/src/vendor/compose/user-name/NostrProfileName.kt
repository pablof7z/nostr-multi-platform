package org.nmp.registry

import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow

/**
 * Inline display-name text for a Nostr profile.
 *
 * Shows `displayName` when set; falls back to `npubShort`
 * (always Rust-formatted — never reformat in Kotlin).
 *
 * Depends on `compose/user-avatar` for [ProfileWire].
 */
@Composable
fun NostrProfileName(
    profile: ProfileWire,
    modifier: Modifier = Modifier,
    style: TextStyle = LocalTextStyle.current.copy(fontWeight = FontWeight.SemiBold),
    color: Color = Color.Unspecified,
) {
    val label = profile.display
    Text(
        text = label,
        style = style,
        color = color,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier.semantics { contentDescription = "Display name: $label" },
    )
}
