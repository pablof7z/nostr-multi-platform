nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.threading",
    crate_name: "nmp-threading",
    summary: "Reply-convention-agnostic timeline grouping algorithm and the reactive nmp.threading.graph e-tag threading read model. Owns ThreadPointer / ParentResolver / ModulePolicy / TimelineBlock / Grouper / ThreadingProjection. No app nouns, no kind semantics.",
    claims: [
        {
            claim_type: "mechanism",
            id: "threading.thread_reconstruction",
            exclusive: true,
            scope: {
                kind: "type",
                value: "ThreadView",
                context: "",
            },
            owns: [
                "thread reconstruction and reply-tree semantics",
            ],
        },
        {
            claim_type: "namespace",
            id: "projection.nmp.threading.graph",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "nmp.threading.graph",
                context: "",
            },
            owns: [
                "reactive e-tag threading-graph projection key",
            ],
        },
    ],
    notes: [
    ],
}
