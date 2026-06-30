nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.native_runtime",
    crate_name: "nmp-native-runtime",
    summary: "Native Rust runtime owner for NMP applications: NmpApp, actor lifecycle, runtime slots, native builder, and AppHost integration.",
    claims: [
        {
            claim_type: "mechanism",
            id: "native_runtime.capability_bridge",
            exclusive: true,
            scope: {
                kind: "type",
                value: "NmpDefaultRuntimeHandles",
                context: "",
            },
            owns: [
                "native runtime capability bridge and projection forwarding",
            ],
        },
    ],
    notes: [
    ],
}
