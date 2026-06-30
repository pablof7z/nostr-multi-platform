nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.coverage_gate",
    crate_name: "nmp-coverage-gate",
    summary: "D2 coverage-gate policy - thresholds and back-off rules for negentropy-before-REQ enforcement.",
    claims: [
        {
            claim_type: "mechanism",
            id: "coverage.negentropy_gate",
            exclusive: true,
            scope: {
                kind: "type",
                value: "CoverageGate",
                context: "",
            },
            owns: [
                "history coverage and sync eligibility gate semantics",
            ],
        },
    ],
    notes: [
    ],
}
