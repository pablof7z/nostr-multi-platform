package org.nmp.android.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// ─────────────────────────────────────────────────────────────────────────
// V-80 OP-centric home feed — Android model (ADR-0038, V-85 complete).
//
// `projections["nmp.feed.home"]` is the Rust `RootFeedSnapshot<
// TimelineEventCard, Nip10ReplyAttribution>` (`apps/chirp/nmp-app-chirp`
// re-exports it as `ChirpTimelineSnapshot`). Wire shape:
//
//   { "cards": [{ "card": ChirpEventCard, "attribution": [ChirpReplyAttribution] }],
//     "page": TimelineWindowPage?, "metrics": null }
//
// The feed is thread-ROOTS-only: every entry is one root. A followed user's
// reply to a non-followed author's note surfaces THAT note here, tagged with
// the replier in `attribution`. Replies never get their own row.
//
// V-85: these types now carry `@Serializable` so that `KernelUpdate` (also
// `@Serializable`) compiles cleanly with `modularTimeline: ChirpOpFeedSnapshot`.
// The generic JSON fallback path (ADR-0037 Commitment 4) can therefore decode
// the `modularTimeline` field directly from the Rust serde JSON — the field
// names are snake_case on the Rust side so `@SerialName` annotations map them.
// ─────────────────────────────────────────────────────────────────────────

/**
 * Raw attribution for one follow's reply to a feed root (mirror of Rust
 * `nmp_nip01::op_feed::Nip10ReplyAttribution`). Display fields fall back the
 * same way [ChirpEventCard] does: [authorDisplayName] is null until the
 * author's kind:0 arrives — the view formats the raw pubkey meanwhile
 * (ADR-0032 raw-data: the `has_*` companion bool distinguishes "absent (no
 * kind:0 yet)" from "present empty string").
 */
@Serializable
data class ChirpReplyAttribution(
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("author_picture_url") val authorPictureUrl: String? = null,
    @SerialName("reply_event_id") val replyEventId: String = "",
    @SerialName("reply_created_at") val replyCreatedAt: ULong = 0UL,
)

/**
 * One feed row: a root render card plus its raw attribution list (mirror of
 * Rust `nmp_feed::RootCard<C, A>`). The [attribution] list carries ALL
 * repliers raw; the renderer chooses how many to show (V-80 Q1) — the list
 * length IS the count, there is no separate total.
 */
@Serializable
data class ChirpRootCard(
    val card: ChirpEventCard,
    val attribution: List<ChirpReplyAttribution> = emptyList(),
)

/** A feed position — raw protocol hex event id plus its signed `created_at`. */
@Serializable
data class TimelineWindowCursor(
    @SerialName("created_at") val createdAt: ULong = 0UL,
    val id: String = "",
)

/** One page of the feed: the request bound plus the next opaque cursor. */
@Serializable
data class TimelineWindowPage(
    val limit: ULong = 0UL,
    @SerialName("next_cursor") val nextCursor: TimelineWindowCursor? = null,
    @SerialName("has_more") val hasMore: Boolean = false,
    @SerialName("total_blocks") val totalBlocks: ULong = 0UL,
)

/**
 * Decoded OP-centric home projection payload (`RootFeedSnapshot`). [page] is
 * null when the snapshot carries no paging envelope (the empty-feed case).
 * ADR-0037 Commitment 4: the generic `Value` path can populate this type from
 * the Rust JSON serde output when no NOFS typed projection is available.
 */
@Serializable
data class ChirpOpFeedSnapshot(
    val cards: List<ChirpRootCard> = emptyList(),
    val page: TimelineWindowPage? = null,
)
