nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.mint_discovery",
    crate_name: "nmp-mint-discovery",
    summary: "WoT-scoped NIP-87 mint discovery: composes nmp-nip87 (kind:38172/38000 codecs) with nmp-wot (score_rooted trust scoring) into a capability-fail-closed discovered-mints view. Owns its own DiscoveredMint model, read interests, memoized aggregation store, and mint_discovery typed projection.",
    claims: [
        {
            claim_type: "namespace",
            id: "projection.mint_discovery",
            exclusive: true,
            scope: {
                kind: "projection",
                value: "mint_discovery",
                context: "",
            },
            owns: [
                "mint discovery projection key and typed FlatBuffers sidecar shape (NMDS)",
            ],
        },
        {
            claim_type: "mechanism",
            id: "mint_discovery.aggregation",
            exclusive: true,
            scope: {
                kind: "namespace",
                value: "nmp-mint-discovery",
                context: "",
            },
            owns: [
                "WoT-scoped, capability-fail-closed mint-discovery aggregation policy, its read interests, and the memoized MintDiscoveryStore",
            ],
        },
    ],
    notes: [
        {
            claim: "mint_discovery.aggregation",
            text: "Extracted from nmp-wallet (#2880 unwind, epic #2864) so any Nostr app can compose mint discovery without depending on the wallet product. nmp-wallet no longer depends on nmp-nip87 or nmp-wot.",
        },
    ],
}
