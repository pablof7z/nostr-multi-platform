nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.intent",
    crate_name: "nmp-intent",
    summary: "Input-intent resolver for NMP (#1804). Higher-order orchestrator: turns one untyped input string into a classified intent (direct reference / relay URL / NIP-05 shape / free-text search / registered recognizer). The recognizer trait + registry live noun-free in nmp-core::substrate::intent; this crate owns the orchestrator + generic parsers.",
    claims: [
        {
            claim_type: "mechanism",
            id: "intent.routing",
            exclusive: true,
            scope: {
                kind: "type",
                value: "IntentRouter",
                context: "",
            },
            owns: [
                "intent normalization and routing substrate",
            ],
        },
    ],
    notes: [
    ],
}
