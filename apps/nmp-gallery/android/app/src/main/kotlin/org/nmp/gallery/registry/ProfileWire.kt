package org.nmp.gallery.registry

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Wire type for a Nostr user profile, decoded from the `nmp-profile`
 * projection emitted by the kernel.
 *
 * `npub` is Rust-formatted (the canonical bech32 NIP-19 encoder) — never
 * reformat it in Kotlin (aim.md §6.9). `npubShort` is NOT part of the wire
 * (#3098): abbreviation is pure string truncation of `npub`, a display
 * decision the host owns, so it is derived locally as a computed property.
 */
@Serializable
data class ProfileWire(
    @SerialName("pubkey") val pubkey: String,
    @SerialName("display_name") val displayName: String? = null,
    @SerialName("about") val about: String? = null,
    @SerialName("picture_url") val pictureUrl: String? = null,
    @SerialName("nip05") val nip05: String? = null,
    /** Full bech32 `npub1…` string. Use for copy / share. */
    @SerialName("npub") val npub: String,
) {
    /**
     * Locally-truncated npub (e.g. `npub1abcd…wxyz`): first 10 chars + `"…"`
     * + last 6 chars of [npub], unchanged when already short enough to fit.
     * Pure string truncation — never re-derives the bech32 encoding itself
     * (that stays Rust-owned), mirrors the shape
     * `nmp_core::display::short_npub` used to bake into this wire (#3098).
     */
    val npubShort: String
        get() = if (npub.length <= 17) npub else "${npub.take(10)}…${npub.takeLast(6)}"

    /** Stable display label: `displayName` if set, else `npubShort`. */
    val display: String get() = displayName?.takeIf { it.isNotEmpty() } ?: npubShort

    /** Avatar URL string; `null` when no picture is set or empty. */
    val avatarUrl: String? get() = pictureUrl?.takeIf { it.isNotEmpty() }
}
