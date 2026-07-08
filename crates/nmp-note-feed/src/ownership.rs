nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.note_feed",
    crate_name: "nmp-note-feed",
    summary: "Thin protocol-composition adapter (post-demolition): supplies NIP-01/NIP-18 knobs (identity/merge/predicates) and a PROVISIONAL feed-row wire for the generic nmp-feed FlatFeed engine. Owns no engine and no authoritative row. See #3082.",
    claims: [
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
                "provisional feed-row wire identity (NFRW)",
            ],
        },
    ],
    notes: [
        {
            claim: "schema.nmp.feed.feed_row",
            text: "PROVISIONAL wire pending #3082. The generic FeedRow, snapshot type, and FlatFeed engine are owned by nmp-feed; this crate only composes NIP facts into row knobs.",
        },
    ],
}
