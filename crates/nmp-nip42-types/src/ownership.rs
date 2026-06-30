nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.nip42_types",
    crate_name: "nmp-nip42-types",
    summary: "Dependency-free NIP-42 wire/type substrate: the RelayAuthState lifecycle enum and the AUTH/OK frame shapes + parsers. Shared verbatim by nmp-core (kernel-inlined FSM) and nmp-nip42 (standalone protocol module) so the two surfaces cannot drift. Depends on nothing in the workspace (serde / serde_json only).",
    claims: [
        {
            claim_type: "mechanism",
            id: "nip42.wire_types",
            exclusive: true,
            scope: {
                kind: "type",
                value: "RelayAuthState",
                context: "",
            },
            owns: [
                "NIP-42 shared wire/type vocabulary",
            ],
        },
    ],
    notes: [
    ],
}
