nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.kinds",
    crate_name: "nmp-kinds",
    summary: "Zero-dependency Layer-0 registry of canonical Nostr kind integers for the NMP workspace. Shared by nmp-core and every NIP crate (including nmp-nip59) without risk of a compile cycle: this crate depends on nothing in the workspace.",
    claims: [
        {
            claim_type: "namespace",
            id: "nostr.kind_integer_registry",
            exclusive: true,
            scope: {
                kind: "registry",
                value: "nmp-kinds",
                context: "",
            },
            owns: [
                "canonical numeric kind constants only",
                "no artifact construction semantics",
            ],
        },
    ],
    notes: [
    ],
}
