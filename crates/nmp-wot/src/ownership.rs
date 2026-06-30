nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.wot",
    crate_name: "nmp-wot",
    summary: "Client-side web-of-trust primitives for NMP: follow/mute graph scoring plus automatic replaceable-kind bootstrap interests.",
    claims: [
        {
            claim_type: "mechanism",
            id: "wot.scoring",
            exclusive: true,
            scope: {
                kind: "type",
                value: "WebOfTrust",
                context: "",
            },
            owns: [
                "follow/mute graph scoring and WoT bootstrap interests",
            ],
        },
    ],
    notes: [
    ],
}
