nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.testing",
    crate_name: "nmp-testing",
    summary: "NMP testing harnesses, doctrine lint gates, mock relays, stress tests, and release validation utilities.",
    claims: [
        {
            claim_type: "mechanism",
            id: "testing.doctrine_lint",
            exclusive: true,
            scope: {
                kind: "command",
                value: "doctrine-lint",
                context: "",
            },
            owns: [
                "doctrine lint smoke-test and policy checks",
            ],
        },
    ],
    notes: [
    ],
}
