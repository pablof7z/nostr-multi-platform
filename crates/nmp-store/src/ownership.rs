nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.store",
    crate_name: "nmp-store",
    summary: "EventStore trait + MemEventStore / LmdbEventStore backends. Extracted from nmp-core (see docs/architecture/crate-boundaries.md section 9).",
    claims: [
        {
            claim_type: "mechanism",
            id: "store.event_query",
            exclusive: true,
            scope: {
                kind: "type",
                value: "EventStore",
                context: "",
            },
            owns: [
                "event admission, storage query, and replacement/delete folding semantics",
            ],
        },
    ],
    notes: [
    ],
}
