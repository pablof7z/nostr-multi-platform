nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip46",
    crate_name: "nmp-nip46",
    summary: "Transport-agnostic NIP-46 protocol core (pure event-reducer handshake + RPC helpers).",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip46.protocol_core",
            exclusive: true,
            scope: {
                kind: "namespace",
                value: "nip46",
                context: "",
            },
            owns: [
                "transport-agnostic NIP-46 reducer and RPC helpers",
            ],
        },
    ],
    notes: [
    ],
}
