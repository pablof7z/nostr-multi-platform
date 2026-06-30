nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip84",
    crate_name: "nmp-nip84",
    summary: "NIP-84 kind:9802 highlight publish action for NMP apps.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.9802.highlight",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "9802",
                context: "",
            },
            owns: [
                "NIP-84 highlight publish action semantics",
            ],
        },
    ],
    notes: [
    ],
}
