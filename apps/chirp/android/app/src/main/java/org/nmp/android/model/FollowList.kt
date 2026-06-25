package org.nmp.android.model

import kotlinx.serialization.Serializable

/**
 * Typed mirror of the Rust-owned `nmp.follow_list` projection (`NF02`).
 * Carries raw hex pubkeys only; UI formatting and labels stay presentation-only.
 */
@Serializable
data class FollowListSnapshot(
    val follows: List<String> = emptyList(),
)
