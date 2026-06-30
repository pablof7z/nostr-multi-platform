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
    ],
    notes: [
    ],
}
