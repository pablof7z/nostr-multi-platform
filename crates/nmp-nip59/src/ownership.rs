nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip59",
    crate_name: "nmp-nip59",
    summary: "NIP-59 gift-wrap / seal / rumor. Shared envelope for NIP-17 DMs and Marmot Welcome delivery.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.1059.gift_wrap",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "1059",
                context: "",
            },
            owns: [
                "NIP-59 gift-wrap envelope construction and decoding",
            ],
        },
    ],
    notes: [
    ],
}
