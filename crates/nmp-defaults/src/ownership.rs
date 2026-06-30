nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.defaults",
    crate_name: "nmp-defaults",
    summary: "Reusable NMP substrate and protocol composition installers. Production app roots call named installers such as register_substrate; register_defaults remains a compatibility/tutorial preset while apps migrate to ADR-0069 explicit composition.",
    claims: [
        {
            claim_type: "mechanism",
            id: "defaults.standard_composition",
            exclusive: true,
            scope: {
                kind: "function",
                value: "register_defaults",
                context: "",
            },
            owns: [
                "standard NMP crate composition installer",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.app.topic_articles",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.app.topic_articles",
                context: "",
            },
            owns: [
                "topic articles example action namespace",
            ],
        },
    ],
    notes: [
    ],
}
