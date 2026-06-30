crate::declare_crate_ownership! {
    owner_id: "nmp.ownership",
    crate_name: "nmp-ownership",
    summary: "Typed positive ownership descriptors for NMP crates and app crates.",
    claims: [
        {
            claim_type: "mechanism",
            id: "ownership.descriptor_macro",
            exclusive: true,
            scope: {
                kind: "macro",
                value: "declare_crate_ownership",
                context: "",
            },
            owns: [
                "typed positive ownership descriptor declaration",
            ],
        },
        {
            claim_type: "mechanism",
            id: "ownership.claim_model",
            exclusive: true,
            scope: {
                kind: "type",
                value: "CrateOwnershipDescriptor",
                context: "",
            },
            owns: [
                "artifact/envelope/mechanism/namespace claim vocabulary",
            ],
        },
    ],
    notes: [
    ],
}
