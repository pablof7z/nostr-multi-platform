nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.codegen",
    crate_name: "nmp-codegen",
    summary: "NMP code generator for consumer-side Swift projection mirrors (KernelTypes + typed-FlatBuffer decoders) plus the nmp.toml manifest parser. The Rust-shell FFI-crate generator was removed by ADR-0046 (composition is a library, not a generator).",
    claims: [
        {
            claim_type: "mechanism",
            id: "codegen.ownership_audit",
            exclusive: true,
            scope: {
                kind: "command",
                value: "crate-ownership audit",
                context: "",
            },
            owns: [
                "workspace descriptor discovery",
                "exclusive scope collision audit",
            ],
        },
        {
            claim_type: "mechanism",
            id: "codegen.action_contract",
            exclusive: true,
            scope: {
                kind: "registry",
                value: "ACTION_CONTRACT",
                context: "",
            },
            owns: [
                "typed action contract registry",
            ],
        },
        {
            claim_type: "mechanism",
            id: "codegen.projection_contract",
            exclusive: true,
            scope: {
                kind: "registry",
                value: "PROJECTION_CONTRACT",
                context: "",
            },
            owns: [
                "projection contract registry",
            ],
        },
    ],
    notes: [
    ],
}
