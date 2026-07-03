nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.devtools",
    crate_name: "nmp-devtools",
    summary: "Dev-only X-Ray diagnostic receipts over internal reconciliation facts; not linked by runtime or app-facing crates.",
    claims: [
        {
            claim_type: "mechanism",
            id: "devtools.xray_receipts",
            exclusive: true,
            scope: {
                kind: "type",
                value: "XrayReceipt",
                context: "",
            },
            owns: [
                "NMP-owned diagnostic receipt vocabulary",
                "bounded X-Ray receipt stream ordering",
            ],
        },
    ],
    notes: [
        {
            claim: "devtools.xray_receipts",
            text: "This crate may inspect private substrates to produce receipts, but exported receipt facts remain NMP-owned diagnostics rather than app surface.",
        },
    ],
}
