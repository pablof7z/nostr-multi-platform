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
// session teardown; Rust/NMP does.
// -----------------------------------------------------------------------------

package org.nmp.android

import uniffi.nmp_uniffi.FeedSessionHandle
import uniffi.nmp_uniffi.NmpApp

object GeneratedFeedHelpers {
    fun activeUserFollowsFeedParamsJson(
        feedKey: String,
        primaryKinds: List<Int>,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
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
            append("\"source\":\"ActiveUserFollows\",")
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

    fun openActiveUserFollowsFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: List<Int>,
        visibleLimit: Int = 80,
        shape: FeedHelperShape = FeedHelperShape.RootIndexed,
    ): FeedSessionHandle {
        val paramsJson = activeUserFollowsFeedParamsJson(feedKey, primaryKinds, visibleLimit, shape)
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
