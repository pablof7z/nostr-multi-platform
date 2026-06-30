nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip11",
    crate_name: "nmp-nip11",
    summary: "NIP-11 relay information documents as an NMP protocol crate - automatic fetch on relay connect + on-demand probe, surfaced through the relay diagnostics projection. nmp-core learns no NIP-11 noun and imports no HTTP crate (D0).",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip11.relay_information",
            exclusive: true,
            scope: {
                kind: "document",
                value: "nip11",
                context: "",
            },
            owns: [
                "relay information document fetch and diagnostics projection",
            ],
        },
    ],
    notes: [
    ],
}
