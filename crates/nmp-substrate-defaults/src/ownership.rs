nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.substrate_defaults",
    crate_name: "nmp-substrate-defaults",
    summary: "Wasm-safe default substrate cache/parser wiring shared by nmp-defaults and reducer-owned web composition roots.",
    claims: [
        {
            claim_type: "mechanism",
            id: "defaults.substrate_composition",
            exclusive: true,
            scope: {
                kind: "function",
                value: "register_substrate_defaults",
                context: "",
            },
            owns: [
                "substrate default installer",
            ],
        },
    ],
    notes: [
    ],
}
