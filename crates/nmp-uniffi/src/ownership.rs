nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.uniffi",
    crate_name: "nmp-uniffi",
    summary: "UniFFI binding surface for the NMP native app lifecycle and byte action/update doorway (issue #2389, M14 scaffold).",
    claims: [
        {
            claim_type: "namespace",
            id: "ffi.uniffi_exports",
            exclusive: true,
            scope: {
                kind: "namespace",
                value: "nmp-uniffi",
                context: "",
            },
            owns: [
                "UniFFI export surface for NMP runtime bindings",
            ],
        },
    ],
    notes: [
    ],
}
