nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.reposts",
    crate_name: "nmp-reposts",
    summary: "App-facing repost-count read owner: compiles the NIP-18 repost read plan and drives open_reposts/close_reposts on the read-lifecycle engine.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nmp.reposts.repost_read",
            exclusive: true,
            scope: {
                kind: "type",
                value: "RepostReadPlan",
                context: "",
            },
            owns: [
                "repost target resolution",
                "repost read plan construction and admission over nmp-nip18",
            ],
        },
        {
            claim_type: "projection",
            id: "projection.nmp.reposts.summary",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.reposts.summary",
                context: "",
            },
            owns: [
                "repost-summary read-model projection key family (open_reposts count read)",
            ],
        },
        {
            claim_type: "schema",
            id: "schema.nmp.reposts.summary",
            exclusive: true,
            scope: {
                kind: "schema",
                value: "nmp.reposts.summary",
                context: "",
            },
            owns: [
                "repost-summary FlatBuffers snapshot schema",
            ],
        },
    ],
    notes: [
        {
            claim: "nmp.reposts.repost_read",
            text: "This crate composes the repost-count read; nmp-nip18 owns kind:6/kind:16 repost wrapper decode and nmp-nip09 owns the kind:5 deletion grammar this crate's reducer folds.",
        },
    ],
}
