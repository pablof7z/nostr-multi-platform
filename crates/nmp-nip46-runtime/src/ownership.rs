nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip46_runtime",
    crate_name: "nmp-nip46-runtime",
    summary: "NIP-46 actor-lane runtime - drives the nmp-nip46 reducer over the actor relay lane (Layer-4). PR-B1: sign-ready with real Nip46Signer + multi-relay + tested end-to-end.",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip46.actor_runtime",
            exclusive: true,
            scope: {
                kind: "type",
                value: "Nip46Runtime",
                context: "",
            },
            owns: [
                "actor-lane NIP-46 runtime driver",
            ],
        },
    ],
    notes: [
    ],
}
