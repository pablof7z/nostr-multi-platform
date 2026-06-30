nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.core",
    crate_name: "nmp-core",
    summary: "NMP kernel substrate: actor runtime, state, capabilities, action registry, snapshots, and protocol extension seams.",
    claims: [
        {
            claim_type: "mechanism",
            id: "core.actor_runtime",
            exclusive: true,
            scope: {
                kind: "type",
                value: "AppHost",
                context: "",
            },
            owns: [
                "kernel actor lifecycle",
                "single-writer action dispatch",
            ],
        },
        {
            claim_type: "mechanism",
            id: "core.publish_pipeline",
            exclusive: true,
            scope: {
                kind: "type",
                value: "PublishCommand",
                context: "",
            },
            owns: [
                "signing and publish command pipeline",
            ],
        },
        {
            claim_type: "mechanism",
            id: "core.projection_substrate",
            exclusive: true,
            scope: {
                kind: "type",
                value: "ObservedProjectionSink",
                context: "",
            },
            owns: [
                "projection registration and snapshot emission",
            ],
        },
        {
            claim_type: "namespace",
            id: "core.action_registry",
            exclusive: true,
            scope: {
                kind: "registry",
                value: "ActionRegistry",
                context: "",
            },
            owns: [
                "typed action module registry",
            ],
        },
    ],
    notes: [
    ],
}
