nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.feed_session",
    crate_name: "nmp-feed-session",
    summary: "Shared feed-session compiler that maps reusable feed declarations to runtime-registered source graphs and feed controllers.",
    claims: [
        {
            claim_type: "mechanism",
            id: "feed.session_compiler",
            exclusive: true,
            scope: {
                kind: "type",
                value: "FeedSessionCompiler",
                context: "",
            },
            owns: [
                "FeedParams to source-graph compilation",
                "session-scoped feed dependency re-resolution",
                "runtime-independent feed controller registration plan",
            ],
        },
    ],
    notes: [
        {
            claim: "feed.session_compiler",
            text: "Product projection keys remain app-owned; browser and native runtimes only adapt slots, registries, and command seams through FeedSessionHost.",
        },
    ],
}
