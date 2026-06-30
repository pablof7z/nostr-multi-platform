nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.note_feed",
    crate_name: "nmp-note-feed",
    summary: "Reusable note-feed composition that owns concrete OP/flat feed rows, typed feed wire, and feed projection keys by composing lower-level NIP facts with generic feed mechanics.",
    claims: [
        {
            claim_type: "namespace",
            id: "projection.nmp.feed.home",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.feed.home",
                context: "",
            },
            owns: [
                "home feed projection key",
                "note-feed typed wire",
                "note-feed row payload",
            ],
        },
    ],
    notes: [
        {
            claim: "projection.nmp.feed.home",
            text: "NIP-01 owns kind:1 facts and NIP-10 parsing; this crate owns concrete feed composition.",
        },
    ],
}
