nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.planner",
    crate_name: "nmp-planner",
    summary: "Subscription compiler: LogicalInterest, mailbox cache, lattice merge rules, per-relay plan projection. Extracted from nmp-core (see docs/architecture/crate-boundaries.md section 9).",
    claims: [
        {
            claim_type: "mechanism",
            id: "planner.interest_shape",
            exclusive: true,
            scope: {
                kind: "type",
                value: "LogicalInterest",
                context: "",
            },
            owns: [
                "logical interest shape semantics",
                "interest normalization",
            ],
        },
        {
            claim_type: "mechanism",
            id: "planner.filter_kinds",
            exclusive: true,
            scope: {
                kind: "field",
                value: "filter.kinds",
                context: "",
            },
            owns: [
                "kind-set as an opaque filter dimension",
                "kind-set merge behavior",
            ],
        },
        {
            claim_type: "mechanism",
            id: "planner.relay_pin",
            exclusive: true,
            scope: {
                kind: "field",
                value: "relay_pin",
                context: "",
            },
            owns: [
                "relay_pin merge rule",
                "relay-pinned partition behavior",
                "relay_pin plan-id hashing",
            ],
        },
        {
            claim_type: "mechanism",
            id: "planner.subscription_plan",
            exclusive: true,
            scope: {
                kind: "type",
                value: "SubscriptionPlan",
                context: "",
            },
            owns: [
                "compiled per-relay subscription plan construction",
            ],
        },
    ],
    notes: [
        {
            claim: "planner.filter_kinds",
            text: "Kinds are opaque filter values here; planner owns merge/routing semantics, not event artifact semantics.",
        },
    ],
}
