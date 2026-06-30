nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip78",
    crate_name: "nmp-nip78",
    summary: "NIP-78 kind:30078 app-data mechanics: safe builder plus active-account raw app-data projection. App-specific keys and policy stay in app crates.",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.30078.app_data",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "30078",
                context: "",
            },
            owns: [
                "NIP-78 generic app-data event mechanics; app-specific keys stay app-owned",
            ],
        },
    ],
    notes: [
    ],
}
