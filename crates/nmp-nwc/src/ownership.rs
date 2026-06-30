nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nwc",
    crate_name: "nmp-nwc",
    summary: "NIP-47 Nostr Wallet Connect client - URI parsing, NIP-44 encrypted request/response, kind:23194 builder, kind:23195 decoder.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nwc.client_adapter",
            exclusive: true,
            scope: {
                kind: "namespace",
                value: "nmp-nwc",
                context: "",
            },
            owns: [
                "Nostr Wallet Connect client adapter",
            ],
        },
    ],
    notes: [
    ],
}
