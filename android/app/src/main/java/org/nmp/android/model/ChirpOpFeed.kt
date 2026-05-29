package org.nmp.android.model

// ─────────────────────────────────────────────────────────────────────────
// V-80 OP-centric home feed — Android model (ADR-0038 Stage T4 / B4).
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
// This is the SAME shape the iOS peer models in `ios/Chirp/.../TimelineBlock.swift`
// (`ChirpReplyAttribution` / `ChirpRootCard` / `ChirpTimelineSnapshot{cards,page}`).
// On Android the existing `ChirpTimelineSnapshot{blocks, cards}` name is still
// owned by the legacy NFTS/`TimelineScreen` render path (not yet migrated to the
// OP-centric shape — same stale-renderer class as iOS pre-rung-7, tracked as a
// V-84-class follow-up). To keep that render compiling, the typed `NOFS` decoder
// produces a DISTINCT [ChirpOpFeedSnapshot] type here rather than reshaping the
// legacy one. Decoder-only: nothing in the render reads these types yet.
// ─────────────────────────────────────────────────────────────────────────

/**
 * Raw attribution for one follow's reply to a feed root (mirror of Rust
 * `nmp_nip01::op_feed::Nip10ReplyAttribution`). Display fields fall back the
 * same way [ChirpEventCard] does: [authorDisplayName] is null until the
 * author's kind:0 arrives — the view formats the raw pubkey meanwhile
 * (ADR-0032 raw-data: the `has_*` companion bool distinguishes "absent (no
 * kind:0 yet)" from "present empty string").
 */
data class ChirpReplyAttribution(
    val authorPubkey: String = "",
    val authorDisplayName: String? = null,
    val authorPictureUrl: String? = null,
    val replyEventId: String = "",
    val replyCreatedAt: ULong = 0UL,
)

/**
 * One feed row: a root render card plus its raw attribution list (mirror of
 * Rust `nmp_feed::RootCard<C, A>`). The [attribution] list carries ALL
 * repliers raw; the renderer chooses how many to show (V-80 Q1) — the list
 * length IS the count, there is no separate total.
 */
data class ChirpRootCard(
    val card: ChirpEventCard,
    val attribution: List<ChirpReplyAttribution> = emptyList(),
)

/** A feed position — raw protocol hex event id plus its signed `created_at`. */
data class TimelineWindowCursor(
    val createdAt: ULong = 0UL,
    val id: String = "",
)

/** One page of the feed: the request bound plus the next opaque cursor. */
data class TimelineWindowPage(
    val limit: ULong = 0UL,
    val nextCursor: TimelineWindowCursor? = null,
    val hasMore: Boolean = false,
    val totalBlocks: ULong = 0UL,
)

/**
 * Decoded OP-centric home projection payload (`RootFeedSnapshot`). [page] is
 * null when the snapshot carries no paging envelope (the empty-feed case).
 */
data class ChirpOpFeedSnapshot(
    val cards: List<ChirpRootCard> = emptyList(),
    val page: TimelineWindowPage? = null,
)
