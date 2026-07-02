nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.reactions",
    crate_name: "nmp-reactions",
    summary: "App-facing reaction-count read owner: composes the NIP-25 kind:7/kind:5 fold into open_reactions/close_reactions on the shared read-lifecycle engine.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nmp.reactions.reaction_count_read",
            exclusive: true,
            scope: {
                kind: "type",
                value: "ReactionTarget",
                context: "",
            },
            owns: [
                "reaction-count active read composition (open_reactions)",
            ],
        },
        {
            claim_type: "projection",
            id: "projection.nmp.reactions.summary",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.reactions.summary",
                context: "",
            },
            owns: [
                "reaction-summary read-model projection key family (open_reactions count read)",
            ],
        },
        {
            claim_type: "schema",
            id: "schema.nmp.reactions.summary",
            exclusive: true,
            scope: {
                kind: "schema",
                value: "nmp.reactions.summary",
                context: "",
            },
            owns: [
                "reaction-summary FlatBuffers snapshot schema",
            ],
        },
    ],
    notes: [
        {
            claim: "nmp.reactions.reaction_count_read",
            text: "This crate composes the read; nmp-nip25 owns kind:7 reaction semantics and the underlying ReactionAggregateProjection fold (including kind:5 retraction handling), which this crate reuses unmodified.",
        },
    ],
}
