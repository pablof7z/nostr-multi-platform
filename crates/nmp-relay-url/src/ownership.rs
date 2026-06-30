nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.relay_url",
    crate_name: "nmp-relay-url",
    summary: "Dependency-free relay-URL canonicalization vocabulary (Layer 0): the single workspace authority for normalizing a ws:///wss:// relay URL (lowercase scheme+host, strip empty-path trailing slash, fail-closed on a non-ws/wss or hostless URL). Owned here so every layer - planner (L2), network (L1), kernel (L3), protocol crates (L4) - shares one normalization and the keys they hand each other collide cleanly. Depends on nothing in the workspace.",
    claims: [
        {
            claim_type: "mechanism",
            id: "relay_url.canonicalization",
            exclusive: true,
            scope: {
                kind: "type",
                value: "RelayUrl",
                context: "",
            },
            owns: [
                "relay URL parsing and canonicalization",
            ],
        },
    ],
    notes: [
    ],
}
