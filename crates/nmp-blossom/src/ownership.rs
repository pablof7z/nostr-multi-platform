nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.blossom",
    crate_name: "nmp-blossom",
    summary: "Blossom (BUD-02) blob uploads as an NMP protocol crate - kind:24242 auth builder + BlossomUploadCommand (ProtocolCommand) streaming PUT, signing via the generic backend-transparent SignEventForAccount port (ADR-0043).",
    claims: [
        {
            claim_type: "artifact",
            id: "nostr.kind.24242.blossom_auth",
            exclusive: true,
            scope: {
                kind: "kind",
                value: "24242",
                context: "",
            },
            owns: [
                "Blossom authorization event builder and upload command",
            ],
        },
        {
            claim_type: "namespace",
            id: "action.nmp.blossom.upload",
            exclusive: true,
            scope: {
                kind: "action",
                value: "nmp.blossom.upload",
                context: "",
            },
            owns: [
                "Blossom upload action namespace",
            ],
        },
    ],
    notes: [
    ],
}
