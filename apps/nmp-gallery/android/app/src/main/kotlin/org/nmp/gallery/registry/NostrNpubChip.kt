package org.nmp.gallery.registry

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Done
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Tappable chip that shows the Rust-truncated npub and copies the full
 * bech32 `npub1…` to the clipboard on tap.
 *
 * `npub` and `npubShort` must come from the kernel projection —
 * never reformat them in Kotlin.
 *
 * Depends on `compose/user-avatar` for [ProfileWire].
 */
@Composable
fun NostrNpubChip(
    profile: ProfileWire,
    modifier: Modifier = Modifier,
) = NostrNpubChip(
    npub = profile.npub,
    npubShort = profile.npubShort,
    modifier = modifier,
)

@Composable
fun NostrNpubChip(
    npub: String,
    npubShort: String,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var copied by remember { mutableStateOf(false) }

    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = modifier,
    ) {
        Text(
            text = npubShort,
            style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.55f),
        )
        Spacer(Modifier.width(2.dp))
        IconButton(
            onClick = {
                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                clipboard.setPrimaryClip(ClipData.newPlainText("npub", npub))
                copied = true
                scope.launch {
                    delay(2_000)
                    copied = false
                }
            },
            modifier = Modifier
                .size(24.dp)
                .semantics { contentDescription = if (copied) "Copied" else "Copy npub" },
        ) {
            Icon(
                imageVector = if (copied) Icons.Filled.Done else Icons.Filled.ContentCopy,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.45f),
                modifier = Modifier.size(14.dp),
            )
        }
    }
}
