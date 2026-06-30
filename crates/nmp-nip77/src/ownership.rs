nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip77",
    crate_name: "nmp-nip77",
    summary: "NIP-77 negentropy reconciliation primitives and runtime adapter for NMP.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip77.negentropy_runtime",
            exclusive: true,
            scope: {
                kind: "protocol",
                value: "negentropy",
                context: "",
            },
            owns: [
                "NIP-77 reconciliation primitives and runtime adapter",
            ],
        },
    ],
    notes: [
    ],
}
