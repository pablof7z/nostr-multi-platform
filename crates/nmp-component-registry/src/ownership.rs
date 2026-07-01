nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.component_registry",
    crate_name: "nmp-component-registry",
    summary: "Canonical NMP component registry source packages, manifests, builtin lookup, and jsrepo export model.",
    claims: [
        {
            claim_type: "namespace",
            id: "component_registry.assets",
            exclusive: true,
            scope: {
                kind: "path",
                value: "registry/",
                context: "NMP component registry source packages",
            },
            owns: [
                "canonical component registry manifests and source assets",
                "builtin component source lookup",
                "jsrepo export data model",
            ],
        },
    ],
    notes: [
        {
            claim: "component_registry.assets",
            text: "nmp-cli owns command UX and install/update invocation; nmp-component-registry owns the upstream registry assets.",
        },
    ],
}
