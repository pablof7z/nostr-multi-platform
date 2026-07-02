// -----------------------------------------------------------------------------
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen feed-helpers --platform kotlin \
//       --out <app>/src/main/java/<package>/FeedHelpers.kt
//
// Source of truth: `crates/nmp-codegen/src/feed_helpers.rs`.
//
// These helpers build canonical FeedParams JSON and call the existing
// openFeedJson binding. They do not own feed reactivity, compiler selection, or
// feed teardown; Rust/NMP does.
// -----------------------------------------------------------------------------

package org.nmp.android

import uniffi.nmp_uniffi.FeedHandle
import uniffi.nmp_uniffi.NmpApp

object GeneratedFeedHelpers {
    private fun buildFeedParamsJson(
        feedKey: String,
        primaryKinds: List<Int>,
        sourceJson: String,
        visibleLimit: Int,
        shape: FeedHelperShape,
    ): String {
        val kinds = primaryKinds.joinToString(",")
        return buildString {
            append("{")
            append("\"primary_kinds\":[")
            append(kinds)
            append("],")
            append("\"shape\":")
            append(jsonString(shape.wireValue))
            append(",")
            append("\"source\":")
            append(sourceJson)
            append(",")
            append("\"admission\":\"All\",")
            append("\"order\":\"NewestByFeedPosition\",")
            append("\"window\":{")
            append("\"initial_limit\":")
            append(visibleLimit)
            append(",\"page_size\":")
            append(visibleLimit)
            append(",\"source_page_size\":")
            append(visibleLimit)
            append("},")
            append("\"key\":")
            append(jsonString(feedKey))
            append(",")
            append("\"item_projection\":\"FeedRows\"")
            append("}")
        }
    }

    fun activeUserFollowsFeedParamsJson(
        feedKey: String,
        primaryKinds: List<Int>,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
    ): String = buildFeedParamsJson(feedKey, primaryKinds, "\"ActiveUserFollows\"", visibleLimit, shape)

    fun openActiveUserFollowsFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: List<Int>,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
    ): FeedHandle {
        val paramsJson = activeUserFollowsFeedParamsJson(feedKey, primaryKinds, visibleLimit, shape)
        return app.openFeedJson(paramsJson)
    }

    /** The active account's hosted-group set. See `FeedSourceExpr::ActiveUserHostedGroups`. */
    fun hostedGroupsFeedParamsJson(
        feedKey: String,
        primaryKinds: List<Int>,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
    ): String = buildFeedParamsJson(feedKey, primaryKinds, "\"ActiveUserHostedGroups\"", visibleLimit, shape)

    fun openHostedGroupsFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: List<Int>,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
    ): FeedHandle {
        val paramsJson = hostedGroupsFeedParamsJson(feedKey, primaryKinds, visibleLimit, shape)
        return app.openFeedJson(paramsJson)
    }

    /** Members of an app/defaults-registered list id. See `FeedSourceExpr::ListMembers`. */
    fun listMembersFeedParamsJson(
        feedKey: String,
        primaryKinds: List<Int>,
        listId: String,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
    ): String {
        val sourceJson = "{\"ListMembers\":{\"list\":${jsonString(listId)}}}"
        return buildFeedParamsJson(feedKey, primaryKinds, sourceJson, visibleLimit, shape)
    }

    fun openListMembersFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: List<Int>,
        listId: String,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
    ): FeedHandle {
        val paramsJson = listMembersFeedParamsJson(feedKey, primaryKinds, listId, visibleLimit, shape)
        return app.openFeedJson(paramsJson)
    }

    /** An app-registered relay set. See `FeedSourceExpr::RelaySet`. */
    fun relaySetFeedParamsJson(
        feedKey: String,
        primaryKinds: List<Int>,
        relaySetId: String,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
    ): String {
        val sourceJson = "{\"RelaySet\":{\"relays\":${jsonString(relaySetId)}}}"
        return buildFeedParamsJson(feedKey, primaryKinds, sourceJson, visibleLimit, shape)
    }

    fun openRelaySetFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: List<Int>,
        relaySetId: String,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
    ): FeedHandle {
        val paramsJson = relaySetFeedParamsJson(feedKey, primaryKinds, relaySetId, visibleLimit, shape)
        return app.openFeedJson(paramsJson)
    }

    private fun jsonString(value: String): String {
        return buildString {
            append('"')
            for (ch in value) {
                when (ch) {
                    '\\' -> append("\\\\")
                    '"' -> append("\\\"")
                    '\b' -> append("\\b")
                    '\n' -> append("\\n")
                    '\r' -> append("\\r")
                    '\t' -> append("\\t")
                    else -> {
                        if (ch < ' ') {
                            append("\\u")
                            append(ch.code.toString(16).padStart(4, '0'))
                        } else {
                            append(ch)
                        }
                    }
                }
            }
            append('"')
        }
    }
}

enum class FeedHelperShape(val wireValue: String) {
    RootIndexed("RootIndexed"),
    Flat("Flat"),
}
