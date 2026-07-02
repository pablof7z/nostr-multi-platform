nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.threading",
    crate_name: "nmp-threading",
    summary: "Reply-convention-agnostic timeline grouping and reactive threading graph projection. Owns ThreadPointer / ParentResolver / ModulePolicy / TimelineBlock / Grouper and the nmp.threading.graph.* projection family. No app nouns, no kind semantics.",
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
                kind: "projection_family",
                value: "nmp.threading.graph.*",
                context: "",
            },
            owns: [
                "per-session reactive threading graph projection family",
            ],
        },
        {
            claim_type: "schema",
            id: "schema.nmp.threading.graph",
            exclusive: true,
            scope: {
                kind: "schema_id",
                value: "nmp.threading.graph",
                context: "",
            },
            owns: [
                "typed FlatBuffers schema for threading graph snapshots",
            ],
        },
    ],
    notes: [
    ],
}
