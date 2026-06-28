package org.nmp.android.components

import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf

/**
 * Host bridge for profile projections owned by the NMP kernel.
 *
 * Registry components call this bridge with stable Nostr references. The app
 * supplies one platform adapter that maps [resolveProfileRef] to the kernel's
 * profile `resolve_ref` path and reads the current row from `refs.profile`.
 * Components own when to resolve, release, and re-read the projection.
 */
interface NostrProfileHost {
    @Composable
    fun profileForPubkey(pubkey: String): ProfileWire?
    fun resolveProfileRef(pubkey: String, consumerId: String)
    fun releaseProfileRef(pubkey: String, consumerId: String)
}

val LocalNostrProfileHost = staticCompositionLocalOf<NostrProfileHost?> { null }
