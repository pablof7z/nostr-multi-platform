nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.signer_iface",
    crate_name: "nmp-signer-iface",
    summary: "Transport interface types shared by nmp-core and nmp-signers - SignerError, SignerOp, and the NIP-46 Nip46Rpc / Nip46Transport contract. Leaf crate: depends on nothing in the workspace so it can sit below the doctrine D0 boundary.",
    claims: [
        {
            claim_type: "mechanism",
            id: "signer.unsigned_event_vocabulary",
            exclusive: true,
            scope: {
                kind: "type",
                value: "UnsignedEvent",
                context: "",
            },
            owns: [
                "raw unsigned event value passed to signer backends",
                "no per-kind artifact semantics",
            ],
        },
        {
            claim_type: "mechanism",
            id: "signer.port",
            exclusive: true,
            scope: {
                kind: "trait",
                value: "SignerBackend",
                context: "",
            },
            owns: [
                "backend-transparent signing capability interface",
            ],
        },
    ],
    notes: [
    ],
}
