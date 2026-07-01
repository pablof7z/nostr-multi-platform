nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip50",
    crate_name: "nmp-nip50",
    summary: "NIP-50 search request and result-projection primitives for NMP. Core/planner carry the generic search filter; this crate owns NIP-50 search semantics.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip50.search",
            exclusive: true,
            scope: {
                kind: "filter",
                value: "search",
                context: "",
            },
            owns: [
                "NIP-50 search request and result projection semantics",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.nip50.search",
            exclusive: true,
            scope: {
                kind: "projection_family",
                value: "nmp.nip50.search.*",
                context: "",
            },
            owns: [
                "per-session NIP-50 search result projection family",
            ],
        },
    ],
    notes: [
    ],
}
