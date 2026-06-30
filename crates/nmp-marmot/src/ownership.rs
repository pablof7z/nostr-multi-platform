nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.marmot",
    crate_name: "nmp-marmot",
    summary: "Marmot Protocol (MLS-over-Nostr) as a Layer-4 NIP crate. Sole importer of mdk-core/openmls; adapts MDK types to NMP Domain/View module contracts. Mutating ops install a typed action module under the nmp.marmot namespace. API surface: docs/research/mdk-api.md.",
    claims: [
        {
            claim_type: "artifact",
            id: "marmot.kind.30443.key_package",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "30443",
                context: "",
            },
            owns: [
                "Marmot key package event semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "marmot.kind.444.welcome_rumor",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "444",
                context: "",
            },
            owns: [
                "Marmot welcome rumor semantics",
            ],
        },
        {
            claim_type: "artifact",
            id: "marmot.kind.445.group_message",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "445",
                context: "",
            },
            owns: [
                "Marmot group message/commit/proposal semantics",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.marmot",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.marmot",
                context: "",
            },
            owns: [
                "Marmot action namespace family",
            ],
        },
    ],
    notes: [
    ],
}
