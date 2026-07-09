nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.feed",
    crate_name: "nmp-feed",
    summary: "Reusable bounded Nostr feed windowing, cursor paging, and feed controller registry for NMP apps.",
    claims: [
        {
            claim_type: "mechanism",
            id: "feed.timeline_assembly",
            exclusive: true,
            scope: {
                kind: "type",
                value: "FeedProjection",
                context: "",
            },
            owns: [
                "feed item assembly and timeline projection semantics",
            ],
        },
        {
            claim_type: "namespace",
            id: "schema.nmp.feed.feed_row",
            exclusive: true,
            scope: {
                kind: "schema",
                value: "nmp.feed.feed_row",
                context: "",
            },
            owns: [
                "the frozen FeedRow FlatBuffers wire (NFRS)",
            ],
        },
    ],
    notes: [
        {
            claim: "schema.nmp.feed.feed_row",
            text: "FROZEN (#3082 settled design). The generic FeedRow, TypedRef/DeliveryMode, composite-feed declaration surface, and the FlatBuffers wire all live in this crate. Protocol crates only compose NIP facts into row/lane-mapping knobs; none owns a wire (#3092 deleted the last thin adapter crate that used to make this claim).",
        },
    ],
}
