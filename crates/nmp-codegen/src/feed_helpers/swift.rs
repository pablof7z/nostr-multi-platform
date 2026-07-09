//! Swift feed-helper emitter.

/// Render `FeedHelpers.generated.swift`.
#[must_use]
pub fn render() -> String {
    r#"// -----------------------------------------------------------------------------
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
// feed teardown; Rust/NMP does.
// -----------------------------------------------------------------------------

import Foundation

public enum GeneratedFeedHelpers {
    private static func buildFeedParamsJson(
        feedKey: String,
        primaryKinds: [UInt32],
        source: Any,
        visibleLimit: UInt32,
        shape: FeedHelperShape
    ) throws -> String {
        let limit = Int(visibleLimit)
        let params: [String: Any] = [
            "primary_kinds": primaryKinds.map(Int.init),
            "shape": shape.rawValue,
            "source": source,
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

    public static func activeUserFollowsFeedParamsJson(
        feedKey: String,
        primaryKinds: [UInt32],
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .flat
    ) throws -> String {
        try buildFeedParamsJson(
            feedKey: feedKey,
            primaryKinds: primaryKinds,
            source: "ActiveUserFollows",
            visibleLimit: visibleLimit,
            shape: shape
        )
    }

    public static func openActiveUserFollowsFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: [UInt32],
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .flat
    ) throws -> FeedHandle {
        let paramsJson = try activeUserFollowsFeedParamsJson(
            feedKey: feedKey,
            primaryKinds: primaryKinds,
            visibleLimit: visibleLimit,
            shape: shape
        )
        return try app.openFeedJson(paramsJson: paramsJson)
    }

    /// The active account's hosted-group set (one row family per joined
    /// relay-hosted group). See `FeedSourceExpr::ActiveUserHostedGroups`.
    public static func hostedGroupsFeedParamsJson(
        feedKey: String,
        primaryKinds: [UInt32],
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .flat
    ) throws -> String {
        try buildFeedParamsJson(
            feedKey: feedKey,
            primaryKinds: primaryKinds,
            source: "ActiveUserHostedGroups",
            visibleLimit: visibleLimit,
            shape: shape
        )
    }

    public static func openHostedGroupsFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: [UInt32],
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .flat
    ) throws -> FeedHandle {
        let paramsJson = try hostedGroupsFeedParamsJson(
            feedKey: feedKey,
            primaryKinds: primaryKinds,
            visibleLimit: visibleLimit,
            shape: shape
        )
        return try app.openFeedJson(paramsJson: paramsJson)
    }

    /// Members of an app/defaults-registered list id. See
    /// `FeedSourceExpr::ListMembers`.
    public static func listMembersFeedParamsJson(
        feedKey: String,
        primaryKinds: [UInt32],
        listId: String,
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .flat
    ) throws -> String {
        try buildFeedParamsJson(
            feedKey: feedKey,
            primaryKinds: primaryKinds,
            source: ["ListMembers": ["list": listId]],
            visibleLimit: visibleLimit,
            shape: shape
        )
    }

    public static func openListMembersFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: [UInt32],
        listId: String,
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .flat
    ) throws -> FeedHandle {
        let paramsJson = try listMembersFeedParamsJson(
            feedKey: feedKey,
            primaryKinds: primaryKinds,
            listId: listId,
            visibleLimit: visibleLimit,
            shape: shape
        )
        return try app.openFeedJson(paramsJson: paramsJson)
    }

    /// An app-registered relay set. See `FeedSourceExpr::RelaySet`.
    public static func relaySetFeedParamsJson(
        feedKey: String,
        primaryKinds: [UInt32],
        relaySetId: String,
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .flat
    ) throws -> String {
        try buildFeedParamsJson(
            feedKey: feedKey,
            primaryKinds: primaryKinds,
            source: ["RelaySet": ["relays": relaySetId]],
            visibleLimit: visibleLimit,
            shape: shape
        )
    }

    public static func openRelaySetFeed(
        app: NmpApp,
        feedKey: String,
        primaryKinds: [UInt32],
        relaySetId: String,
        visibleLimit: UInt32 = 80,
        shape: FeedHelperShape = .flat
    ) throws -> FeedHandle {
        let paramsJson = try relaySetFeedParamsJson(
            feedKey: feedKey,
            primaryKinds: primaryKinds,
            relaySetId: relaySetId,
            visibleLimit: visibleLimit,
            shape: shape
        )
        return try app.openFeedJson(paramsJson: paramsJson)
    }
}

public enum FeedHelperShape: String {
    case flat = "Flat"
}

public enum FeedHelperEncodingError: Error {
    case utf8
}
"#
    .to_string()
}
