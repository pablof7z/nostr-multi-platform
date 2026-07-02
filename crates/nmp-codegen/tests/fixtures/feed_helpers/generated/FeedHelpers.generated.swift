// -----------------------------------------------------------------------------
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-codegen -- gen feed-helpers --platform swift \
//       --out <app>/Bridge/Generated/FeedHelpers.generated.swift
//
// Source of truth: `crates/nmp-codegen/src/feed_helpers.rs`.
//
// These helpers build canonical FeedParams JSON and call the existing
// openFeedJson binding. They do not own feed reactivity, compiler selection, or
// session teardown; Rust/NMP does.
// -----------------------------------------------------------------------------

import Foundation

public enum GeneratedFeedHelpers {
    public static func activeUserFollowsFeedParamsJson(
        feedKey: String,
        primaryKinds: [UInt32],
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .rootIndexed
    ) throws -> String {
        let limit = Int(visibleLimit)
        let params: [String: Any] = [
            "primary_kinds": primaryKinds.map(Int.init),
            "shape": shape.rawValue,
            "source": "ActiveUserFollows",
            "admission": "All",
            "order": "NewestByFeedPosition",
            "window": [
                "initial_limit": limit,
                "page_size": limit,
                "source_page_size": limit,
            ],
            "key": feedKey,
            "item_projection": "FeedRows",
        ]
        let data = try JSONSerialization.data(withJSONObject: params, options: [.sortedKeys])
        guard let json = String(data: data, encoding: .utf8) else {
            throw FeedHelperEncodingError.utf8
        }
        return json
    }

    public static func openActiveUserFollowsFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: [UInt32],
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .rootIndexed
    ) throws -> FeedSessionHandle {
        let paramsJson = try activeUserFollowsFeedParamsJson(
            feedKey: feedKey,
            primaryKinds: primaryKinds,
            visibleLimit: visibleLimit,
            shape: shape
        )
        return try app.openFeedJson(paramsJson: paramsJson)
    }
}

public enum FeedHelperShape: String {
    case rootIndexed = "RootIndexed"
    case flat = "Flat"
}

public enum FeedHelperEncodingError: Error {
    case utf8
}
