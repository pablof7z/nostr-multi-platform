nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.cli",
    crate_name: "nmp-cli",
    summary: "NMP developer CLI: scaffold explicit app composition roots, install app-owned components, and inspect/upgrade the NMP dependency policy.",
    claims: [
        {
            claim_type: "namespace",
            id: "cli.nmp_command_surface",
            exclusive: true,
            scope: {
                kind: "command",
                value: "nmp",
                context: "",
            },
            owns: [
                "developer CLI command surface",
            ],
        },
    ],
    notes: [
    ],
}
