nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.threading",
    crate_name: "nmp-threading",
    summary: "Reply-convention-agnostic timeline grouping algorithm. Owns ThreadPointer / ParentResolver / ModulePolicy / TimelineBlock / Grouper, consumed by per-NIP wrapper view modules (NIP-10 in nmp-nip01). No app nouns, no kind semantics.",
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
    ],
    notes: [
    ],
}
