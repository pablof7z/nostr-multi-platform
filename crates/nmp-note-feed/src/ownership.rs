nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.note_feed",
    crate_name: "nmp-note-feed",
    summary: "Reusable note-feed composition that owns concrete OP/flat feed rows and typed feed wire by composing lower-level NIP facts with generic feed mechanics. Product projection keys are app-owned.",
    claims: [
        {
            claim_type: "namespace",
            id: "schema.nmp.note_feed.opfeed",
            exclusive: true,
            scope: {
                kind: "schema",
                value: "nmp.note_feed.opfeed",
                context: "",
            },
            owns: [
                "OP feed schema identity",
                "note-feed typed wire",
                "note-feed row payload",
            ],
        },
    ],
    notes: [
        {
            claim: "schema.nmp.note_feed.opfeed",
            text: "NIP-01 owns kind:1 facts and NIP-10 parsing; this crate owns concrete feed composition and NNFS wire. Product projection keys are app-owned.",
        },
    ],
}
