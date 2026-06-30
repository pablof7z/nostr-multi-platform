nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.signers",
    crate_name: "nmp-signers",
    summary: "Signer trait + Local nsec / NIP-46 bunker / NIP-07 implementations + multi-account AccountManager for nmp-core.",
    claims: [
        {
            claim_type: "mechanism",
            id: "signers.implementations",
            exclusive: true,
            scope: {
                kind: "namespace",
                value: "nmp-signers",
                context: "",
            },
            owns: [
                "local and remote signer backend implementations",
            ],
        },
    ],
    notes: [
    ],
}
