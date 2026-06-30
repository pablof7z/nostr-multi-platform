nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nostr_lmdb",
    crate_name: "nmp-nostr-lmdb",
    summary: "NMP-local fork of nostr-lmdb v0.44.1 with env-injection seam (PD-026 Option B)",
    claims: [
        {
            claim_type: "mechanism",
            id: "store.lmdb_backend",
            exclusive: true,
            scope: {
                kind: "backend",
                value: "lmdb",
                context: "",
            },
            owns: [
                "LMDB event-store backend implementation",
            ],
        },
    ],
    notes: [
    ],
}
